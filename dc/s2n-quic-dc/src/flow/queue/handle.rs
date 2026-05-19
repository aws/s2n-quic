// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{
    descriptor::Descriptor,
    inner::{Closed, Error, Half},
    probes, AutoWake,
};
use crate::intrusive;
use core::{
    fmt,
    task::{Context, Poll},
};
use s2n_quic_core::varint::VarInt;

macro_rules! impl_recv {
    ($name:ident, $field:ident, $half:expr, $type_param:ident) => {
        pub struct $name<S: 'static, C: 'static, Key: 'static> {
            descriptor: Descriptor<S, C, Key>,
        }

        impl<S: 'static, C: 'static, Key: 'static> $name<S, C, Key> {
            #[inline]
            pub(super) fn new(descriptor: Descriptor<S, C, Key>) -> Self {
                Self { descriptor }
            }

            /// Returns the associated `queue_id` for the channel
            ///
            /// This can be sent to a peer, which can be used to route packets back to the channel.
            #[inline]
            pub fn queue_id(&self) -> VarInt {
                unsafe { self.descriptor.queue_id() }
            }

            /// Returns the peer's queue ID, or `None` if not yet observed from a packet.
            #[inline]
            pub fn remote_queue_id(&self) -> Option<VarInt> {
                unsafe { self.descriptor.remote_queue_id() }
            }

            #[inline]
            pub fn push(&self, value: intrusive::Entry<$type_param>) {
                unsafe {
                    let res = self.descriptor.$field().push(value, || false, || Ok(()));
                    debug_assert!(res.is_ok());
                    probes::on_send(self.descriptor.queue_id(), $half, true);
                }
            }

            #[inline]
            pub fn try_recv(&self) -> Result<Option<intrusive::Entry<$type_param>>, Closed> {
                unsafe {
                    let value = self.descriptor.$field().pop()?;
                    probes::on_recv(self.descriptor.queue_id(), $half, value.is_some().into());
                    Ok(value)
                }
            }

            #[inline]
            pub fn try_swap(&self) -> Result<intrusive::Queue<$type_param>, Closed> {
                unsafe {
                    let queue = self.descriptor.$field().try_swap()?;
                    probes::on_recv(self.descriptor.queue_id(), $half, queue.len());
                    Ok(queue)
                }
            }

            #[inline]
            pub async fn recv(&self) -> Result<intrusive::Entry<$type_param>, Closed> {
                core::future::poll_fn(|cx| self.poll_recv(cx)).await
            }

            #[inline]
            pub fn poll_recv(
                &self,
                cx: &mut Context,
            ) -> Poll<Result<intrusive::Entry<$type_param>, Closed>> {
                unsafe {
                    match self.descriptor.$field().poll_pop(cx) {
                        Poll::Ready(Ok(entry)) => {
                            probes::on_recv(self.descriptor.queue_id(), $half, 1);
                            Poll::Ready(Ok(entry))
                        }
                        Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
                        Poll::Pending => {
                            probes::on_recv(self.descriptor.queue_id(), $half, 0);
                            Poll::Pending
                        }
                    }
                }
            }

            #[inline]
            pub fn poll_swap(
                &self,
                cx: &mut Context,
            ) -> Poll<Result<intrusive::Queue<$type_param>, Closed>> {
                unsafe {
                    match self.descriptor.$field().poll_swap(cx) {
                        Poll::Ready(Ok(queue)) => {
                            probes::on_recv(self.descriptor.queue_id(), $half, queue.len());
                            Poll::Ready(Ok(queue))
                        }
                        Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
                        Poll::Pending => {
                            probes::on_recv(self.descriptor.queue_id(), $half, 0);
                            Poll::Pending
                        }
                    }
                }
            }
        }

        impl<S: 'static, C: 'static, Key: 'static> fmt::Debug for $name<S, C, Key> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.debug_struct(stringify!($name))
                    .field("queue_id", &self.queue_id())
                    .finish()
            }
        }

        impl<S: 'static, C: 'static, Key: 'static> Drop for $name<S, C, Key> {
            #[inline]
            fn drop(&mut self) {
                unsafe {
                    self.descriptor.drop_receiver($half);
                }
            }
        }
    };
}

impl_recv!(Control, control_queue, Half::Control, C);
impl_recv!(Stream, stream_queue, Half::Stream, S);

pub struct Sender<S: 'static, C: 'static, Key: 'static> {
    descriptor: Descriptor<S, C, Key>,
}

impl<S: 'static, C: 'static, Key: 'static> Clone for Sender<S, C, Key> {
    #[inline]
    fn clone(&self) -> Self {
        unsafe {
            Self {
                descriptor: self.descriptor.clone_for_sender(),
            }
        }
    }
}

impl<S: 'static, C: 'static, Key: 'static> Sender<S, C, Key> {
    #[inline]
    pub(super) fn new(descriptor: Descriptor<S, C, Key>) -> Self {
        Self { descriptor }
    }

    #[inline]
    pub fn send_stream(
        &self,
        entry: intrusive::Entry<S>,
        remote_queue_id: Option<VarInt>,
        params: &<Key as super::descriptor::Key>::Request,
    ) -> Result<AutoWake, Error<intrusive::Entry<S>>>
    where
        Key: super::descriptor::Key,
    {
        unsafe {
            let waker = self.descriptor.stream_queue().push(
                entry,
                || {
                    if let Some(id) = remote_queue_id {
                        self.descriptor.set_remote_queue_id(id);
                        true
                    } else {
                        false
                    }
                },
                || self.descriptor.validate(params),
            )?;
            probes::on_send(self.descriptor.queue_id(), Half::Stream, false);
            Ok(waker)
        }
    }

    #[inline]
    pub fn send_control(
        &self,
        entry: intrusive::Entry<C>,
        remote_queue_id: Option<VarInt>,
        params: &<Key as super::descriptor::Key>::Request,
    ) -> Result<AutoWake, Error<intrusive::Entry<C>>>
    where
        Key: super::descriptor::Key,
    {
        unsafe {
            let waker = self.descriptor.control_queue().push(
                entry,
                || {
                    if let Some(id) = remote_queue_id {
                        self.descriptor.set_remote_queue_id(id);
                        true
                    } else {
                        false
                    }
                },
                || self.descriptor.validate(params),
            )?;
            probes::on_send(self.descriptor.queue_id(), Half::Control, false);
            Ok(waker)
        }
    }

    #[inline]
    pub fn validate_stream(
        &self,
        params: &<Key as super::descriptor::Key>::Request,
    ) -> Result<(), super::ValidateError>
    where
        Key: super::descriptor::Key,
    {
        unsafe {
            self.descriptor
                .stream_queue()
                .with_key(|| self.descriptor.validate(params))
                .map_err(|()| super::ValidateError::Unallocated)?
                .map_err(super::ValidateError::Validation)
        }
    }

    #[inline]
    pub fn validate_control(
        &self,
        params: &<Key as super::descriptor::Key>::Request,
    ) -> Result<(), super::ValidateError>
    where
        Key: super::descriptor::Key,
    {
        unsafe {
            self.descriptor
                .control_queue()
                .with_key(|| self.descriptor.validate(params))
                .map_err(|()| super::ValidateError::Unallocated)?
                .map_err(super::ValidateError::Validation)
        }
    }
}

impl<S: 'static, C: 'static, Key: 'static> Drop for Sender<S, C, Key> {
    #[inline]
    fn drop(&mut self) {
        unsafe {
            self.descriptor.drop_sender();
        }
    }
}

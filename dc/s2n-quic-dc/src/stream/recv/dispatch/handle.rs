// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{descriptor::Descriptor, queue::Error};
use crate::{stream::Actor, sync::ring_deque};
use core::{
    fmt,
    task::{Context, Poll},
};
use s2n_quic_core::varint::VarInt;
use std::collections::VecDeque;

macro_rules! impl_recv {
    ($name:ident, $field:ident, $drop:ident) => {
        pub struct $name<T: 'static> {
            descriptor: Descriptor<T>,
        }

        impl<T: 'static> $name<T> {
            #[inline]
            pub(super) fn new(descriptor: Descriptor<T>) -> Self {
                Self { descriptor }
            }

            /// Returns the associated `queue_id` for the channel
            ///
            /// This can be sent to a peer, which can be used to route packets back to the channel.
            #[inline]
            pub fn queue_id(&self) -> VarInt {
                unsafe { self.descriptor.queue_id() }
            }

            #[inline]
            pub fn push(&self, item: T) -> Option<T> {
                unsafe { self.descriptor.$field().force_push(item) }
            }

            #[inline]
            pub fn try_recv(&self) -> Result<Option<T>, ring_deque::Closed> {
                unsafe { self.descriptor.$field().pop() }
            }

            #[inline]
            pub async fn recv(&self, actor: Actor) -> Result<T, ring_deque::Closed> {
                core::future::poll_fn(|cx| self.poll_recv(cx, actor)).await
            }

            #[inline]
            pub fn poll_recv(
                &self,
                cx: &mut Context,
                actor: Actor,
            ) -> Poll<Result<T, ring_deque::Closed>> {
                unsafe { self.descriptor.$field().poll_pop(cx, actor) }
            }

            #[inline]
            pub fn poll_swap(
                &self,
                cx: &mut Context,
                actor: Actor,
                out: &mut VecDeque<T>,
            ) -> Poll<Result<(), ring_deque::Closed>> {
                unsafe { self.descriptor.$field().poll_swap(cx, actor, out) }
            }
        }

        impl<T: 'static> fmt::Debug for $name<T> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.debug_struct(stringify!($name))
                    .field("queue_id", &self.queue_id())
                    .finish()
            }
        }

        impl<T: 'static> Drop for $name<T> {
            #[inline]
            fn drop(&mut self) {
                unsafe {
                    self.descriptor.$drop();
                }
            }
        }
    };
}

impl_recv!(Control, control_queue, drop_control_receiver);
impl_recv!(Stream, stream_queue, drop_stream_receiver);

pub struct Sender<T: 'static> {
    descriptor: Descriptor<T>,
}

impl<T: 'static> Clone for Sender<T> {
    #[inline]
    fn clone(&self) -> Self {
        unsafe {
            Self {
                descriptor: self.descriptor.clone_for_sender(),
            }
        }
    }
}

impl<T: 'static> Sender<T> {
    #[inline]
    pub(super) fn new(descriptor: Descriptor<T>) -> Self {
        Self { descriptor }
    }

    #[inline]
    pub fn send_stream(&self, item: T) -> Result<Option<T>, Error> {
        unsafe { self.descriptor.stream_queue().push(item) }
    }

    #[inline]
    pub fn send_control(&self, item: T) -> Result<Option<T>, Error> {
        unsafe { self.descriptor.control_queue().push(item) }
    }
}

impl<T: 'static> Drop for Sender<T> {
    #[inline]
    fn drop(&mut self) {
        unsafe {
            self.descriptor.drop_sender();
        }
    }
}

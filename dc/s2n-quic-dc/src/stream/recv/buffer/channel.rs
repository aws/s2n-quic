// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::Dispatch;
use crate::{
    event,
    socket::recv::descriptor::Filled,
    stream::{
        recv::{
            self,
            dispatch::{Control, Stream},
        },
        socket::Socket,
        Actor, TransportFeatures,
    },
};
use core::task::{Context, Poll};
use s2n_quic_core::ensure;
use std::{collections::VecDeque, io};

#[derive(Debug)]
pub struct Channel<Recv = Stream> {
    pending: VecDeque<Filled>,
    receiver: Recv,
}

impl<Recv> Channel<Recv> {
    #[inline]
    pub fn new(receiver: Recv) -> Self {
        Self {
            pending: VecDeque::new(),
            receiver,
        }
    }
}

macro_rules! impl_buffer {
    ($recv:ident) => {
        impl super::Buffer for Channel<$recv> {
            #[inline]
            fn is_empty(&self) -> bool {
                self.pending.is_empty()
            }

            #[inline]
            fn poll_fill<S, Pub>(
                &mut self,
                cx: &mut Context,
                actor: Actor,
                socket: &S,
                publisher: &mut Pub,
            ) -> Poll<io::Result<usize>>
            where
                S: ?Sized + Socket,
                Pub: event::ConnectionPublisher,
            {
                // check if we've already filled the queue
                ensure!(self.pending.is_empty(), Ok(1).into());

                let capacity = u16::MAX as usize;

                // the socket isn't actually used since we're relying on another task to fill the `receiver` channel
                let _ = socket;

                let result = self
                    .receiver
                    .poll_swap(cx, actor, &mut self.pending)
                    .map_err(|_err| io::Error::from(io::ErrorKind::BrokenPipe));

                match result {
                    Poll::Ready(Ok(())) => {
                        let committed_len = self
                            .pending
                            .iter()
                            .map(|segment| {
                                debug_assert!(
                                    !segment.is_empty(),
                                    "the channel should not contain empty packets"
                                );
                                segment.len() as usize
                            })
                            .sum::<usize>();
                        publisher.on_stream_read_socket_flushed(
                            event::builder::StreamReadSocketFlushed {
                                capacity,
                                committed_len,
                            },
                        );
                        Ok(committed_len).into()
                    }
                    Poll::Ready(Err(error)) => {
                        let errno = error.raw_os_error();
                        publisher.on_stream_read_socket_errored(
                            event::builder::StreamReadSocketErrored { capacity, errno },
                        );
                        Err(error).into()
                    }
                    Poll::Pending => {
                        publisher.on_stream_read_socket_blocked(
                            event::builder::StreamReadSocketBlocked { capacity },
                        );
                        Poll::Pending
                    }
                }
            }

            #[inline]
            fn process<R>(
                &mut self,
                features: TransportFeatures,
                router: &mut R,
            ) -> Result<(), recv::Error>
            where
                R: Dispatch,
            {
                debug_assert!(
                    !features.is_stream(),
                    "only datagram oriented transport is supported"
                );

                for mut segment in self.pending.drain(..) {
                    let remote_addr = segment.remote_address().get();
                    let ecn = segment.ecn();
                    router.on_datagram_segment(&remote_addr, ecn, segment.payload_mut())?;
                }

                Ok(())
            }
        }
    };
}

impl_buffer!(Stream);
impl_buffer!(Control);

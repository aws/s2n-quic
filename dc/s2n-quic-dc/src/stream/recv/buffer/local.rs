// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::Dispatch;
use crate::{
    event, msg,
    stream::{recv, server::handshake, socket::Socket, Actor, TransportFeatures},
};
use bytes::buf::UninitSlice;
use core::task::{Context, Poll};
use s2n_codec::{DecoderBufferMut, DecoderError};
use s2n_quic_core::{buffer::writer::Storage, ensure, ready};
use std::io;

#[derive(Debug)]
pub struct Local {
    recv_buffer: msg::recv::Message,
    saw_fin: bool,
    handshake: Option<handshake::Receiver>,
}

impl Local {
    #[inline]
    pub fn new(recv_buffer: msg::recv::Message, handshake: Option<handshake::Receiver>) -> Self {
        Self {
            recv_buffer,
            saw_fin: false,
            handshake,
        }
    }

    pub fn saw_fin(&self) -> bool {
        self.saw_fin
    }

    pub fn copy_into(&mut self, mut output: &mut UninitSlice) -> usize {
        let mut written = 0usize;
        while output.has_remaining_capacity() && !self.recv_buffer.peek().is_empty() {
            let chunk_len = self
                .recv_buffer
                .peek()
                .len()
                .clamp(0, output.remaining_capacity());
            output.put_slice(&self.recv_buffer.peek()[..chunk_len]);
            written += chunk_len;
            self.recv_buffer.consume(chunk_len);
        }
        written
    }
}

impl super::Buffer for Local {
    #[inline]
    fn is_empty(&self) -> bool {
        self.recv_buffer.is_empty()
    }

    #[inline]
    fn poll_fill<S, Pub>(
        &mut self,
        cx: &mut Context,
        _actor: Actor,
        socket: &S,
        publisher: &mut Pub,
    ) -> Poll<io::Result<usize>>
    where
        S: ?Sized + Socket,
        Pub: event::ConnectionPublisher,
    {
        loop {
            if let Some(chan) = self.handshake.as_mut() {
                match chan.poll_recv(cx) {
                    Poll::Ready(Some(recv_buffer)) => {
                        debug_assert!(!recv_buffer.is_empty());
                        // no point in doing anything with an empty buffer
                        ensure!(!recv_buffer.is_empty(), continue);
                        // we got a buffer from the handshake so return and process it
                        self.recv_buffer = recv_buffer;
                        return Ok(self.recv_buffer.payload_len()).into();
                    }
                    Poll::Ready(None) => {
                        // the channel was closed so drop it
                        self.handshake = None;
                    }
                    Poll::Pending => {
                        // keep going and read the socket
                    }
                }
            }

            if ready!(self.poll_fill_once(cx, socket, publisher))? == 0 {
                self.saw_fin = true;
            }

            return Ok(self.recv_buffer.payload_len()).into();
        }
    }

    #[inline]
    fn process<R>(&mut self, features: TransportFeatures, router: &mut R) -> Result<(), recv::Error>
    where
        R: Dispatch,
    {
        ensure!(!self.recv_buffer.is_empty(), Ok(()));

        if features.is_stream() {
            self.dispatch_buffer_stream(router)
        } else {
            self.dispatch_buffer_datagram(router)
        }
    }
}

impl Local {
    #[inline(always)]
    fn poll_fill_once<S, Pub>(
        &mut self,
        cx: &mut Context,
        socket: &S,
        publisher: &mut Pub,
    ) -> Poll<io::Result<usize>>
    where
        S: ?Sized + Socket,
        Pub: event::ConnectionPublisher,
    {
        let capacity = self.recv_buffer.remaining_capacity();

        let result = socket.poll_recv_buffer(cx, &mut self.recv_buffer);

        match &result {
            Poll::Ready(Ok(len)) => {
                publisher.on_stream_read_socket_flushed(event::builder::StreamReadSocketFlushed {
                    capacity,
                    committed_len: *len,
                });
            }
            Poll::Ready(Err(error)) => {
                let errno = error.raw_os_error();
                publisher.on_stream_read_socket_errored(event::builder::StreamReadSocketErrored {
                    capacity,
                    errno,
                });
            }
            Poll::Pending => {
                publisher.on_stream_read_socket_blocked(event::builder::StreamReadSocketBlocked {
                    capacity,
                });
            }
        };

        result
    }

    #[inline]
    fn dispatch_buffer_stream<R>(&mut self, router: &mut R) -> Result<(), recv::Error>
    where
        R: Dispatch,
    {
        let msg = &mut self.recv_buffer;
        let remote_addr = msg.remote_address();
        let ecn = msg.ecn();
        let tag_len = router.tag_len();

        let mut prev_packet_len = None;

        loop {
            // consume the previous packet
            if let Some(packet_len) = prev_packet_len.take() {
                msg.consume(packet_len);
            }

            let segment = msg.peek();
            ensure!(!segment.is_empty(), break);

            let initial_len = segment.len();
            let decoder = DecoderBufferMut::new(segment);

            let packet = match decoder.decode_parameterized(tag_len) {
                Ok((packet, remaining)) => {
                    prev_packet_len = Some(initial_len - remaining.len());
                    packet
                }
                Err(decoder_error) => {
                    if let DecoderError::UnexpectedEof(len) = decoder_error {
                        // if making the buffer contiguous resulted in the slice increasing, then
                        // try to parse a packet again
                        if msg.make_contiguous().len() > initial_len {
                            continue;
                        }

                        // otherwise, we'll need to receive more bytes from the stream to correctly
                        // parse a packet

                        // if we have pending data greater than the max datagram size then it's never going to parse
                        if msg.payload_len() > crate::stream::MAX_DATAGRAM_SIZE {
                            tracing::error!(
                                unconsumed = msg.payload_len(),
                                remaining_capacity = msg.remaining_capacity()
                            );
                            msg.clear();
                            return Err(recv::error::Kind::Decode.into());
                        }

                        if self.saw_fin {
                            tracing::error!("truncated stream");
                            msg.clear();
                            return Err(recv::error::Kind::Decode.into());
                        }

                        tracing::trace!(
                            socket_kind = %"stream",
                            unexpected_eof = len,
                            buffer_len = initial_len
                        );

                        break;
                    }

                    tracing::error!(
                        socket_kind = %"stream",
                        fatal_error = %decoder_error,
                        payload_len = msg.payload_len()
                    );

                    // any other decoder errors mean the stream has been corrupted so
                    // it's time to shut down the connection
                    msg.clear();
                    return Err(recv::error::Kind::Decode.into());
                }
            };

            if let Err(err) = router.on_packet(&remote_addr, ecn, packet) {
                // the stream errored and we can't recover so clear out the buffer
                msg.clear();
                return Err(err);
            }
        }

        if let Some(len) = prev_packet_len.take() {
            msg.consume(len);
        }

        Ok(())
    }

    #[inline]
    fn dispatch_buffer_datagram<R>(&mut self, router: &mut R) -> Result<(), recv::Error>
    where
        R: Dispatch,
    {
        let msg = &mut self.recv_buffer;
        let remote_addr = msg.remote_address();
        let ecn = msg.ecn();

        for segment in msg.segments() {
            router.on_datagram_segment(&remote_addr, ecn, segment)?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod test;

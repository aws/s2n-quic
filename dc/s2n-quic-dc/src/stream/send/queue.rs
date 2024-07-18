// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    msg::{addr, segment},
    stream::{
        send::{
            application::{self, transmission},
            buffer,
            state::Transmission,
        },
        socket::Socket,
    },
};
use bytes::buf::UninitSlice;
use core::task::{Context, Poll};
use s2n_quic_core::{assume, buffer::reader, ensure, inet::ExplicitCongestionNotification, ready};
use s2n_quic_platform::features::Gso;
use std::{collections::VecDeque, io};

/// An enqueued segment waiting to be transmitted on the socket
#[derive(Debug)]
pub struct Segment {
    ecn: ExplicitCongestionNotification,
    buffer: buffer::Segment,
    offset: u16,
}

impl Segment {
    #[inline]
    fn as_slice(&self) -> &[u8] {
        &self.buffer[self.offset as usize..]
    }
}

pub struct Message<'a> {
    batch: &'a mut Option<Vec<Transmission>>,
    queue: &'a mut Queue,
    max_segments: usize,
    segment_alloc: &'a buffer::Allocator,
}

impl<'a> application::state::Message for Message<'a> {
    #[inline]
    fn max_segments(&self) -> usize {
        self.max_segments
    }

    #[inline]
    fn push<P: FnOnce(&mut UninitSlice) -> transmission::Event<()>>(
        &mut self,
        buffer_len: usize,
        p: P,
    ) {
        let mut buffer = self.segment_alloc.alloc(buffer_len);

        let transmission = {
            let buffer = buffer.make_mut();

            debug_assert!(buffer.capacity() >= buffer_len);

            let slice = UninitSlice::uninit(buffer.spare_capacity_mut());

            let transmission = p(slice);

            unsafe {
                let packet_len = transmission.info.packet_len;
                assume!(buffer.capacity() >= packet_len as usize);
                buffer.set_len(packet_len as usize);
            }

            transmission
        };

        let transmission::Event {
            packet_number,
            info,
            has_more_app_data,
        } = transmission;

        let ecn = info.ecn;

        if let Some(batch) = self.batch.as_mut() {
            let info = info.map(|_| buffer.clone());

            batch.push(transmission::Event {
                packet_number,
                info,
                has_more_app_data,
            });
        }

        self.queue.segments.push_back(Segment {
            ecn,
            buffer,
            offset: 0,
        });
    }
}

#[derive(Debug, Default)]
pub struct Queue {
    /// Holds any segments that haven't been flushed to the socket
    segments: VecDeque<Segment>,
    /// How many bytes we've accepted from the caller of `poll_write`, but actually returned
    /// `Poll::Pending` for. This many bytes will be skipped the next time `poll_write` is called.
    ///
    /// This functionality ensures that we don't return to the application until we've flushed all
    /// outstanding packets to the underlying socket. Experience has shown applications rely on
    /// TCP's behavior, which never really requires `flush` or `shutdown` to progress the stream.
    accepted_len: usize,
}

impl Queue {
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }

    #[inline]
    pub fn accepted_len(&self) -> usize {
        self.accepted_len
    }

    #[inline]
    pub fn push_buffer<B, F, E>(
        &mut self,
        buf: &mut B,
        batch: &mut Option<Vec<Transmission>>,
        max_segments: usize,
        segment_alloc: &buffer::Allocator,
        push: F,
    ) -> Result<(), E>
    where
        B: reader::Storage,
        F: FnOnce(&mut Message, &mut reader::storage::Tracked<B>) -> Result<(), E>,
    {
        let mut message = Message {
            batch,
            queue: self,
            max_segments,
            segment_alloc,
        };

        let mut buf = buf.track_read();

        push(&mut message, &mut buf)?;

        // record how many bytes we encrypted/buffered so we only return Ready once everything has
        // been flushed
        self.accepted_len += buf.consumed_len();

        Ok(())
    }

    #[inline]
    pub fn poll_flush<S>(
        &mut self,
        cx: &mut Context,
        limit: usize,
        socket: &S,
        addr: &addr::Addr,
        segment_alloc: &buffer::Allocator,
        gso: &Gso,
    ) -> Poll<Result<usize, io::Error>>
    where
        S: ?Sized + Socket,
    {
        ready!(self.poll_flush_segments(cx, socket, addr, segment_alloc, gso))?;

        // Consume accepted credits
        let accepted = limit.min(self.accepted_len);
        self.accepted_len -= accepted;
        Poll::Ready(Ok(accepted))
    }

    #[inline]
    fn poll_flush_segments<S>(
        &mut self,
        cx: &mut Context,
        socket: &S,
        addr: &addr::Addr,
        segment_alloc: &buffer::Allocator,
        gso: &Gso,
    ) -> Poll<Result<(), io::Error>>
    where
        S: ?Sized + Socket,
    {
        ensure!(!self.segments.is_empty(), Poll::Ready(Ok(())));

        let default_addr = addr::Addr::new(Default::default());

        let addr = if socket.features().is_connected() {
            // no need to load the socket addr if the stream is already connected
            &default_addr
        } else {
            addr
        };

        if socket.features().is_stream() {
            self.poll_flush_segments_stream(cx, socket, addr, segment_alloc)
        } else {
            self.poll_flush_segments_datagram(cx, socket, addr, segment_alloc, gso)
        }
    }

    #[inline]
    fn poll_flush_segments_stream<S>(
        &mut self,
        cx: &mut Context,
        socket: &S,
        addr: &addr::Addr,
        segment_alloc: &buffer::Allocator,
    ) -> Poll<Result<(), io::Error>>
    where
        S: ?Sized + Socket,
    {
        while !self.segments.is_empty() {
            let segments = segment::Batch::new(self.segments.iter().map(|v| (v.ecn, v.as_slice())));

            let ecn = segments.ecn();
            let res = ready!(socket.poll_send(cx, addr, ecn, &segments));

            drop(segments);

            match res {
                Ok(written_len) => {
                    self.consume_segments(written_len, segment_alloc);

                    // keep trying to drain the buffer
                    continue;
                }
                Err(err) => {
                    // the socket encountered an error so clear everything out since we're shutting
                    // down
                    self.segments.clear();
                    self.accepted_len = 0;
                    return Err(err).into();
                }
            }
        }

        Ok(()).into()
    }

    #[inline]
    fn consume_segments(&mut self, consumed: usize, segment_alloc: &buffer::Allocator) {
        ensure!(consumed > 0);

        let mut remaining = consumed;

        while let Some(mut segment) = self.segments.pop_front() {
            if let Some(r) = remaining.checked_sub(segment.as_slice().len()) {
                remaining = r;

                // try to reuse the buffer for future allocations
                segment_alloc.free(segment.buffer);

                // if we don't have any remaining bytes to pop then we're done
                ensure!(remaining > 0, break);

                continue;
            }

            segment.offset += core::mem::take(&mut remaining) as u16;

            debug_assert!(!segment.as_slice().is_empty());

            self.segments.push_front(segment);
            break;
        }

        debug_assert_eq!(
            remaining, 0,
            "consumed ({consumed}) with too many bytes remaining ({remaining})"
        );
    }

    #[inline]
    fn poll_flush_segments_datagram<S>(
        &mut self,
        cx: &mut Context,
        socket: &S,
        addr: &addr::Addr,
        segment_alloc: &buffer::Allocator,
        gso: &Gso,
    ) -> Poll<Result<(), io::Error>>
    where
        S: ?Sized + Socket,
    {
        let mut max_segments = gso.max_segments();

        while !self.segments.is_empty() {
            // construct all of the segments we're going to send in this batch
            let segments = segment::Batch::new(
                self.segments
                    .iter()
                    .map(|v| (v.ecn, v.as_slice()))
                    .take(max_segments),
            );

            let ecn = segments.ecn();
            let res = match ready!(socket.poll_send(cx, addr, ecn, &segments)) {
                Ok(_) => Ok(()),
                Err(error) => {
                    if gso.handle_socket_error(&error).is_some() {
                        // update the max_segments value if it was changed due to the error
                        max_segments = 1;
                    }
                    Err(error)
                }
            };

            // consume the segments that we transmitted
            let segment_count = segments.len();
            drop(segments);
            for segment in self.segments.drain(..segment_count) {
                // try to reuse the buffer for future allocations
                segment_alloc.free(segment.buffer);
            }

            res?;
        }

        Ok(()).into()
    }
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    allocator::Allocator,
    clock,
    event::{self, ConnectionPublisher},
    msg,
    packet::{stream, Packet},
    stream::{
        recv,
        server::handshake,
        shared::{self, ArcShared, Half},
        socket::{self, Socket},
        TransportFeatures,
    },
    task::waker::worker::Waker as WorkerWaker,
};
use core::{
    mem::ManuallyDrop,
    ops,
    task::{Context, Poll},
};
use s2n_codec::{DecoderBufferMut, DecoderError};
use s2n_quic_core::{buffer, dc, ensure, ready, stream::state, time::Clock};
use std::{
    io,
    sync::{
        atomic::{AtomicU64, AtomicU8, Ordering},
        Mutex, MutexGuard,
    },
};

/// Who will send ACKs?
#[derive(Clone, Copy, Debug, Default)]
pub enum AckMode {
    /// The application task is sending ACKs
    #[default]
    Application,
    /// The worker task is sending ACKs
    Worker,
}

pub enum ApplicationState {
    Open,
    Closed { is_panicking: bool },
}

impl ApplicationState {
    const IS_CLOSED_MASK: u8 = 1;
    const IS_PANICKING_MASK: u8 = 1 << 1;

    #[inline]
    fn load(shared: &AtomicU8) -> Self {
        let value = shared.load(Ordering::Acquire);
        if value == 0 {
            return Self::Open;
        }

        let is_panicking = value & Self::IS_PANICKING_MASK != 0;

        Self::Closed { is_panicking }
    }

    #[inline]
    fn close(shared: &AtomicU8, is_panicking: bool) {
        let mut value = Self::IS_CLOSED_MASK;

        if is_panicking {
            value |= Self::IS_PANICKING_MASK;
        }

        shared.store(value, Ordering::Release);
    }
}

#[derive(Debug)]
pub struct State {
    inner: Mutex<Inner>,
    application_epoch: AtomicU64,
    application_state: AtomicU8,
    pub worker_waker: WorkerWaker,
}

impl State {
    #[inline]
    pub fn new(
        stream_id: stream::Id,
        params: &dc::ApplicationParams,
        handshake: Option<handshake::Receiver>,
        features: TransportFeatures,
        recv_buffer: Option<&mut msg::recv::Message>,
    ) -> Self {
        let recv_buffer = match recv_buffer {
            Some(prev) => prev.take(),
            None => msg::recv::Message::new(9000u16),
        };
        let receiver = recv::state::State::new(stream_id, params, features);
        let reassembler = Default::default();
        let inner = Inner {
            receiver,
            reassembler,
            handshake,
            recv_buffer,
        };
        let inner = Mutex::new(inner);
        Self {
            inner,
            application_epoch: AtomicU64::new(0),
            application_state: AtomicU8::new(0),
            worker_waker: Default::default(),
        }
    }

    #[inline]
    pub fn application_state(&self) -> ApplicationState {
        ApplicationState::load(&self.application_state)
    }

    #[inline]
    pub fn application_epoch(&self) -> u64 {
        self.application_epoch.load(Ordering::Acquire)
    }

    #[inline]
    pub fn application_guard<'a, Sub>(
        &'a self,
        ack_mode: AckMode,
        send_buffer: &'a mut msg::send::Message,
        shared: &'a ArcShared<Sub>,
        sockets: &'a dyn socket::Application,
    ) -> io::Result<AppGuard<'a, Sub>>
    where
        Sub: event::Subscriber,
    {
        // increment the epoch at which we acquired the guard
        self.application_epoch.fetch_add(1, Ordering::AcqRel);

        let inner = self.inner.lock().map_err(|_| {
            io::Error::new(io::ErrorKind::Other, "shared recv state has been poisoned")
        })?;

        let initial_state = inner.receiver.state().clone();

        let inner = ManuallyDrop::new(inner);

        Ok(AppGuard {
            inner,
            ack_mode,
            send_buffer,
            shared,
            sockets,
            initial_state,
        })
    }

    #[inline]
    pub fn shutdown(&self, is_panicking: bool) {
        ApplicationState::close(&self.application_state, is_panicking);
        self.worker_waker.wake();
    }

    #[inline]
    pub fn worker_try_lock(&self) -> io::Result<Option<MutexGuard<Inner>>> {
        match self.inner.try_lock() {
            Ok(lock) => Ok(Some(lock)),
            Err(std::sync::TryLockError::WouldBlock) => Ok(None),
            Err(_) => Err(io::Error::new(
                io::ErrorKind::Other,
                "shared recv state has been poisoned",
            )),
        }
    }
}

pub struct AppGuard<'a, Sub>
where
    Sub: event::Subscriber,
{
    inner: ManuallyDrop<MutexGuard<'a, Inner>>,
    ack_mode: AckMode,
    send_buffer: &'a mut msg::send::Message,
    shared: &'a ArcShared<Sub>,
    sockets: &'a dyn socket::Application,
    initial_state: state::Receiver,
}

impl<Sub> AppGuard<'_, Sub>
where
    Sub: event::Subscriber,
{
    /// Returns `true` if the read worker should be woken
    #[inline]
    fn send_ack(&mut self) -> bool {
        // we only send ACKs for unreliable protocols
        ensure!(
            !self.sockets.read_application().features().is_reliable(),
            false
        );

        match self.ack_mode {
            AckMode::Application => {
                self.inner
                    .fill_transmit_queue(self.shared, self.send_buffer);

                ensure!(!self.send_buffer.is_empty(), false);

                let did_send = self
                    .sockets
                    .read_application()
                    .try_send_buffer(self.send_buffer)
                    .is_ok();

                // clear out the sender buffer if we didn't already
                let _ = self.send_buffer.drain();

                // only wake the worker if we weren't able to transmit the ACK
                !did_send
            }
            AckMode::Worker => {
                // only wake the worker if the receiver says we should
                self.inner.receiver.should_transmit()
            }
        }
    }
}

impl<Sub> ops::Deref for AppGuard<'_, Sub>
where
    Sub: event::Subscriber,
{
    type Target = Inner;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<Sub> ops::DerefMut for AppGuard<'_, Sub>
where
    Sub: event::Subscriber,
{
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

impl<Sub> Drop for AppGuard<'_, Sub>
where
    Sub: event::Subscriber,
{
    #[inline]
    fn drop(&mut self) {
        let wake_worker_for_ack = self.send_ack();

        let current_state = self.inner.receiver.state().clone();

        unsafe {
            // SAFETY: inner is no longer used
            ManuallyDrop::drop(&mut self.inner);
        }

        if wake_worker_for_ack && !current_state.is_terminal() {
            // TODO wake the worker
        }

        // no need to look at anything if the state didn't change
        ensure!(self.initial_state != current_state);

        // shut down the worker if we're in a terminal state
        if current_state.is_terminal() {
            self.shared.receiver.shutdown(false);
        }
    }
}

#[derive(Debug)]
pub struct Inner {
    pub receiver: recv::state::State,
    pub reassembler: buffer::Reassembler,
    pub recv_buffer: msg::recv::Message,
    pub handshake: Option<handshake::Receiver>,
}

impl Inner {
    #[inline]
    pub fn fill_transmit_queue<Sub>(
        &mut self,
        shared: &ArcShared<Sub>,
        send_buffer: &mut msg::send::Message,
    ) where
        Sub: event::Subscriber,
    {
        let source_control_port = shared.source_control_port();

        self.receiver.on_transmit(
            shared
                .crypto
                .control_sealer()
                .expect("control sealer should be available with recv transmissions"),
            shared.credentials(),
            source_control_port,
            send_buffer,
            &shared.clock,
        );

        ensure!(!send_buffer.is_empty());

        // Update the remote address with the latest value
        send_buffer.set_remote_address(shared.read_remote_addr());
    }

    #[inline]
    pub fn poll_fill_recv_buffer<S, C, Sub>(
        &mut self,
        cx: &mut Context,
        socket: &S,
        clock: &C,
        subscriber: &shared::Subscriber<Sub>,
    ) -> Poll<io::Result<()>>
    where
        S: ?Sized + Socket,
        C: ?Sized + Clock,
        Sub: event::Subscriber,
    {
        // cache the timestamps to avoid fetching too many
        let clock = &s2n_quic_core::time::clock::Cached::new(clock);

        loop {
            if let Some(chan) = self.handshake.as_mut() {
                match chan.poll_recv(cx) {
                    Poll::Ready(Some(recv_buffer)) => {
                        debug_assert!(!recv_buffer.is_empty());
                        // no point in doing anything with an empty buffer
                        ensure!(!recv_buffer.is_empty(), continue);
                        // we got a buffer from the handshake so return and process it
                        self.recv_buffer = recv_buffer;
                        return Ok(()).into();
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

            ready!(self.poll_fill_recv_buffer_once(cx, socket, clock, subscriber))?;

            return Ok(()).into();
        }
    }

    #[inline(always)]
    fn poll_fill_recv_buffer_once<S, C, Sub>(
        &mut self,
        cx: &mut Context,
        socket: &S,
        clock: &C,
        subscriber: &shared::Subscriber<Sub>,
    ) -> Poll<io::Result<usize>>
    where
        S: ?Sized + Socket,
        C: ?Sized + Clock,
        Sub: event::Subscriber,
    {
        let capacity = self.recv_buffer.remaining_capacity();

        let result = socket.poll_recv_buffer(cx, &mut self.recv_buffer);

        let now = clock.get_time();

        match &result {
            Poll::Ready(Ok(len)) => {
                subscriber.publisher(now).on_stream_read_socket_flushed(
                    event::builder::StreamReadSocketFlushed {
                        capacity,
                        committed_len: *len,
                    },
                );
            }
            Poll::Ready(Err(error)) => {
                let errno = error.raw_os_error();
                subscriber.publisher(now).on_stream_read_socket_errored(
                    event::builder::StreamReadSocketErrored { capacity, errno },
                );
            }
            Poll::Pending => {
                subscriber.publisher(now).on_stream_read_socket_blocked(
                    event::builder::StreamReadSocketBlocked { capacity },
                );
            }
        };

        result
    }

    #[inline]
    pub fn process_recv_buffer<Sub>(
        &mut self,
        out_buf: &mut impl buffer::writer::Storage,
        shared: &ArcShared<Sub>,
        features: TransportFeatures,
    ) -> bool
    where
        Sub: event::Subscriber,
    {
        let clock = clock::Cached::new(&shared.clock);
        let clock = &clock;

        // try copying data out of the reassembler into the application buffer
        self.receiver
            .on_read_buffer(&mut self.reassembler, out_buf, clock);

        // check if we have any packets to process
        if !self.recv_buffer.is_empty() {
            if features.is_stream() {
                self.dispatch_buffer_stream(out_buf, shared, clock, features)
            } else {
                self.dispatch_buffer_datagram(out_buf, shared, clock, features)
            }

            // if we processed packets then we may have data to copy out
            self.receiver
                .on_read_buffer(&mut self.reassembler, out_buf, clock);
        }

        // we only check for timeouts on unreliable transports
        if !features.is_reliable() {
            self.receiver
                .on_timeout(clock, || shared.last_peer_activity());
        }

        // indicate to the caller if we need to transmit an ACK
        self.receiver.should_transmit()
    }

    #[inline]
    fn dispatch_buffer_stream<Sub, C>(
        &mut self,
        out_buf: &mut impl buffer::writer::Storage,
        shared: &ArcShared<Sub>,
        clock: &C,
        features: TransportFeatures,
    ) where
        Sub: event::Subscriber,
        C: Clock + ?Sized,
    {
        let msg = &mut self.recv_buffer;
        let remote_addr = msg.remote_address();
        let ecn = msg.ecn();
        let tag_len = shared.crypto.tag_len();

        let mut any_valid_packets = false;
        let mut did_complete_handshake = false;

        let mut prev_packet_len = None;

        let mut out_buf = buffer::duplex::Interposer::new(out_buf, &mut self.reassembler);

        // this opener should never actually be used anywhere. any packets that try to use control
        // authentication will result in stream closure.
        let control_opener = &crate::crypto::open::control::stream::Reliable::default();

        loop {
            // consume the previous packet
            if let Some(packet_len) = prev_packet_len.take() {
                msg.consume(packet_len);
            }

            let segment = msg.peek();
            ensure!(!segment.is_empty(), break);

            let initial_len = segment.len();
            let decoder = DecoderBufferMut::new(segment);

            let mut packet = match decoder.decode_parameterized(tag_len) {
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
                            self.receiver.on_error(recv::error::Kind::Decode);
                            return;
                        }

                        tracing::trace!(
                            protocol_features = ?features,
                            unexpected_eof = len,
                            buffer_len = initial_len
                        );

                        break;
                    }

                    tracing::error!(
                        protocol_features = ?features,
                        fatal_error = %decoder_error,
                        payload_len = msg.payload_len()
                    );

                    // any other decoder errors mean the stream has been corrupted so
                    // it's time to shut down the connection
                    msg.clear();
                    self.receiver.on_error(recv::error::Kind::Decode);
                    return;
                }
            };

            tracing::trace!(?packet);

            match &mut packet {
                Packet::Stream(packet) => {
                    debug_assert_eq!(Some(packet.total_len()), prev_packet_len);

                    // make sure the packet looks OK before deriving openers from it
                    if self
                        .receiver
                        .precheck_stream_packet(shared.credentials(), packet)
                        .is_err()
                    {
                        // check if the receiver returned an error
                        if self.receiver.check_error().is_err() {
                            msg.clear();
                            return;
                        } else {
                            // move on to the next packet
                            continue;
                        }
                    }

                    let _ = shared.crypto.open_with(
                        |opener| {
                            self.receiver.on_stream_packet(
                                opener,
                                control_opener,
                                shared.credentials(),
                                packet,
                                ecn,
                                clock,
                                &mut out_buf,
                            )?;

                            any_valid_packets = true;
                            did_complete_handshake |=
                                packet.next_expected_control_packet().as_u64() > 0;

                            <Result<_, recv::Error>>::Ok(())
                        },
                        clock,
                        &shared.subscriber,
                    );

                    if self.receiver.check_error().is_err() {
                        msg.clear();
                        return;
                    }
                }
                other => {
                    let kind = other.kind();
                    shared
                        .crypto
                        .map()
                        .handle_unexpected_packet(other, &shared.read_remote_addr().into());

                    // if we get a packet we don't expect then it's fatal for streams
                    msg.clear();
                    self.receiver
                        .on_error(recv::error::Kind::UnexpectedPacket { packet: kind });
                    return;
                }
            }
        }

        if let Some(len) = prev_packet_len.take() {
            msg.consume(len);
        }

        if any_valid_packets {
            shared.on_valid_packet(&remote_addr, Half::Read, did_complete_handshake);
        }
    }

    #[inline]
    fn dispatch_buffer_datagram<Sub, C>(
        &mut self,
        out_buf: &mut impl buffer::writer::Storage,
        shared: &ArcShared<Sub>,
        clock: &C,
        features: TransportFeatures,
    ) where
        Sub: event::Subscriber,
        C: Clock + ?Sized,
    {
        let msg = &mut self.recv_buffer;
        let remote_addr = msg.remote_address();
        let ecn = msg.ecn();
        let tag_len = shared.crypto.tag_len();

        let mut any_valid_packets = false;
        let mut did_complete_handshake = false;

        let mut out_buf = buffer::duplex::Interposer::new(out_buf, &mut self.reassembler);
        let control_opener = shared
            .crypto
            .control_opener()
            .expect("control opener should be available on unreliable transports");

        for segment in msg.segments() {
            let segment_len = segment.len();
            let mut decoder = DecoderBufferMut::new(segment);

            'segment: while !decoder.is_empty() {
                let packet = match decoder.decode_parameterized(tag_len) {
                    Ok((packet, remaining)) => {
                        decoder = remaining;
                        packet
                    }
                    Err(decoder_error) => {
                        // the packet was likely corrupted so log it and move on to the
                        // next segment
                        tracing::warn!(
                            protocol_features = ?features,
                            %decoder_error,
                            segment_len
                        );

                        break 'segment;
                    }
                };

                match packet {
                    Packet::Stream(mut packet) => {
                        // make sure the packet looks OK before deriving openers from it
                        ensure!(
                            self.receiver
                                .precheck_stream_packet(shared.credentials(), &packet)
                                .is_ok(),
                            continue
                        );

                        let _ = shared.crypto.open_with(
                            |opener| {
                                self.receiver.on_stream_packet(
                                    opener,
                                    control_opener,
                                    shared.credentials(),
                                    &mut packet,
                                    ecn,
                                    clock,
                                    &mut out_buf,
                                )?;

                                any_valid_packets = true;
                                did_complete_handshake |=
                                    packet.next_expected_control_packet().as_u64() > 0;

                                <Result<_, recv::Error>>::Ok(())
                            },
                            clock,
                            &shared.subscriber,
                        );
                    }
                    other => {
                        shared
                            .crypto
                            .map()
                            .handle_unexpected_packet(&other, &shared.read_remote_addr().into());

                        // TODO if the packet was authentic then close the receiver with an error
                    }
                }
            }
        }

        if any_valid_packets {
            shared.on_valid_packet(&remote_addr, Half::Read, did_complete_handshake);
        }
    }
}

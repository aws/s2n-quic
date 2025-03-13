// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    allocator::Allocator,
    clock, event, msg,
    packet::{stream, Packet},
    stream::{
        recv::{self, buffer::Buffer as _},
        shared::{self, ArcShared},
        socket::{self, Socket},
        Actor, TransportFeatures,
    },
    task::waker::worker::Waker as WorkerWaker,
};
use core::{
    fmt,
    mem::ManuallyDrop,
    ops,
    task::{Context, Poll},
};
use s2n_quic_core::{
    buffer, dc, ensure, inet::SocketAddress, ready, stream::state, time::Clock, varint::VarInt,
};
use std::{
    io,
    sync::{
        atomic::{AtomicU64, AtomicU8, Ordering},
        Mutex, MutexGuard,
    },
};

pub type RecvBuffer = recv::buffer::Either<recv::buffer::Local, recv::buffer::Channel>;

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
    is_owned_socket: bool,
}

impl State {
    #[inline]
    pub fn new(
        stream_id: stream::Id,
        params: &dc::ApplicationParams,
        features: TransportFeatures,
        buffer: RecvBuffer,
    ) -> Self {
        let receiver = recv::state::State::new(stream_id, params, features);
        let reassembler = Default::default();
        let is_owned_socket = matches!(buffer, recv::buffer::Either::A(recv::buffer::Local { .. }));
        let inner = Inner {
            receiver,
            reassembler,
            buffer,
            is_handshaking: true,
        };
        let inner = Mutex::new(inner);
        Self {
            inner,
            application_epoch: AtomicU64::new(0),
            application_state: AtomicU8::new(0),
            worker_waker: Default::default(),
            is_owned_socket,
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
    pub fn poll_peek_worker<S, C, Sub>(
        &self,
        cx: &mut Context,
        socket: &S,
        clock: &C,
        subscriber: &shared::Subscriber<Sub>,
    ) -> Poll<()>
    where
        S: ?Sized + Socket,
        C: ?Sized + Clock,
        Sub: event::Subscriber,
    {
        if self.is_owned_socket {
            let _ = ready!(socket.poll_peek_len(cx));
            return Poll::Ready(());
        }
        let Ok(Some(mut inner)) = self.worker_try_lock() else {
            // have the worker arm its timer
            return Poll::Ready(());
        };
        let _ = ready!(inner.poll_fill_recv_buffer(cx, Actor::Worker, socket, clock, subscriber));
        Poll::Ready(())
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
        ensure!(!self.sockets.features().is_reliable(), false);

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

pub struct Inner {
    pub receiver: recv::state::State,
    pub reassembler: buffer::Reassembler,
    buffer: RecvBuffer,
    is_handshaking: bool,
}

impl fmt::Debug for Inner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Inner")
            .field("receiver", &self.receiver)
            .field("reassembler", &self.reassembler)
            .field("is_handshaking", &self.is_handshaking)
            .finish()
    }
}

impl Inner {
    #[inline]
    pub fn payload_is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    #[inline]
    pub fn fill_transmit_queue<Sub>(
        &mut self,
        shared: &ArcShared<Sub>,
        send_buffer: &mut msg::send::Message,
    ) where
        Sub: event::Subscriber,
    {
        let stream_id = shared.stream_id();
        let source_queue_id = shared.local_queue_id();

        self.receiver.on_transmit(
            shared
                .crypto
                .control_sealer()
                .expect("control sealer should be available with recv transmissions"),
            shared.credentials(),
            stream_id,
            source_queue_id,
            send_buffer,
            &shared.clock,
        );

        ensure!(!send_buffer.is_empty());

        // Update the remote address with the latest value
        send_buffer.set_remote_address(shared.remote_addr());
    }

    #[inline]
    pub fn poll_fill_recv_buffer<S, C, Sub>(
        &mut self,
        cx: &mut Context,
        actor: Actor,
        socket: &S,
        clock: &C,
        subscriber: &shared::Subscriber<Sub>,
    ) -> Poll<io::Result<usize>>
    where
        S: ?Sized + Socket,
        C: ?Sized + Clock,
        Sub: event::Subscriber,
    {
        self.buffer.poll_fill(
            cx,
            actor,
            socket,
            &mut subscriber.publisher(clock.get_time()),
        )
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
        if !self.buffer.is_empty() {
            let res = {
                let mut out_buf = buffer::duplex::Interposer::new(out_buf, &mut self.reassembler);

                if features.is_stream() {
                    // this opener should never actually be used anywhere. any packets that try to use control
                    // authentication will result in stream closure.
                    let control_opener = &crate::crypto::open::control::stream::Reliable::default();

                    let mut router = PacketDispatch::new_stream(
                        &mut self.receiver,
                        &mut self.is_handshaking,
                        &mut out_buf,
                        control_opener,
                        clock,
                        shared,
                    );

                    self.buffer.process(features, &mut router)
                } else {
                    let control_opener = shared
                        .crypto
                        .control_opener()
                        .expect("control opener should be available on unreliable transports");

                    let mut router = PacketDispatch::new_datagram(
                        &mut self.receiver,
                        &mut self.is_handshaking,
                        &mut out_buf,
                        control_opener,
                        clock,
                        shared,
                    );

                    self.buffer.process(features, &mut router)
                }
            };

            if let Err(err) = res {
                self.receiver.on_error(err);
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
}

struct PacketDispatch<'a, Buf, Crypt, Clk, Sub, const IS_STREAM: bool>
where
    Buf: buffer::Duplex<Error = core::convert::Infallible>,
    Crypt: crate::crypto::open::control::Stream,
    Clk: Clock + ?Sized,
    Sub: event::Subscriber,
{
    did_complete_handshake: bool,
    any_valid_packets: bool,
    is_handshaking: &'a mut bool,
    remote_addr: SocketAddress,
    remote_queue_id: Option<VarInt>,
    receiver: &'a mut recv::state::State,
    control_opener: &'a Crypt,
    out_buf: &'a mut Buf,
    shared: &'a ArcShared<Sub>,
    clock: &'a Clk,
}

impl<'a, Buf, Crypt, Clk, Sub> PacketDispatch<'a, Buf, Crypt, Clk, Sub, true>
where
    Buf: buffer::Duplex<Error = core::convert::Infallible>,
    Crypt: crate::crypto::open::control::Stream,
    Clk: Clock + ?Sized,
    Sub: event::Subscriber,
{
    /// Sets up a dispatcher for stream transports
    #[inline]
    fn new_stream(
        receiver: &'a mut recv::state::State,
        is_handshaking: &'a mut bool,
        out_buf: &'a mut Buf,
        control_opener: &'a Crypt,
        clock: &'a Clk,
        shared: &'a ArcShared<Sub>,
    ) -> Self {
        Self {
            did_complete_handshake: false,
            any_valid_packets: false,
            remote_addr: Default::default(),
            remote_queue_id: None,
            receiver,
            control_opener,
            out_buf,
            shared,
            clock,
            is_handshaking,
        }
    }
}

impl<'a, Buf, Crypt, Clk, Sub> PacketDispatch<'a, Buf, Crypt, Clk, Sub, false>
where
    Buf: buffer::Duplex<Error = core::convert::Infallible>,
    Crypt: crate::crypto::open::control::Stream,
    Clk: Clock + ?Sized,
    Sub: event::Subscriber,
{
    /// Sets up a dispatcher for datagram transports
    #[inline]
    fn new_datagram(
        receiver: &'a mut recv::state::State,
        is_handshaking: &'a mut bool,
        out_buf: &'a mut Buf,
        control_opener: &'a Crypt,
        clock: &'a Clk,
        shared: &'a ArcShared<Sub>,
    ) -> Self {
        Self {
            did_complete_handshake: false,
            any_valid_packets: false,
            remote_addr: Default::default(),
            remote_queue_id: None,
            receiver,
            control_opener,
            out_buf,
            shared,
            clock,
            is_handshaking,
        }
    }
}

impl<Buf, Crypt, Clk, Sub, const IS_STREAM: bool> recv::buffer::Dispatch
    for PacketDispatch<'_, Buf, Crypt, Clk, Sub, IS_STREAM>
where
    Buf: buffer::Duplex<Error = core::convert::Infallible>,
    Crypt: crate::crypto::open::control::Stream,
    Clk: Clock + ?Sized,
    Sub: event::Subscriber,
{
    #[inline]
    fn on_packet(
        &mut self,
        remote_addr: &SocketAddress,
        ecn: s2n_quic_core::inet::ExplicitCongestionNotification,
        packet: crate::packet::Packet,
    ) -> Result<(), recv::Error> {
        match packet {
            Packet::Stream(mut packet) => {
                // make sure the packet looks OK before deriving openers from it
                let precheck = self
                    .receiver
                    .precheck_stream_packet(self.shared.credentials(), &packet);

                if IS_STREAM {
                    // datagrams drop invalid packets - streams error out since the stream can't recover
                    precheck?;
                }

                let source_queue_id = packet.source_queue_id();

                let _ = self.shared.crypto.open_with(
                    |opener| {
                        self.receiver.on_stream_packet(
                            opener,
                            self.control_opener,
                            self.shared.credentials(),
                            &mut packet,
                            ecn,
                            self.clock,
                            self.out_buf,
                        )?;

                        self.any_valid_packets = true;
                        self.remote_addr = *remote_addr;

                        if source_queue_id.is_some() {
                            self.remote_queue_id = source_queue_id;
                        }

                        if *self.is_handshaking {
                            // if the peer has seen at least one packet from us, then transition to handshake complete
                            let peer_has_seen_control_packet =
                                packet.next_expected_control_packet().as_u64() > 0;
                            if peer_has_seen_control_packet {
                                *self.is_handshaking = false;
                                self.did_complete_handshake = true;
                            }
                        }

                        <Result<_, recv::Error>>::Ok(())
                    },
                    self.clock,
                    &self.shared.subscriber,
                );

                if IS_STREAM {
                    self.receiver.check_error()?;
                }

                Ok(())
            }
            other => {
                self.shared
                    .crypto
                    .map()
                    .handle_unexpected_packet(&other, &(*remote_addr).into());

                if !IS_STREAM {
                    // TODO if the packet was authentic then close the receiver with an error
                    // Datagram-based streams just drop unexpected packets
                    return Ok(());
                }

                // streams don't allow for other kinds of packets so close it and bail on processing
                Err(recv::error::Kind::UnexpectedPacket {
                    packet: other.kind(),
                }
                .into())
            }
        }
    }
}

impl<Buf, Crypt, Clk, Sub, const IS_STREAM: bool> Drop
    for PacketDispatch<'_, Buf, Crypt, Clk, Sub, IS_STREAM>
where
    Buf: buffer::Duplex<Error = core::convert::Infallible>,
    Crypt: crate::crypto::open::control::Stream,
    Clk: Clock + ?Sized,
    Sub: event::Subscriber,
{
    #[inline]
    fn drop(&mut self) {
        ensure!(self.any_valid_packets);
        self.shared.on_valid_packet(
            &self.remote_addr,
            self.remote_queue_id,
            self.did_complete_handshake,
        );
    }
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    credentials::Credentials,
    crypto::{self, UninitSlice},
    event,
    packet::{control, stream},
    stream::{
        error::{self, Error},
        recv::{packet, shared::TransmitQueue},
        shared::AcceptState,
        TransportFeatures, DEFAULT_IDLE_TIMEOUT,
    },
};
use core::{task::Poll, time::Duration};
use s2n_codec::{EncoderBuffer, EncoderValue};
use s2n_quic_core::{
    buffer::{self, reader::storage::Infallible as _},
    dc::ApplicationParams,
    endpoint::Location,
    ensure, frame,
    inet::ExplicitCongestionNotification,
    ready,
    stream::state::Receiver,
    time::{
        timer::{self, Provider as _},
        Clock, Timer, Timestamp,
    },
    varint::VarInt,
};

mod max_data;
mod recv_budget;
mod transmission;

#[derive(Clone, Copy, Debug)]
struct ErrorState {
    error: Error,
    source: Location,
}

#[derive(Debug)]
pub struct State {
    control_packet_number: u64,
    transmission: transmission::Transmission,
    state: Receiver,
    idle_timer: Timer,
    idle_timeout: Duration,
    // maintains a stable tick timer to avoid timer churn in the platform timer
    tick_timer: Timer,
    is_reliable: bool,
    max_data: max_data::MaxData,
    recv_budget: recv_budget::RecvBudget,
    error: Option<ErrorState>,
    fin_ack_packet_number: Option<VarInt>,
    features: TransportFeatures,
}

impl State {
    #[inline]
    pub fn new<C>(
        stream_id: stream::Id,
        params: &ApplicationParams,
        features: TransportFeatures,
        clock: &C,
    ) -> Self
    where
        C: Clock + ?Sized,
    {
        let (initial_max_data, max_data_window) = if features.is_flow_controlled() {
            (VarInt::MAX, VarInt::MAX)
        } else {
            let initial_max_data = params.local_recv_max_data;
            // use the send data window for the actual window after the stream has been accepted
            let data_window = params.local_send_max_data;
            (initial_max_data, data_window)
        };

        // set up the idle timer in case we never read anything
        let now = clock.get_time();
        let idle_timeout = params.max_idle_timeout().unwrap_or(DEFAULT_IDLE_TIMEOUT);
        let mut idle_timer = Timer::default();
        idle_timer.set(now + idle_timeout);

        // the tick timer just inherits the idle timer since that's the current stable target
        let tick_timer = idle_timer.clone();

        Self {
            is_reliable: stream_id.is_reliable,
            control_packet_number: Default::default(),
            transmission: Default::default(),
            state: Default::default(),
            idle_timer,
            idle_timeout,
            tick_timer,
            max_data: max_data::MaxData::new(initial_max_data, max_data_window),
            recv_budget: recv_budget::RecvBudget::new(max_data_window, params.max_datagram_size()),
            error: None,
            fin_ack_packet_number: None,
            features,
        }
    }

    #[inline]
    pub fn state(&self) -> &Receiver {
        &self.state
    }

    #[inline]
    pub fn timer(&self) -> Option<Timestamp> {
        self.next_expiration()
    }

    #[inline]
    pub fn is_open(&self) -> bool {
        !self.state.is_terminal()
    }

    #[inline]
    pub fn is_finished(&self) -> bool {
        ensure!(self.state.is_terminal(), false);
        ensure!(self.timer().is_none(), false);
        true
    }

    #[inline]
    pub fn stop_sending<Pub>(&mut self, error: s2n_quic_core::application::Error, publisher: &Pub)
    where
        Pub: event::ConnectionPublisher,
    {
        // if we've already received everything then no need to notify the peer to stop
        ensure!(matches!(self.state, Receiver::Recv | Receiver::SizeKnown));
        self.on_error(
            error::Kind::from_application_code(error.into()),
            Location::Local,
            publisher,
        );
    }

    #[inline]
    pub fn on_read_buffer<B, C, Clk>(
        &mut self,
        out_buf: &mut B,
        chunk: &mut C,
        accept_state: AcceptState,
        clock: &Clk,
    ) where
        B: buffer::Duplex<Error = core::convert::Infallible>,
        C: buffer::writer::Storage,
        Clk: Clock + ?Sized,
    {
        // try copying the out_buf into the application chunk, if possible
        if chunk.has_remaining_capacity() && !out_buf.buffer_is_empty() {
            out_buf.infallible_copy_into(chunk);
        }

        // if we know the final offset then update the state
        if out_buf.final_offset().is_some() {
            let _ = self.state.on_receive_fin();
        }

        // if we've received everything then update the state
        if out_buf.has_buffered_fin() && self.state.on_receive_all_data().is_ok() {
            // send an immediate ACK to confirm we received all data
            self.transmission.on_receive_all_data();
        }

        // if we've completely drained the out buffer try transitioning to the final state
        if out_buf.is_consumed() {
            let _ = self.state.on_app_read_all_data();
            // no need to transmit here - the sender has likely already received our
            // receive_all_data
        }

        // Only increase the max data if the application has accepted the stream.
        if matches!(accept_state, AcceptState::Accepted) {
            // Track application consumption for drain rate estimation
            let current = out_buf.current_offset();
            self.recv_budget
                .on_consume(*current as u64, clock.get_time());

            let dynamic_window = Some(self.recv_budget.window());
            self.max_data
                .on_read(current, out_buf.final_offset(), dynamic_window);
        }
    }

    #[inline]
    pub fn precheck_stream_packet<Pub>(
        &mut self,
        credentials: &Credentials,
        packet: &stream::decoder::Packet,
        publisher: &Pub,
    ) -> Result<(), Error>
    where
        Pub: event::ConnectionPublisher,
    {
        match self.precheck_stream_packet_impl(credentials, packet) {
            Ok(()) => Ok(()),
            Err(err) => {
                if err.is_fatal(&self.features) {
                    self.on_error(err, Location::Local, publisher);
                } else {
                    tracing::debug!(non_fatal_error = %err, ?packet);
                }
                Err(err)
            }
        }
    }

    #[inline]
    fn precheck_stream_packet_impl(
        &mut self,
        credentials: &Credentials,
        packet: &stream::decoder::Packet,
    ) -> Result<(), Error> {
        // make sure we're getting packets for the correct stream
        ensure!(
            packet.credentials() == credentials,
            Err(error::Kind::CredentialMismatch {
                expected: *credentials,
                actual: *packet.credentials(),
            }
            .err())
        );

        if self.features.is_stream() {
            // if the transport is streaming then we expect packet numbers in order
            let expected_pn = self
                .transmission
                .stream_ack
                .max_received_packet()
                .map_or(0, |v| v.as_u64() + 1);
            let actual_pn = packet.packet_number().as_u64();
            ensure!(
                expected_pn == actual_pn,
                Err(error::Kind::OutOfOrder {
                    expected: expected_pn,
                    actual: actual_pn,
                }
                .err())
            );
        }

        if self.features.is_reliable() {
            // if the transport is reliable then we don't expect retransmissions
            ensure!(
                !packet.is_retransmission(),
                Err(error::Kind::UnexpectedRetransmission.err())
            );
        }

        Ok(())
    }

    #[inline]
    pub fn on_stream_packet<D, C, B, Clk, Pub>(
        &mut self,
        opener: &D,
        control: &C,
        credentials: &Credentials,
        packet: &mut stream::decoder::Packet,
        ecn: ExplicitCongestionNotification,
        accept_state: AcceptState,
        clock: &Clk,
        out_buf: &mut B,
        publisher: &Pub,
    ) -> Result<(), Error>
    where
        D: crypto::open::Application,
        C: crypto::open::control::Stream,
        Clk: Clock + ?Sized,
        B: buffer::Duplex<Error = core::convert::Infallible>,
        Pub: event::ConnectionPublisher,
    {
        publisher.on_stream_packet_received(event::builder::StreamPacketReceived {
            packet_len: packet.total_len(),
            packet_number: packet.packet_number().as_u64(),
            stream_offset: packet.stream_offset().as_u64(),
            payload_len: packet.payload().len(),
            is_fin: packet.is_fin(),
            is_retransmission: packet.is_retransmission(),
        });

        match self.on_stream_packet_impl(
            opener,
            control,
            credentials,
            packet,
            ecn,
            accept_state,
            clock,
            out_buf,
            publisher,
        ) {
            Ok(()) => Ok(()),
            Err(err) => {
                if err.is_fatal(&self.features) {
                    self.on_error(err, Location::Local, publisher);
                } else {
                    tracing::debug!(non_fatal_error = %err, ?packet);
                }
                Err(err)
            }
        }
    }

    #[inline]
    fn on_stream_packet_impl<D, C, B, Clk, Pub>(
        &mut self,
        opener: &D,
        control: &C,
        credentials: &Credentials,
        packet: &mut stream::decoder::Packet,
        ecn: ExplicitCongestionNotification,
        accept_state: AcceptState,
        clock: &Clk,
        out_buf: &mut B,
        publisher: &Pub,
    ) -> Result<(), Error>
    where
        D: crypto::open::Application,
        C: crypto::open::control::Stream,
        Clk: Clock + ?Sized,
        B: buffer::Duplex<Error = core::convert::Infallible>,
        Pub: event::ConnectionPublisher,
    {
        use buffer::reader::Storage as _;

        self.precheck_stream_packet_impl(credentials, packet)?;

        let is_max_data_ok = self.ensure_max_data(packet);

        // wrap the parsed packet in a reader
        let mut packet = packet::Packet {
            packet: &mut *packet,
            payload_cursor: 0,
            is_decrypted_in_place: false,
            ecn,
            clock,
            opener,
            control,
            receiver: self,
            copied_len: 0,
            publisher,
        };

        if !is_max_data_ok {
            // ensure the packet is authentic before resetting the stream
            let _ = packet.read_chunk(usize::MAX)?;

            tracing::error!(
                message = "max data exceeded",
                allowed = packet.receiver.max_data.value().as_u64(),
                requested = packet
                    .packet
                    .stream_offset()
                    .as_u64()
                    .saturating_add(packet.packet.payload().len() as u64),
            );

            let error = error::Kind::MaxDataExceeded.err();
            self.on_error(error, Location::Local, publisher);
            return Err(error);
        }

        let initial = out_buf.buffered_len();

        // decrypt and write the packet to the provided buffer
        out_buf.read_from(&mut packet)?;

        let new = out_buf.buffered_len();

        // Don't emit for empty payloads, those don't have copy costs.
        if !packet.packet.payload().is_empty() {
            // If the reader didn't actually copy any bytes from the packet payload then it skipped over it,
            // meaning the application data contained was already received previously.
            if packet.copied_len == 0 {
                publisher.on_stream_packet_spuriously_retransmitted(
                    event::builder::StreamPacketSpuriouslyRetransmitted {
                        packet_len: packet.packet.total_len(),
                        packet_number: packet.packet.packet_number().as_u64(),
                        stream_offset: packet.packet.stream_offset().as_u64(),
                        payload_len: packet.packet.payload().len(),
                        is_fin: packet.packet.is_fin(),
                        is_retransmission: packet.packet.is_retransmission(),
                    },
                );
            }

            publisher.on_stream_decrypt_packet(event::builder::StreamDecryptPacket {
                decrypted_in_place: packet.is_decrypted_in_place,
                forced_copy: if packet.is_decrypted_in_place {
                    packet.packet.payload().len()
                } else {
                    // Any new bytes buffered into the `out_buf` means they are going to need to be
                    // copied out into the application buffer.
                    //
                    // `saturating_sub` would be needed if we moved bytes out of the reassembler into
                    // the application buffer as part of the read_from. Currently it doesn't seem like
                    // that's possible (looking at `read_from`) but defense in depth.
                    new.saturating_sub(initial)
                },
                required_application_buffer: packet.packet.payload().len(),
            });
        }

        let mut chunk = buffer::writer::storage::Empty;
        self.on_read_buffer(out_buf, &mut chunk, accept_state, clock);

        Ok(())
    }

    #[inline]
    pub(super) fn on_stream_packet_in_place<D, C, Clk, Pub>(
        &mut self,
        crypto: &D,
        control: &C,
        packet: &mut stream::decoder::Packet,
        ecn: ExplicitCongestionNotification,
        clock: &Clk,
        publisher: &Pub,
    ) -> Result<(), Error>
    where
        D: crypto::open::Application,
        C: crypto::open::control::Stream,
        Clk: Clock + ?Sized,
        Pub: event::ConnectionPublisher,
    {
        // ensure the packet is authentic before processing it
        let res = packet.decrypt_in_place(crypto, control);

        res?;

        self.on_cleartext_stream_packet(packet, ecn, clock, publisher)
    }

    #[inline]
    pub(super) fn on_stream_packet_copy<D, C, Clk, Pub>(
        &mut self,
        crypto: &D,
        control: &C,
        packet: &mut stream::decoder::Packet,
        ecn: ExplicitCongestionNotification,
        payload_out: &mut UninitSlice,
        clock: &Clk,
        publisher: &Pub,
    ) -> Result<(), Error>
    where
        D: crypto::open::Application,
        C: crypto::open::control::Stream,
        Clk: Clock + ?Sized,
        Pub: event::ConnectionPublisher,
    {
        // ensure the packet is authentic before processing it
        let res = packet.decrypt(crypto, control, payload_out);

        res?;

        self.on_cleartext_stream_packet(packet, ecn, clock, publisher)
    }

    #[inline]
    fn ensure_max_data(&self, packet: &stream::decoder::Packet) -> bool {
        // we only need to enforce flow control for non-controlled transport
        ensure!(!self.features.is_flow_controlled(), true);

        self.max_data
            .ensure_packet(packet.stream_offset(), packet.payload().len() as u64)
    }

    #[inline]
    fn on_cleartext_stream_packet<Clk, Pub>(
        &mut self,
        packet: &mut stream::decoder::Packet,
        ecn: ExplicitCongestionNotification,
        clock: &Clk,
        publisher: &Pub,
    ) -> Result<(), Error>
    where
        Clk: Clock + ?Sized,
        Pub: event::ConnectionPublisher,
    {
        tracing::trace!(
            stream_id = %packet.stream_id(),
            stream_offset = packet.stream_offset().as_u64(),
            payload_len = packet.payload().len(),
            final_offset = ?packet.final_offset().map(|v| v.as_u64()),
        );

        let space = match packet.tag().packet_space() {
            stream::PacketSpace::Stream => &mut self.transmission.stream_ack,
            stream::PacketSpace::Recovery => &mut self.transmission.recovery_ack,
        };
        ensure!(
            space.filter.on_packet(packet).is_ok(),
            Err(error::Kind::Duplicate.err())
        );

        let packet_number = packet.packet_number();

        space.on_packet_received(packet_number, clock.get_time());

        let packet_space = packet.tag().packet_space();

        // update the idle timer since we received a valid packet
        if matches!(self.state, Receiver::Recv | Receiver::SizeKnown)
            || packet.stream_offset() == VarInt::ZERO
        {
            self.update_idle_timer(clock);

            // Differentiate ACK scheduling based on packet space:
            // - Recovery packets signal loss and need immediate ACKs
            // - Stream packets are batched to reduce ACK overhead
            match packet_space {
                stream::PacketSpace::Recovery => self.transmission.on_recovery_packet_active(),
                stream::PacketSpace::Stream => self.transmission.on_new_packet_active(),
            }

            // Arm the max-ack-delay timer if we have unacknowledged packets but
            // haven't yet hit the threshold
            self.transmission.arm_max_ack_delay(clock);
        } else {
            // After receiving all data, rate-limit ACKs to avoid storms while still
            // confirming delivery for the sender's probes
            self.transmission.on_new_packet_draining(clock);
        }

        for frame in packet.control_frames_mut() {
            let Ok(frame) = frame else {
                return Err(error::Kind::Decode.err());
            };
            match frame {
                frame::Frame::ConnectionClose(close) => {
                    let error = if close.frame_type.is_some() {
                        error::Kind::TransportError {
                            code: close.error_code,
                        }
                    } else {
                        error::Kind::from_connection_close(&close)
                    };

                    let error = error.err();
                    self.on_error(error, Location::Remote, publisher);
                    return Err(error);
                }
                frame::Frame::DataBlocked(data_blocked) => {
                    publisher.on_stream_data_blocked_received(
                        event::builder::StreamDataBlockedReceived {
                            packet_number: packet_number.as_u64(),
                            stream_offset: data_blocked.data_limit.as_u64(),
                        },
                    );
                    // respond to the sender with our current MAX_DATA value if they're behind
                    self.max_data.on_data_blocked(data_blocked.data_limit);
                }
                _ => {
                    // ignore other frames for now
                }
            }
        }

        self.transmission.increment_ecn(ecn);

        if !self.is_reliable {
            // TODO should we perform loss detection on the receiver and reset the stream if we have a big
            // enough gap?
        }

        // Notify recv_budget that we received a data packet with payload
        // (used for active-transfer filtering in RTT estimation)
        if !packet.payload().is_empty() {
            self.recv_budget.on_data_received(clock.get_time());
        }

        // clean up any ACK state that we can
        self.on_next_expected_control_packet(packet.next_expected_control_packet(), clock);

        Ok(())
    }

    #[inline]
    pub fn should_transmit(&self) -> bool {
        let mut enabled = self.transmission.is_queued();
        if matches!(self.state, Receiver::Recv | Receiver::SizeKnown) {
            enabled |= self.max_data.is_queued();
        }
        enabled
    }

    #[inline]
    pub fn on_transport_close<Pub>(&mut self, publisher: &Pub)
    where
        Pub: event::ConnectionPublisher,
    {
        // only stream transports can be closed
        ensure!(self.features.is_stream());

        // only error out if we're still expecting more data
        ensure!(matches!(self.state, Receiver::Recv | Receiver::SizeKnown));

        self.on_error(error::Kind::TruncatedTransport, Location::Local, publisher);
    }

    #[inline]
    fn on_next_expected_control_packet<Clk: Clock + ?Sized>(
        &mut self,
        next_expected_control_packet: VarInt,
        clock: &Clk,
    ) {
        if let Some(largest_delivered_control_packet) =
            next_expected_control_packet.checked_sub(VarInt::from_u8(1))
        {
            tracing::debug!(
                next_expected_control_packet = next_expected_control_packet.as_u64(),
                largest_delivered = largest_delivered_control_packet.as_u64(),
                stream_intervals = self.transmission.stream_ack.interval_len(),
                recovery_intervals = self.transmission.recovery_ack.interval_len(),
                "Processing next_expected_control_packet from peer"
            );

            self.transmission
                .on_largest_delivered_packet(largest_delivered_control_packet);
            self.max_data
                .on_largest_delivered_packet(largest_delivered_control_packet);

            // Sample feedback-loop RTT from the control packet echo
            self.recv_budget
                .on_control_ack(largest_delivered_control_packet, clock.get_time());

            if let Some(fin_ack_packet_number) = self.fin_ack_packet_number {
                // if the sender received our ACK to the fin, then we can shut down immediately
                if largest_delivered_control_packet >= fin_ack_packet_number {
                    self.silent_shutdown();
                }
            }
        }
    }

    #[inline]
    fn update_idle_timer<Clk: Clock + ?Sized>(&mut self, clock: &Clk) {
        let target = clock.get_time() + self.idle_timeout;
        self.idle_timer.set(target);

        // if the tick timer isn't armed then rearmed it; otherwise keep it stable to avoid
        // churn
        if !self.tick_timer.is_armed() {
            self.tick_timer.set(target);
        }
    }

    #[inline]
    fn mtu(&self) -> u16 {
        // TODO should we pull this from somewhere

        // we want to make sure ACKs get through so use the minimum packet length for QUIC
        7000
    }

    #[inline]
    #[track_caller]
    pub fn on_error<E, Pub>(&mut self, error: E, source: Location, publisher: &Pub)
    where
        Error: From<E>,
        Pub: event::ConnectionPublisher,
    {
        let error = Error::from(error);
        debug_assert!(error.is_fatal(&self.features));

        // If we've already received or read all data, the error is irrelevant to the
        // application — don't poison the stream. This prevents late FlowReset packets
        // (e.g., from server dispatch cleanup after a successful RPC) from turning a
        // completed stream into a failed one.
        ensure!(!matches!(
            self.state,
            Receiver::DataRecvd | Receiver::DataRead
        ));

        let _ = self.state.on_reset();

        // make sure we haven't already set the error from something else
        ensure!(self.error.is_none());
        let is_idle_timeout = matches!(error.kind(), error::Kind::IdleTimeout);
        self.error = Some(ErrorState { error, source });
        if error.kind().is_abandoned() {
            publisher.on_stream_abandoned(event::builder::StreamAbandoned { error, source });
        } else {
            publisher.on_stream_receiver_errored(event::builder::StreamReceiverErrored {
                error,
                source,
            });
        }

        if matches!(source, Location::Local)
            && !is_idle_timeout
            && error.kind().as_connection_close().is_some()
        {
            // The application abandoned the stream (stop_sending) or experienced
            // an application-level error (panic, accept queue full, etc.). Clear
            // ACK state but keep the transmission state machine alive so the queued
            // connection-close can be transmitted to the peer, notifying the
            // sender to stop.
            self.transmission.clear_acks();
            self.transmission.on_error();
        } else if matches!(source, Location::Local) && !is_idle_timeout {
            // Other local errors (credential mismatch, truncated transport, etc.)
            // are handled by separate error notification mechanisms (e.g.,
            // FlowReset secret control packets). Fully clear the transmission
            // state so the receiver shuts down without attempting to send a
            // connection-close.
            self.transmission.clear();
            self.transmission.on_error();
        } else {
            self.transmission.clear();
            let _ = self.state.on_app_read_reset();
            self.silent_shutdown();
        }
    }

    #[inline]
    pub fn check_error(&self) -> Result<(), Error> {
        // if we already received/read all of the data then filter out errors
        ensure!(
            !matches!(self.state, Receiver::DataRead | Receiver::DataRecvd),
            Ok(())
        );

        if let Some(err) = self.error {
            Err(err.error)
        } else {
            Ok(())
        }
    }

    #[inline]
    pub fn on_timeout<Clk, Ld, Pub>(&mut self, clock: &Clk, load_last_activity: Ld, publisher: &Pub)
    where
        Clk: Clock + ?Sized,
        Ld: FnOnce() -> Timestamp,
        Pub: event::ConnectionPublisher,
    {
        let now = clock.get_time();
        if self.poll_idle_timer(clock, load_last_activity).is_ready() {
            self.silent_shutdown();

            // only transition to an error state if we didn't receive everything
            ensure!(matches!(self.state, Receiver::Recv | Receiver::SizeKnown));

            // we don't want to transmit anything so enter a terminal state
            let mut did_transition = false;
            did_transition |= self.state.on_reset().is_ok();
            did_transition |= self.state.on_app_read_reset().is_ok();
            if did_transition {
                self.on_error(error::Kind::IdleTimeout, Location::Local, publisher);
            }

            return;
        }

        // check if the throttled ACK timer has expired
        self.transmission.on_timeout(clock);

        // check if we need to retransmit MAX_DATA
        self.max_data.on_timeout(clock);

        // if the tick timer expired, then copy the current idle timeout target
        if self.tick_timer.poll_expiration(now).is_ready() {
            self.tick_timer = self.idle_timer.clone();
        }
    }

    #[inline]
    fn poll_idle_timer<Clk, Ld>(&mut self, clock: &Clk, load_last_activity: Ld) -> Poll<()>
    where
        Clk: Clock + ?Sized,
        Ld: FnOnce() -> Timestamp,
    {
        let now = clock.get_time();

        // check the idle timer first
        ready!(self.idle_timer.poll_expiration(now));

        // if that expired then load the last activity from the peer and update the idle timer with
        // the value
        let last_peer_activity = load_last_activity();
        self.update_idle_timer(&last_peer_activity);

        // check the idle timer once more before returning
        ready!(self.idle_timer.poll_expiration(now));

        Poll::Ready(())
    }

    #[inline]
    fn silent_shutdown(&mut self) {
        self.idle_timer.cancel();
        self.tick_timer.cancel();
        self.transmission.clear();
        tracing::trace!("silent_shutdown");
    }

    #[inline]
    pub fn on_transmit<K, T, Clk, Pub>(
        &mut self,
        key: &K,
        credentials: &Credentials,
        stream_id: stream::Id,
        source_queue_id: Option<VarInt>,
        queue: &mut T,
        clock: &Clk,
        publisher: &Pub,
    ) where
        K: crypto::seal::control::Stream,
        T: TransmitQueue,
        Clk: Clock + ?Sized,
        Pub: event::ConnectionPublisher,
    {
        (if self.error.is_none() {
            Self::on_transmit_ack
        } else {
            Self::on_transmit_error
        })(
            self,
            key,
            credentials,
            stream_id,
            source_queue_id,
            queue,
            clock,
            publisher,
        )
    }

    #[inline]
    fn on_transmit_ack<K, T, Clk, Pub>(
        &mut self,
        key: &K,
        credentials: &Credentials,
        stream_id: stream::Id,
        source_queue_id: Option<VarInt>,
        queue: &mut T,
        clock: &Clk,
        publisher: &Pub,
    ) where
        K: crypto::seal::control::Stream,
        T: TransmitQueue,
        Clk: Clock + ?Sized,
        Pub: event::ConnectionPublisher,
    {
        ensure!(self.should_transmit());

        // get how many intervals we're tracking - the more there are, the more loss the network
        // is experiencing
        let intervals = self.transmission.interval_len();

        // The value of `20` is somewhat arbitrary but doing some worst-case math the ACK ranges with
        // 20 segments would consume about 20-25% of the packet this is a good starting point.
        // We don't want to go too much lower otherwise we end up spamming ACKs.
        let duplicate_threshold = 20;

        // Only duplicate ACKs when the number of ACK intervals is very large,
        // indicating severe loss. Recovery packets already trigger immediate ACKs
        // via `on_recovery_packet_active()`, so we don't need blanket duplication
        // whenever recovery packets exist — that was doubling the control packet
        // rate during normal operation.
        let count = if intervals > duplicate_threshold {
            2
        } else {
            1
        };

        for _ in 0..count {
            let res = queue.push_with(|mut buffer| {
                let mtu = self.mtu();

                // output.set_ecn(self.ecn());

                let packet_number = self.next_pn();

                let encoder = EncoderBuffer::new(&mut buffer[..]);

                let max_data = self.max_data.frame();
                let encoding_size: VarInt = max_data.encoding_size().try_into().unwrap();

                let (stream_ack, recovery_ack, encoding_size) =
                    self.transmission.encoding(encoding_size, mtu, clock);

                tracing::trace!(?stream_ack, ?recovery_ack, ?max_data);

                let frame = ((max_data, stream_ack), recovery_ack);

                let packet_len = control::encoder::encode(
                    encoder,
                    source_queue_id,
                    Some(stream_id),
                    packet_number,
                    Default::default(),
                    encoding_size,
                    &frame,
                    key,
                    credentials,
                );

                if packet_len == 0 {
                    return 0;
                }

                publisher.on_stream_control_packet_transmitted(
                    event::builder::StreamControlPacketTransmitted {
                        packet_len,
                        control_data_len: encoding_size.as_u64() as usize,
                        packet_number: packet_number.as_u64(),
                    },
                );

                self.on_packet_sent(packet_number, clock);

                packet_len
            });

            if res.is_err() {
                break;
            }
        }

        // Arm the retransmit timer for MAX_DATA if we just transitioned to Inflight
        self.max_data.arm_retransmit_timer(clock);
    }

    #[inline]
    fn on_transmit_error<K, T, Clk, Pub>(
        &mut self,
        control_key: &K,
        credentials: &Credentials,
        stream_id: stream::Id,
        source_queue_id: Option<VarInt>,
        queue: &mut T,
        clock: &Clk,
        publisher: &Pub,
    ) where
        K: crypto::seal::control::Stream,
        T: TransmitQueue,
        Clk: Clock + ?Sized,
        Pub: event::ConnectionPublisher,
    {
        ensure!(self.should_transmit());
        let Some(error) = self.error else {
            return;
        };
        // Only transmit errors that we originated
        ensure!(matches!(error.source, Location::Local));

        let _ = queue.push_with(|mut buffer| {
            let packet_number = self.next_pn();

            let encoder = EncoderBuffer::new(&mut buffer[..]);

            let frame = error
                .error
                .as_connection_close()
                .unwrap_or_else(|| s2n_quic_core::transport::Error::NO_ERROR.into());

            let encoding_size = frame.encoding_size().try_into().unwrap();

            let result = control::encoder::encode(
                encoder,
                source_queue_id,
                Some(stream_id),
                packet_number,
                Default::default(),
                encoding_size,
                &frame,
                control_key,
                credentials,
            );

            if result == 0 {
                return 0;
            }

            tracing::debug!(connection_close = ?frame);

            publisher.on_stream_control_packet_transmitted(
                event::builder::StreamControlPacketTransmitted {
                    packet_len: result,
                    control_data_len: encoding_size.as_u64() as usize,
                    packet_number: packet_number.as_u64(),
                },
            );

            self.on_packet_sent(packet_number, clock);

            result
        });
    }

    #[inline]
    fn next_pn(&mut self) -> VarInt {
        VarInt::new(self.control_packet_number).expect("2^62 is a lot of packets")
    }

    #[inline]
    fn on_packet_sent<Clk: Clock + ?Sized>(&mut self, packet_number: VarInt, clock: &Clk) {
        // record the fin_ack packet number so we can shutdown more quickly
        if !matches!(self.state, Receiver::Recv | Receiver::SizeKnown)
            && self.fin_ack_packet_number.is_none()
        {
            self.fin_ack_packet_number = Some(packet_number);
        }

        // Record send timestamp for feedback-loop RTT estimation
        self.recv_budget
            .on_control_sent(packet_number, clock.get_time());

        self.control_packet_number += 1;
        self.transmission.on_transmit(packet_number);
        self.max_data.on_transmit(packet_number);
    }
}

impl timer::Provider for State {
    #[inline]
    fn timers<Q: timer::Query>(&self, query: &mut Q) -> timer::Result {
        self.idle_timer.timers(query)?;
        self.tick_timer.timers(query)?;
        self.transmission.timers(query)?;
        self.max_data.timers(query)?;
        Ok(())
    }
}

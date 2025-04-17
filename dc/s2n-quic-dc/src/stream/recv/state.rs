// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    allocator::Allocator,
    clock,
    credentials::Credentials,
    crypto::{self, UninitSlice},
    packet::{control, stream},
    stream::{
        recv::{
            ack,
            error::{self, Error},
            packet, probes,
        },
        TransportFeatures, DEFAULT_IDLE_TIMEOUT,
    },
};
use core::{task::Poll, time::Duration};
use s2n_codec::{EncoderBuffer, EncoderValue};
use s2n_quic_core::{
    buffer::{self, reader::storage::Infallible as _},
    dc::ApplicationParams,
    ensure,
    frame::{self, ack::EcnCounts},
    inet::ExplicitCongestionNotification,
    packet::number::PacketNumberSpace,
    ready,
    stream::state::Receiver,
    time::{
        timer::{self, Provider as _},
        Clock, Timer, Timestamp,
    },
    varint::VarInt,
};

#[derive(Debug)]
pub struct State {
    ecn_counts: EcnCounts,
    control_packet_number: u64,
    stream_ack: ack::Space,
    recovery_ack: ack::Space,
    state: Receiver,
    idle_timer: Timer,
    idle_timeout: Duration,
    // maintains a stable tick timer to avoid timer churn in the platform timer
    tick_timer: Timer,
    _should_transmit: bool,
    is_reliable: bool,
    max_data: VarInt,
    max_data_window: VarInt,
    error: Option<Error>,
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
        let initial_max_data = params.local_recv_max_data;

        // set up the idle timer in case we never read anything
        let now = clock.get_time();
        let idle_timeout = params.max_idle_timeout().unwrap_or(DEFAULT_IDLE_TIMEOUT);
        let mut idle_timer = Timer::default();
        idle_timer.set(now + idle_timeout);

        // the tick timer just inherits the idle timer since that's the current stable target
        let tick_timer = idle_timer.clone();

        Self {
            is_reliable: stream_id.is_reliable,
            ecn_counts: Default::default(),
            control_packet_number: Default::default(),
            stream_ack: Default::default(),
            recovery_ack: Default::default(),
            state: Default::default(),
            idle_timer,
            idle_timeout,
            tick_timer,
            _should_transmit: false,
            max_data: initial_max_data,
            max_data_window: initial_max_data,
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
    pub fn stop_sending(&mut self, error: s2n_quic_core::application::Error) {
        // if we've already received everything then no need to notify the peer to stop
        ensure!(matches!(self.state, Receiver::Recv | Receiver::SizeKnown));
        self.on_error(error::Kind::ApplicationError { error });
    }

    #[inline]
    pub fn on_read_buffer<B, C, Clk>(&mut self, out_buf: &mut B, chunk: &mut C, _clock: &Clk)
    where
        B: buffer::Duplex<Error = core::convert::Infallible>,
        C: buffer::writer::Storage,
        Clk: Clock + ?Sized,
    {
        // try copying the out_buf into the application chunk, if possible
        if chunk.has_remaining_capacity() && !out_buf.buffer_is_empty() {
            out_buf.infallible_copy_into(chunk);
        }

        // record our new max data value
        let new_max_data = out_buf
            .current_offset()
            .saturating_add(self.max_data_window);

        if new_max_data > self.max_data {
            self.max_data = new_max_data;
            self.needs_transmission("new_max_data");
        }

        // if we know the final offset then update the sate
        if out_buf.final_offset().is_some() {
            let _ = self.state.on_receive_fin();
        }

        // if we've received everything then update the state
        if out_buf.has_buffered_fin() && self.state.on_receive_all_data().is_ok() {
            self.needs_transmission("receive_all_data");
        }

        // if we've completely drained the out buffer try transitioning to the final state
        if out_buf.is_consumed() && self.state.on_app_read_all_data().is_ok() {
            self.needs_transmission("app_read_all_data");
        }
    }

    #[inline]
    pub fn precheck_stream_packet(
        &mut self,
        credentials: &Credentials,
        packet: &stream::decoder::Packet,
    ) -> Result<(), Error> {
        match self.precheck_stream_packet_impl(credentials, packet) {
            Ok(()) => Ok(()),
            Err(err) => {
                if err.is_fatal(&self.features) {
                    tracing::error!(fatal_error = %err, ?packet);
                    self.on_error(err);
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
                .stream_ack
                .packets
                .max_value()
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
    pub fn on_stream_packet<D, C, B, Clk>(
        &mut self,
        opener: &D,
        control: &C,
        credentials: &Credentials,
        packet: &mut stream::decoder::Packet,
        ecn: ExplicitCongestionNotification,
        clock: &Clk,
        out_buf: &mut B,
    ) -> Result<(), Error>
    where
        D: crypto::open::Application,
        C: crypto::open::control::Stream,
        Clk: Clock + ?Sized,
        B: buffer::Duplex<Error = core::convert::Infallible>,
    {
        probes::on_stream_packet(
            credentials.id,
            *packet.stream_id(),
            packet.tag().packet_space(),
            packet.packet_number(),
            packet.stream_offset(),
            packet.payload().len(),
            packet.is_fin(),
            packet.is_retransmission(),
        );

        match self.on_stream_packet_impl(opener, control, credentials, packet, ecn, clock, out_buf)
        {
            Ok(()) => Ok(()),
            Err(err) => {
                if err.is_fatal(&self.features) {
                    tracing::error!(fatal_error = %err, ?packet);
                    self.on_error(err);
                } else {
                    tracing::debug!(non_fatal_error = %err, ?packet);
                }
                Err(err)
            }
        }
    }

    #[inline]
    fn on_stream_packet_impl<D, C, B, Clk>(
        &mut self,
        opener: &D,
        control: &C,
        credentials: &Credentials,
        packet: &mut stream::decoder::Packet,
        ecn: ExplicitCongestionNotification,
        clock: &Clk,
        out_buf: &mut B,
    ) -> Result<(), Error>
    where
        D: crypto::open::Application,
        C: crypto::open::control::Stream,
        Clk: Clock + ?Sized,
        B: buffer::Duplex<Error = core::convert::Infallible>,
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
            credentials,
            receiver: self,
        };

        if !is_max_data_ok {
            // ensure the packet is authentic before resetting the stream
            let _ = packet.read_chunk(usize::MAX)?;

            tracing::error!(
                message = "max data exceeded",
                allowed = packet.receiver.max_data.as_u64(),
                requested = packet
                    .packet
                    .stream_offset()
                    .as_u64()
                    .saturating_add(packet.packet.payload().len() as u64),
            );

            let error = error::Kind::MaxDataExceeded.err();
            self.on_error(error);
            return Err(error);
        }

        // decrypt and write the packet to the provided buffer
        out_buf.read_from(&mut packet)?;

        let mut chunk = buffer::writer::storage::Empty;
        self.on_read_buffer(out_buf, &mut chunk, clock);

        Ok(())
    }

    #[inline]
    pub(super) fn on_stream_packet_in_place<D, C, Clk>(
        &mut self,
        crypto: &D,
        control: &C,
        credentials: &Credentials,
        packet: &mut stream::decoder::Packet,
        ecn: ExplicitCongestionNotification,
        clock: &Clk,
    ) -> Result<(), Error>
    where
        D: crypto::open::Application,
        C: crypto::open::control::Stream,
        Clk: Clock + ?Sized,
    {
        // ensure the packet is authentic before processing it
        let res = packet.decrypt_in_place(crypto, control);

        probes::on_stream_packet_decrypted(
            credentials.id,
            *packet.stream_id(),
            packet.tag().packet_space(),
            packet.packet_number(),
            packet.stream_offset(),
            packet.payload().len(),
            packet.is_fin(),
            packet.is_retransmission(),
            res.is_ok(),
        );

        res?;

        self.on_cleartext_stream_packet(packet, ecn, clock)
    }

    #[inline]
    pub(super) fn on_stream_packet_copy<D, C, Clk>(
        &mut self,
        crypto: &D,
        control: &C,
        credentials: &Credentials,
        packet: &mut stream::decoder::Packet,
        ecn: ExplicitCongestionNotification,
        payload_out: &mut UninitSlice,
        clock: &Clk,
    ) -> Result<(), Error>
    where
        D: crypto::open::Application,
        C: crypto::open::control::Stream,
        Clk: Clock + ?Sized,
    {
        // ensure the packet is authentic before processing it
        let res = packet.decrypt(crypto, control, payload_out);

        probes::on_stream_packet_decrypted(
            credentials.id,
            *packet.stream_id(),
            packet.tag().packet_space(),
            packet.packet_number(),
            packet.stream_offset(),
            packet.payload().len(),
            packet.is_fin(),
            packet.is_retransmission(),
            res.is_ok(),
        );

        res?;

        self.on_cleartext_stream_packet(packet, ecn, clock)
    }

    #[inline]
    fn ensure_max_data(&self, packet: &stream::decoder::Packet) -> bool {
        // we only need to enforce flow control for non-controlled transport
        ensure!(!self.features.is_flow_controlled(), true);

        self.max_data
            .as_u64()
            .checked_sub(packet.payload().len() as u64)
            .and_then(|v| v.checked_sub(packet.stream_offset().as_u64()))
            .is_some()
    }

    #[inline]
    fn on_cleartext_stream_packet<Clk>(
        &mut self,
        packet: &stream::decoder::Packet,
        ecn: ExplicitCongestionNotification,
        clock: &Clk,
    ) -> Result<(), Error>
    where
        Clk: Clock + ?Sized,
    {
        tracing::trace!(
            stream_id = %packet.stream_id(),
            stream_offset = packet.stream_offset().as_u64(),
            payload_len = packet.payload().len(),
            final_offset = ?packet.final_offset().map(|v| v.as_u64()),
        );

        let space = match packet.tag().packet_space() {
            stream::PacketSpace::Stream => &mut self.stream_ack,
            stream::PacketSpace::Recovery => &mut self.recovery_ack,
        };
        ensure!(
            space.filter.on_packet(packet).is_ok(),
            Err(error::Kind::Duplicate.err())
        );

        let packet_number = PacketNumberSpace::Initial.new_packet_number(packet.packet_number());
        if let Err(err) = space.packets.insert_packet_number(packet_number) {
            tracing::debug!("could not record packet number {packet_number} with error {err:?}");
        }

        // if we got a new packet then we'll need to transmit an ACK
        self.needs_transmission("new_packet");

        // update the idle timer since we received a valid packet
        if matches!(self.state, Receiver::Recv | Receiver::SizeKnown)
            || packet.stream_offset() == VarInt::ZERO
        {
            self.update_idle_timer(clock);
        }

        // TODO process control data
        let _ = packet.control_data();

        self.ecn_counts.increment(ecn);

        if !self.is_reliable {
            // TODO should we perform loss detection on the receiver and reset the stream if we have a big
            // enough gap?
        }

        // clean up any ACK state that we can
        self.on_next_expected_control_packet(packet.next_expected_control_packet());

        Ok(())
    }

    #[inline]
    pub fn should_transmit(&self) -> bool {
        self._should_transmit
    }

    #[inline]
    pub fn on_transport_close(&mut self) {
        // only stream transports can be closed
        ensure!(self.features.is_stream());

        // only error out if we're still expecting more data
        ensure!(matches!(self.state, Receiver::Recv | Receiver::SizeKnown));

        self.on_error(error::Kind::TruncatedTransport);
    }

    #[inline]
    fn needs_transmission(&mut self, reason: &str) {
        if self.error.is_none() {
            // we only transmit errors for reliable + flow-controlled transports
            if self.features.is_reliable() && self.features.is_flow_controlled() {
                tracing::trace!(skipping_transmission = reason);
                return;
            }
        }

        if !self._should_transmit {
            tracing::trace!(needs_transmission = reason);
        }
        self._should_transmit = true;
    }

    #[inline]
    fn on_next_expected_control_packet(&mut self, next_expected_control_packet: VarInt) {
        if let Some(largest_delivered_control_packet) =
            next_expected_control_packet.checked_sub(VarInt::from_u8(1))
        {
            self.stream_ack
                .on_largest_delivered_packet(largest_delivered_control_packet);
            self.recovery_ack
                .on_largest_delivered_packet(largest_delivered_control_packet);

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
        1200
    }

    #[inline]
    fn ecn(&self) -> ExplicitCongestionNotification {
        // TODO how do we decide what to send on control messages
        ExplicitCongestionNotification::Ect0
    }

    #[inline]
    #[track_caller]
    pub fn on_error<E>(&mut self, error: E)
    where
        Error: From<E>,
    {
        let error = Error::from(error);
        debug_assert!(error.is_fatal(&self.features));
        let _ = self.state.on_reset();
        self.stream_ack.clear();
        self.recovery_ack.clear();
        self.needs_transmission("on_error");

        // make sure we haven't already set the error from something else
        ensure!(self.error.is_none());
        self.error = Some(error);
    }

    #[inline]
    pub fn check_error(&self) -> Result<(), Error> {
        // if we already received/read all of the data then filter out errors
        ensure!(
            !matches!(self.state, Receiver::DataRead | Receiver::DataRecvd),
            Ok(())
        );

        if let Some(err) = self.error {
            Err(err)
        } else {
            Ok(())
        }
    }

    #[inline]
    pub fn on_timeout<Clk, Ld>(&mut self, clock: &Clk, load_last_activity: Ld)
    where
        Clk: Clock + ?Sized,
        Ld: FnOnce() -> Timestamp,
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
                self.on_error(error::Kind::IdleTimeout);
                // override the transmission since we're just timing out
                self._should_transmit = false;
            }

            return;
        }

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
        self._should_transmit = false;
        self.idle_timer.cancel();
        self.tick_timer.cancel();
        self.stream_ack.clear();
        self.recovery_ack.clear();
        tracing::trace!("silent_shutdown");
    }

    #[inline]
    pub fn on_transmit<K, A, Clk>(
        &mut self,
        key: &K,
        credentials: &Credentials,
        stream_id: stream::Id,
        source_queue_id: Option<VarInt>,
        output: &mut A,
        clock: &Clk,
    ) where
        K: crypto::seal::control::Stream,
        A: Allocator,
        Clk: Clock + ?Sized,
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
            output,
            // avoid querying the clock for every transmitted packet
            &clock::Cached::new(clock),
        )
    }

    #[inline]
    fn on_transmit_ack<K, A, Clk>(
        &mut self,
        key: &K,
        credentials: &Credentials,
        stream_id: stream::Id,
        source_queue_id: Option<VarInt>,
        output: &mut A,
        _clock: &Clk,
    ) where
        K: crypto::seal::control::Stream,
        A: Allocator,
        Clk: Clock + ?Sized,
    {
        ensure!(self.should_transmit());

        let mtu = self.mtu();

        output.set_ecn(self.ecn());

        let packet_number = self.next_pn();

        ensure!(let Some(segment) = output.alloc());

        let buffer = output.get_mut(&segment);
        buffer.resize(mtu as _, 0);

        let encoder = EncoderBuffer::new(buffer);

        // TODO compute this by storing the time that we received the max packet number
        let ack_delay = VarInt::ZERO;

        let max_data = frame::MaxData {
            maximum_data: self.max_data,
        };
        let max_data_encoding_size: VarInt = max_data.encoding_size().try_into().unwrap();

        let (stream_ack, max_data_encoding_size) = self.stream_ack.encoding(
            max_data_encoding_size,
            ack_delay,
            Some(self.ecn_counts),
            mtu,
        );
        let (recovery_ack, max_data_encoding_size) =
            self.recovery_ack
                .encoding(max_data_encoding_size, ack_delay, None, mtu);

        let encoding_size = max_data_encoding_size;

        tracing::trace!(?stream_ack, ?recovery_ack, ?max_data);

        let frame = ((max_data, stream_ack), recovery_ack);

        let result = control::encoder::encode(
            encoder,
            source_queue_id,
            Some(stream_id),
            packet_number,
            VarInt::ZERO,
            &mut &[][..],
            encoding_size,
            &frame,
            key,
            credentials,
        );

        match result {
            0 => {
                output.free(segment);
                return;
            }
            packet_len => {
                buffer.truncate(packet_len);
                // TODO duplicate the transmission in case we have a lot of gaps in packets
                output.push(segment);
            }
        }

        for (ack, space) in [
            (&self.stream_ack, stream::PacketSpace::Stream),
            (&self.recovery_ack, stream::PacketSpace::Recovery),
        ] {
            let metrics = (
                ack.packets.min_value(),
                ack.packets.max_value(),
                ack.packets.interval_len().checked_sub(1),
            );
            if let (Some(min), Some(max), Some(gaps)) = metrics {
                probes::on_transmit_control(
                    credentials.id,
                    stream_id,
                    space,
                    packet_number,
                    min,
                    max,
                    gaps,
                );
            };
        }

        // make sure we sent a packet
        ensure!(!output.is_empty());

        // record the max value we've seen for removing old packet numbers
        self.stream_ack.on_transmit(packet_number);
        self.recovery_ack.on_transmit(packet_number);

        self.on_packet_sent(packet_number);
    }

    #[inline]
    fn on_transmit_error<K, A, Clk>(
        &mut self,
        control_key: &K,
        credentials: &Credentials,
        stream_id: stream::Id,
        source_queue_id: Option<VarInt>,
        output: &mut A,
        _clock: &Clk,
    ) where
        K: crypto::seal::control::Stream,
        A: Allocator,
        Clk: Clock + ?Sized,
    {
        ensure!(self.should_transmit());

        let mtu = self.mtu() as usize;

        output.set_ecn(self.ecn());

        let packet_number = self.next_pn();

        ensure!(let Some(segment) = output.alloc());

        let buffer = output.get_mut(&segment);
        buffer.resize(mtu, 0);

        let encoder = EncoderBuffer::new(buffer);

        let frame = self
            .error
            .as_ref()
            .and_then(|err| err.connection_close())
            .unwrap_or_else(|| s2n_quic_core::transport::Error::NO_ERROR.into());

        let encoding_size = frame.encoding_size().try_into().unwrap();

        let result = control::encoder::encode(
            encoder,
            source_queue_id,
            Some(stream_id),
            packet_number,
            VarInt::ZERO,
            &mut &[][..],
            encoding_size,
            &frame,
            control_key,
            credentials,
        );

        match result {
            0 => {
                output.free(segment);
                return;
            }
            packet_len => {
                buffer.truncate(packet_len);
                output.push(segment);
            }
        }

        tracing::debug!(connection_close = ?frame);

        // clean things up
        self.stream_ack.clear();
        self.recovery_ack.clear();

        probes::on_transmit_close(credentials.id, stream_id, packet_number, frame.error_code);

        self.on_packet_sent(packet_number);
    }

    #[inline]
    fn next_pn(&mut self) -> VarInt {
        VarInt::new(self.control_packet_number).expect("2^62 is a lot of packets")
    }

    #[inline]
    fn on_packet_sent(&mut self, packet_number: VarInt) {
        // record the fin_ack packet number so we can shutdown more quickly
        if !matches!(self.state, Receiver::Recv | Receiver::SizeKnown)
            && self.fin_ack_packet_number.is_none()
        {
            self.fin_ack_packet_number = Some(packet_number);
        }

        self.control_packet_number += 1;
        self._should_transmit = false;
    }
}

impl timer::Provider for State {
    #[inline]
    fn timers<Q: timer::Query>(&self, query: &mut Q) -> timer::Result {
        self.idle_timer.timers(query)?;
        self.tick_timer.timers(query)?;
        Ok(())
    }
}

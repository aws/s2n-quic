use crate::{
    allocator::{Allocator, Segment},
    congestion,
    crypto::{decrypt, encrypt, UninitSlice},
    packet::{
        self,
        stream::{self, decoder, encoder},
    },
    path::Parameters,
    stream::{
        packet_map, packet_number, processing,
        send::{
            error::Error,
            filter::Filter,
            probes,
            transmission::Type as TransmissionType,
            worker::{self, retransmission::Segment as Retransmission},
        },
    },
};
use s2n_codec::{DecoderBufferMut, EncoderBuffer};
use s2n_quic_core::{
    branch, ensure,
    frame::{self, FrameMut},
    inet::ExplicitCongestionNotification,
    interval_set::IntervalSet,
    packet::number::PacketNumberSpace,
    path::{ecn, INITIAL_PTO_BACKOFF},
    random,
    recovery::{Pto, RttEstimator},
    stream::state,
    time::{timer, Clock, Timer, Timestamp},
    varint::VarInt,
};
use std::collections::BinaryHeap;
use tracing::{debug, trace};

mod checker;
mod probe;
pub mod retransmission;
pub mod transmission;

type PacketMap<R> = packet_map::Map<transmission::Info<R>>;

#[derive(Debug)]
pub struct State<S: Segment, R: Segment> {
    pub stream_id: stream::Id,
    rtt_estimator: RttEstimator,
    pub sent_packets: PacketMap<R>,
    pub state: state::Sender,
    control_filter: Filter,
    pub retransmissions: BinaryHeap<retransmission::Segment<S>>,
    next_expected_control_packet: VarInt,
    pub cca: congestion::Controller,
    ecn: ecn::Controller,
    pub pto: Pto,
    pto_backoff: u32,
    idle_timer: Timer,
    pub error: Option<Error>,
    unacked_ranges: IntervalSet<VarInt>,
    max_sent_offset: VarInt,
    pub max_data: VarInt,
    checker: checker::Checker,
}

impl<S: Segment, R: Segment> State<S, R> {
    #[inline]
    pub fn new(stream_id: stream::Id, params: &Parameters) -> Self {
        let mtu = params.max_mtu;
        let initial_max_data = params.remote_max_data;

        // initialize the pending data left to send
        let mut unacked_ranges = IntervalSet::new();
        unacked_ranges.insert(VarInt::ZERO..=VarInt::MAX).unwrap();

        let cca = congestion::Controller::new(mtu.into());
        let max_sent_offset = VarInt::ZERO;

        let mut checker = checker::Checker::default();
        checker.on_max_data(initial_max_data);

        Self {
            stream_id,
            next_expected_control_packet: VarInt::ZERO,
            rtt_estimator: crate::recovery::rtt_estimator(),
            cca,
            sent_packets: Default::default(),
            control_filter: Default::default(),
            ecn: ecn::Controller::default(),
            state: Default::default(),
            retransmissions: Default::default(),
            pto: Pto::default(),
            pto_backoff: INITIAL_PTO_BACKOFF,
            idle_timer: Default::default(),
            error: None,
            unacked_ranges,
            max_sent_offset,
            max_data: initial_max_data,
            checker,
        }
    }

    /// Returns the current flow offset
    #[inline]
    pub fn flow_offset(&self) -> VarInt {
        let extra_window = self
            .cca
            .congestion_window()
            .saturating_sub(self.cca.bytes_in_flight());
        self.max_data
            .min(self.max_sent_offset + extra_window as usize)
    }

    /// Called by the worker when it receives a control packet from the peer
    #[inline]
    pub fn on_control_packet<D, Clk, A>(
        &mut self,
        decrypt_key: &mut D,
        ecn: ExplicitCongestionNotification,
        packet: &mut packet::control::decoder::Packet,
        random: &mut dyn random::Generator,
        clock: &Clk,
        message: &mut A,
    ) -> Result<(), processing::Error>
    where
        D: decrypt::Key,
        Clk: Clock,
        A: Allocator<Segment = S, Retransmission = R>,
    {
        match self.on_control_packet_impl(decrypt_key, ecn, packet, random, clock, message) {
            Ok(None) => {}
            Ok(Some(error)) => return Err(error),
            Err(error) => {
                self.on_error(error, message);
            }
        }

        self.invariants();

        Ok(())
    }

    #[inline(always)]
    fn on_control_packet_impl<D, Clk, A>(
        &mut self,
        decrypt_key: &mut D,
        _ecn: ExplicitCongestionNotification,
        packet: &mut packet::control::decoder::Packet,
        random: &mut dyn random::Generator,
        clock: &Clk,
        message: &mut A,
    ) -> Result<Option<processing::Error>, Error>
    where
        D: decrypt::Key,
        Clk: Clock,
        A: Allocator<Segment = S, Retransmission = R>,
    {
        probes::on_control_packet(
            decrypt_key.credentials().id,
            self.stream_id,
            packet.packet_number(),
            packet.control_data().len(),
        );

        // only process the packet after we know it's authentic
        let res = decrypt_key.decrypt(
            packet.crypto_nonce(),
            packet.header(),
            &[],
            packet.auth_tag(),
            UninitSlice::new(&mut []),
        );

        probes::on_control_packet_decrypted(
            decrypt_key.credentials().id,
            self.stream_id,
            packet.packet_number(),
            packet.control_data().len(),
            res.is_ok(),
        );

        // drop the packet if it failed to authenticate
        if let Err(err) = res {
            return Ok(Some(err.into()));
        }

        // check if we've already seen the packet
        ensure!(
            self.control_filter.on_packet(packet).is_ok(),
            return {
                probes::on_control_packet_duplicate(
                    decrypt_key.credentials().id,
                    self.stream_id,
                    packet.packet_number(),
                    packet.control_data().len(),
                );
                // drop the packet if we've already seen it
                Ok(Some(processing::Error::Duplicate))
            }
        );

        let packet_number = packet.packet_number();

        // raise our next expected control packet
        {
            let pn = packet_number.saturating_add(VarInt::from_u8(1));
            let pn = self.next_expected_control_packet.max(pn);
            self.next_expected_control_packet = pn;
        }

        let mut newly_acked = false;

        {
            let mut decoder = DecoderBufferMut::new(packet.control_data_mut());
            while !decoder.is_empty() {
                let (frame, remaining) = decoder
                    .decode::<FrameMut>()
                    .map_err(|decoder| Error::FrameError { decoder })?;
                decoder = remaining;

                trace!(?frame);

                match frame {
                    FrameMut::Padding(_) => {
                        continue;
                    }
                    FrameMut::Ping(_) => {
                        // no need to do anything special here
                    }
                    FrameMut::Ack(ack) => {
                        self.on_frame_ack(
                            decrypt_key,
                            &ack,
                            random,
                            clock,
                            message,
                            &mut newly_acked,
                        )?;
                    }
                    FrameMut::MaxData(frame) => {
                        if self.max_data < frame.maximum_data {
                            self.max_data = frame.maximum_data;
                            self.checker.on_max_data(frame.maximum_data);
                        }
                    }
                    FrameMut::ConnectionClose(close) => {
                        debug!(connection_close = ?close, state = ?self.state);

                        probes::on_close(
                            decrypt_key.credentials().id,
                            self.stream_id,
                            packet_number,
                            close.error_code,
                        );

                        // if there was no error and we transmitted everything then just shut the
                        // stream down
                        if close.error_code == VarInt::ZERO
                            && close.frame_type.is_some()
                            && self.state.on_recv_all_acks().is_ok()
                        {
                            self.clean_up(message);
                            // transmit one more PTO packet so we can ACK the peer's
                            // CONNECTION_CLOSE frame and they can shutdown quickly. Otherwise,
                            // they'll need to hang around to respond to potential loss.
                            self.pto.force_transmit();
                            return Ok(None);
                        }

                        // no need to transmit a reset back to the peer - just close it
                        let _ = self.state.on_send_reset();
                        let _ = self.state.on_recv_reset_ack();
                        let error = if close.frame_type.is_some() {
                            Error::TransportError {
                                code: close.error_code,
                            }
                        } else {
                            Error::ApplicationError {
                                error: close.error_code.into(),
                            }
                        };
                        return Err(error);
                    }
                    _ => continue,
                }
            }
        }

        if newly_acked {
            if self.pto_backoff != INITIAL_PTO_BACKOFF {
                probes::on_pto_backoff_reset(
                    decrypt_key.credentials().id,
                    self.stream_id,
                    self.pto_backoff,
                );
            }

            self.pto_backoff = INITIAL_PTO_BACKOFF;
        }

        trace!(
            retransmissions = self.retransmissions.len(),
            packets_in_flight = self.sent_packets.iter().count(),
        );

        // try to transition to the final state if we've sent all of the data
        if self.unacked_ranges.is_empty() && self.state.on_recv_all_acks().is_ok() {
            self.clean_up(message);
            // transmit one more PTO packet so we can ACK the peer's
            // CONNECTION_CLOSE frame and they can shutdown quickly. Otherwise,
            // they'll need to hang around to respond to potential loss.
            self.pto.force_transmit();
        }

        // make sure we have all of the pending packets we need to finish the transmission
        if !self.state.is_terminal() {
            // TODO pass `unacked_ranges`
            self.checker
                .check_pending_packets(&self.sent_packets, &self.retransmissions);
        }

        // re-arm the idle timer as long as we're still sending data
        if self.state.is_ready() || self.state.is_sending() || self.state.is_data_sent() {
            self.arm_idle_timer(clock);
        }

        Ok(None)
    }

    #[inline]
    fn on_frame_ack<D, Ack, Clk, A>(
        &mut self,
        decrypt_key: &mut D,
        ack: &frame::Ack<Ack>,
        random: &mut dyn random::Generator,
        clock: &Clk,
        message: &mut A,
        newly_acked: &mut bool,
    ) -> Result<(), Error>
    where
        D: decrypt::Key,
        Ack: frame::ack::AckRanges,
        Clk: Clock,
        A: Allocator<Segment = S, Retransmission = R>,
    {
        // TODO get all of this information
        // self.ecn.validate(
        //    newly_acked_ecn_counts,
        //    sent_packet_ecn_counts,
        //    baseline_ecn_counts,
        //    ack_frame_ecn_counts,
        //    now,
        //    rtt,
        //    path,
        //    publisher,
        // );

        let ack_time = clock.get_time();

        let mut max = None;
        let mut cca_args = None;
        let mut bytes_acked = 0;

        for range in ack.ack_ranges() {
            max = max.max(Some(*range.end()));
            let pmin = PacketNumberSpace::Initial.new_packet_number(*range.start());
            let pmax = PacketNumberSpace::Initial.new_packet_number(*range.end());
            let range = s2n_quic_core::packet::number::PacketNumberRange::new(pmin, pmax);
            for (num, packet) in self.sent_packets.remove_range(range) {
                packet.data.on_ack(num);

                if packet.data.included_fin {
                    let _ = self
                        .unacked_ranges
                        .remove(packet.data.stream_offset..=VarInt::MAX);
                } else {
                    let _ = self.unacked_ranges.remove(packet.data.range());
                }

                self.checker
                    .on_ack(packet.data.stream_offset, packet.data.payload_len);

                self.ecn.on_packet_ack(packet.time_sent, packet.ecn);
                bytes_acked += packet.data.cca_len() as usize;

                // record the most recent packet
                if cca_args
                    .as_ref()
                    .map_or(true, |prev: &(Timestamp, _)| prev.0 < packet.time_sent)
                {
                    cca_args = Some((packet.time_sent, packet.cc_info));
                }

                // free the retransmission segment
                if let Some(segment) = packet.data.retransmission {
                    message.free_retransmission(segment);
                }

                probes::on_packet_ack(
                    decrypt_key.credentials().id,
                    self.stream_id,
                    num.as_u64(),
                    packet.data.packet_len,
                    packet.data.stream_offset,
                    packet.data.payload_len,
                    ack_time.saturating_duration_since(packet.time_sent),
                );

                *newly_acked |= true;
            }
        }

        if let Some((time_sent, cc_info)) = cca_args {
            self.cca.on_packet_ack(
                time_sent,
                bytes_acked,
                cc_info,
                &self.rtt_estimator,
                random,
                ack_time,
            );
        }

        let mut is_unrecoverable = false;

        if let Some(lost_max) = max.and_then(|min| min.checked_sub(VarInt::from_u8(2))) {
            let lost_min = PacketNumberSpace::Initial.new_packet_number(VarInt::ZERO);
            let lost_max = PacketNumberSpace::Initial.new_packet_number(lost_max);
            let range = s2n_quic_core::packet::number::PacketNumberRange::new(lost_min, lost_max);
            for (num, packet) in self.sent_packets.remove_range(range) {
                packet.data.on_loss(num);

                // TODO create a path and publisher
                // self.ecn.on_packet_loss(packet.time_sent, packet.ecn, now, path, publisher);

                self.cca.on_packet_lost(
                    packet.data.cca_len() as _,
                    packet.cc_info,
                    random,
                    ack_time,
                );

                probes::on_packet_lost(
                    decrypt_key.credentials().id,
                    self.stream_id,
                    num.as_u64(),
                    packet.data.packet_len,
                    packet.data.stream_offset,
                    packet.data.payload_len,
                    ack_time.saturating_duration_since(packet.time_sent),
                    packet.data.retransmission.is_some(),
                );

                if let Some(segment) = packet.data.retransmission {
                    let segment = message.retransmit(segment);
                    let retransmission = Retransmission {
                        segment,
                        stream_offset: packet.data.stream_offset,
                        payload_len: packet.data.payload_len,
                        ty: TransmissionType::Stream,
                        included_fin: packet.data.included_fin,
                    };
                    self.retransmissions.push(retransmission);
                } else {
                    // we can only recover reliable streams
                    is_unrecoverable |= packet.data.payload_len > 0 && !self.stream_id.is_reliable;
                }
            }
        }

        ensure!(!is_unrecoverable, Err(Error::RetransmissionFailure));

        self.invariants();

        Ok(())
    }

    /// Called by the worker thread when it becomes aware of the application having transmitted a
    /// segment
    #[inline]
    pub fn on_transmit_segment(
        &mut self,
        packet_number: VarInt,
        time_sent: Timestamp,
        transmission: transmission::Info<R>,
        ecn: ExplicitCongestionNotification,
        mut has_more_app_data: bool,
    ) {
        has_more_app_data |= !self.retransmissions.is_empty();
        let cc_info = self.cca.on_packet_sent(
            time_sent,
            transmission.cca_len(),
            has_more_app_data,
            &self.rtt_estimator,
        );

        // update the max offset that we've transmitted
        self.max_sent_offset = self.max_sent_offset.max(transmission.end_offset());

        // try to transition to start sending
        let _ = self.state.on_send_stream();
        if transmission.included_fin {
            // if the transmission included the final offset, then transition to that state
            let _ = self.state.on_send_fin();
        }

        let info = packet_map::SentPacketInfo {
            data: transmission,
            time_sent,
            ecn,
            cc_info,
        };

        let packet_number = PacketNumberSpace::Initial.new_packet_number(packet_number);
        self.sent_packets.insert(packet_number, info);

        self.invariants();
    }

    #[inline]
    pub fn arm_pto_timer<E, Clk>(&mut self, encrypt_key: &mut E, clock: &Clk)
    where
        E: encrypt::Key,
        Clk: Clock,
    {
        let pto_backoff = self.pto_backoff;
        let pto_period = self
            .rtt_estimator
            .pto_period(pto_backoff, PacketNumberSpace::Initial);
        self.pto.update(clock.get_time(), pto_period);

        probes::on_pto_armed(
            encrypt_key.credentials().id,
            self.stream_id,
            pto_period,
            pto_backoff,
        );
    }

    #[inline]
    pub fn on_transmit<E, Clk, A>(
        &mut self,
        packet_number: &packet_number::Counter,
        encrypt_key: &mut E,
        source_control_port: u16,
        source_stream_port: Option<u16>,
        clock: &Clk,
        message: &mut A,
        send_quantum: &mut usize,
        mtu: u16,
    ) -> Result<(), Error>
    where
        E: encrypt::Key,
        Clk: Clock,
        A: Allocator<Segment = S, Retransmission = R>,
    {
        if let Err(error) = self.on_transmit_recovery_impl(
            packet_number,
            encrypt_key,
            source_control_port,
            source_stream_port,
            clock,
            message,
            send_quantum,
            mtu,
        ) {
            self.on_error(error, message);
            return Err(error);
        }

        Ok(())
    }

    #[inline]
    fn on_transmit_recovery_impl<E, Clk, A>(
        &mut self,
        packet_number: &packet_number::Counter,
        encrypt_key: &mut E,
        source_control_port: u16,
        source_stream_port: Option<u16>,
        clock: &Clk,
        message: &mut A,
        send_quantum: &mut usize,
        mtu: u16,
    ) -> Result<(), Error>
    where
        E: encrypt::Key,
        Clk: Clock,
        A: Allocator<Segment = S, Retransmission = R>,
    {
        // try using a retransmission as a probe
        self.on_transmit_retransmission_probe(message)?;

        self.on_transmit_retransmissions(
            packet_number,
            encrypt_key,
            clock,
            message,
            send_quantum,
            mtu,
        )?;

        self.on_transmit_probe(
            packet_number,
            encrypt_key,
            source_control_port,
            source_stream_port,
            clock,
            message,
            send_quantum,
            mtu,
        )?;

        Ok(())
    }

    #[inline]
    fn on_transmit_retransmission_probe<A>(&mut self, message: &mut A) -> Result<(), Error>
    where
        A: Allocator<Segment = S, Retransmission = R>,
    {
        // We'll only have retransmissions if we're reliable
        ensure!(self.stream_id.is_reliable, Ok(()));

        let mut transmissions = self.pto.transmissions() as usize;
        ensure!(transmissions > 0, Ok(()));

        // Only push a new probe if we don't have existing retransmissions.
        //
        // The retransmissions structure uses a BinaryHeap, which prioritizes the smallest stream
        // offsets, in order to more quickly unblock the peer. If we keep using retransmissions as
        // probes, then it can cause issues where we don't make progress and keep sending the same
        // segments.
        ensure!(self.retransmissions.is_empty(), Ok(()));

        transmissions = transmissions.saturating_sub(self.retransmissions.len());
        ensure!(transmissions > 0, Ok(()));

        let pending = self
            .sent_packets
            .iter()
            .filter(|(_, packet)| packet.data.retransmission.is_some())
            .take(transmissions);

        for (_pn, packet) in pending {
            if let Some(retransmission) = packet.data.retransmission.as_ref() {
                let Some(segment) = message.retransmit_copy(retransmission) else {
                    break;
                };
                let retransmission = Retransmission {
                    segment,
                    ty: TransmissionType::Probe,
                    stream_offset: packet.data.stream_offset,
                    payload_len: packet.data.payload_len,
                    included_fin: packet.data.included_fin,
                };
                self.retransmissions.push(retransmission);
            }
        }

        Ok(())
    }

    #[inline]
    fn on_transmit_retransmissions<E, Clk, A>(
        &mut self,
        packet_number: &packet_number::Counter,
        encrypt_key: &mut E,
        clock: &Clk,
        message: &mut A,
        send_quantum: &mut usize,
        mtu: u16,
    ) -> Result<(), Error>
    where
        E: encrypt::Key,
        Clk: Clock,
        A: Allocator<Segment = S, Retransmission = R>,
    {
        while let Some(retransmission) = self.retransmissions.peek() {
            ensure!(message.can_push(), break);
            if retransmission.ty.is_probe() {
                if self.pto.transmissions() == 0 {
                    let retrans = self
                        .retransmissions
                        .pop()
                        .expect("retransmission should be available");
                    message.free(retrans.segment);
                    continue;
                }
            } else {
                ensure!(!self.cca.is_congestion_limited(), break);
            }
            ensure!(*send_quantum >= mtu as usize, break);

            let segment_len = message.segment_len();
            let buffer = message.get_mut(retransmission);

            debug_assert!(!buffer.is_empty(), "empty retransmission buffer submitted");

            // make sure we have enough space in the current buffer for the payload
            ensure!(
                segment_len.map_or(true, |s| s as usize >= buffer.len()),
                break
            );

            let packet_number = match packet_number.next() {
                Ok(pn) => pn,
                Err(err) if message.is_empty() => return Err(err.into()),
                // if we've sent something wait until `on_transmit` gets called again to return an
                // error
                Err(_) => break,
            };

            {
                let buffer = DecoderBufferMut::new(buffer);
                match decoder::Packet::retransmit(buffer, packet_number, encrypt_key) {
                    Ok(info) => info,
                    Err(err) => {
                        let retransmission = self
                            .retransmissions
                            .pop()
                            .expect("retransmission should be available");
                        message.free(retransmission.segment);
                        debug_assert!(false, "{err:?}");
                        return Err(Error::RetransmissionFailure);
                    }
                }
            };

            let time_sent = clock.get_time();
            *send_quantum = send_quantum.saturating_sub(buffer.len());
            let packet_len = buffer.len() as u16;

            if branch!(message.is_empty()) {
                let ecn = self
                    .ecn
                    .ecn(s2n_quic_core::transmission::Mode::Normal, time_sent);
                message.set_ecn(ecn);
            }

            {
                let info = self
                    .retransmissions
                    .pop()
                    .expect("retransmission should be available");
                let stream_offset = info.stream_offset;
                let payload_len = info.payload_len;
                let ty = info.ty;
                let included_fin = info.included_fin;

                let retransmission = if ty.is_stream() && self.stream_id.is_reliable {
                    let segment = message.push_with_retransmission(info.segment);
                    Some(segment)
                } else {
                    message.push(info.segment);
                    None
                };

                let transmission = transmission::Info {
                    packet_len,
                    stream_offset,
                    payload_len,
                    included_fin,
                    retransmission,
                };

                self.on_transmit_segment(
                    packet_number,
                    time_sent,
                    transmission,
                    message.ecn(),
                    false,
                );

                if self.pto.transmissions() > 0 && ty.is_probe() {
                    self.pto.on_transmit_once();
                }

                #[cfg(debug_assertions)]
                self.on_transmit_offset(
                    encrypt_key,
                    packet_number,
                    stream_offset,
                    included_fin,
                    payload_len,
                    ty,
                    true,
                    clock,
                );
            }
        }

        Ok(())
    }

    #[inline]
    pub fn on_transmit_probe<E, Clk, A>(
        &mut self,
        packet_number: &packet_number::Counter,
        encrypt_key: &mut E,
        source_control_port: u16,
        source_stream_port: Option<u16>,
        clock: &Clk,
        message: &mut A,
        send_quantum: &mut usize,
        mtu: u16,
    ) -> Result<(), Error>
    where
        E: encrypt::Key,
        Clk: Clock,
        A: Allocator<Segment = S, Retransmission = R>,
    {
        while self.pto.transmissions() > 0 {
            ensure!(message.can_push(), break);
            // don't write a packet unless the segment len is the MTU
            if let Some(segment_len) = message.segment_len() {
                ensure!(segment_len == mtu, Ok(()));
            }

            let mut payload = worker::probe::Probe {
                offset: self.max_sent_offset,
                final_offset: None,
            };

            let packet_len = self.on_transmit_data_unchecked(
                packet_number,
                encrypt_key,
                source_control_port,
                source_stream_port,
                &mut payload,
                clock,
                message,
                mtu,
                TransmissionType::Probe,
            )?;

            ensure!(packet_len > 0, break);

            *send_quantum -= packet_len as usize;

            self.pto.on_transmit_once();
        }

        Ok(())
    }

    #[inline]
    pub fn on_transmit_data_unchecked<E, I, Clk, A>(
        &mut self,
        packet_number: &packet_number::Counter,
        encrypt_key: &mut E,
        source_control_port: u16,
        source_stream_port: Option<u16>,
        cleartext_payload: &mut I,
        clock: &Clk,
        message: &mut A,
        mtu: u16,
        ty: TransmissionType,
    ) -> Result<u16, Error>
    where
        E: encrypt::Key,
        I: s2n_quic_core::buffer::Reader<Error = core::convert::Infallible>,
        Clk: Clock,
        A: Allocator<Segment = S, Retransmission = R>,
    {
        // try to allocate a segment in the current buffer
        ensure!(let Some(segment) = message.alloc(), Ok(0));

        // try to get the next packet number
        ensure!(
            let Ok(packet_number) = packet_number.next(),
            return {
                message.free(segment);
                ensure!(!message.is_empty(), Err(Error::PacketNumberExhaustion));
                Ok(0)
            }
        );

        let buffer = message.get_mut(&segment);

        {
            let mtu = mtu as usize;

            // grow the buffer if needed
            if branch!(buffer.capacity() < mtu) {
                // We don't use `resize` here, since that will require initializing the bytes,
                // which can add up quickly. This is OK, though, since we're just writing into the
                // buffer and not actually reading anything.
                buffer.reserve(mtu - buffer.len());
            }

            unsafe {
                debug_assert!(buffer.capacity() >= mtu);
                buffer.set_len(mtu as _);
            }
        }

        self.checker.check_payload(cleartext_payload);

        let stream_offset = cleartext_payload.current_offset();
        let encoder = EncoderBuffer::new(buffer);
        let packet_len = encoder::encode(
            encoder,
            source_control_port,
            source_stream_port,
            self.stream_id,
            packet_number,
            self.next_expected_control_packet,
            VarInt::ZERO,
            &mut &[][..],
            VarInt::ZERO,
            &(),
            cleartext_payload,
            encrypt_key,
        )?;

        // no need to keep going if the output is empty
        ensure!(
            packet_len > 0,
            return {
                message.free(segment);
                Ok(0)
            }
        );

        let payload_len = (cleartext_payload.current_offset() - stream_offset)
            .try_into()
            .unwrap();

        let included_fin = cleartext_payload.final_offset().map_or(false, |fin| {
            stream_offset.as_u64() + payload_len as u64 == fin.as_u64()
        });

        buffer.truncate(packet_len);

        debug_assert!(
            packet_len < 1 << 16,
            "cannot write larger packets than 2^16"
        );
        let packet_len = packet_len as u16;

        let time_sent = clock.get_time();

        // get the current ECN marking for this batch on the first transmission
        if branch!(message.is_empty()) {
            let ecn = self
                .ecn
                .ecn(s2n_quic_core::transmission::Mode::Normal, time_sent);
            message.set_ecn(ecn);
        }

        {
            let has_more_app_data = branch!(cleartext_payload.buffered_len() > 0);

            let retransmission = if ty.is_stream() && self.stream_id.is_reliable {
                let segment = message.push_with_retransmission(segment);
                Some(segment)
            } else {
                message.push(segment);
                None
            };

            let transmission = transmission::Info {
                packet_len,
                stream_offset,
                payload_len,
                included_fin,
                retransmission,
            };

            self.on_transmit_segment(
                packet_number,
                time_sent,
                transmission,
                message.ecn(),
                has_more_app_data,
            );
        }

        Ok(packet_len)
    }

    #[inline]
    pub fn on_timeout<E, Clk, A>(
        &mut self,
        _encrypt_key: &mut E,
        clock: &Clk,
        message: &mut A,
    ) -> Result<(), Error>
    where
        E: encrypt::Key,
        Clk: Clock,
        A: Allocator<Segment = S, Retransmission = R>,
    {
        if self.state.is_ready() {
            self.arm_idle_timer(clock);
        } else if branch!(self.idle_timer.poll_expiration(clock.get_time()).is_ready()) {
            self.on_idle_timeout(message);
        }

        let packets_in_flight = !self.sent_packets.is_empty();
        if branch!(self
            .pto
            .on_timeout(packets_in_flight, clock.get_time())
            .is_ready())
        {
            // TODO where does this come from
            let max_pto_backoff = 1024;
            self.pto_backoff = self.pto_backoff.saturating_mul(2).min(max_pto_backoff);
        }

        Ok(())
    }

    #[inline]
    fn arm_idle_timer(&mut self, clock: &impl Clock) {
        // TODO make this configurable
        let idle_timeout = crate::stream::DEFAULT_IDLE_TIMEOUT;
        self.idle_timer.set(clock.get_time() + idle_timeout);
    }

    #[inline]
    fn on_idle_timeout<A>(&mut self, message: &mut A)
    where
        A: Allocator<Segment = S, Retransmission = R>,
    {
        // we don't want to transmit anything so enter a terminal state
        let mut did_transition = false;
        did_transition |= self.state.on_send_reset().is_ok();
        did_transition |= self.state.on_recv_reset_ack().is_ok();
        if did_transition {
            self.on_error(Error::IdleTimeout, message);
        }
    }

    #[inline]
    pub fn check_error(&self) -> Result<(), Error> {
        if let Some(err) = self.error {
            Err(err)
        } else {
            Ok(())
        }
    }

    #[inline]
    fn on_error<A>(&mut self, error: Error, message: &mut A)
    where
        A: Allocator<Segment = S, Retransmission = R>,
    {
        ensure!(self.error.is_none());
        self.error = Some(error);
        let _ = self.state.on_queue_reset();

        self.clean_up(message);
    }

    #[inline]
    fn clean_up<A>(&mut self, message: &mut A)
    where
        A: Allocator<Segment = S, Retransmission = R>,
    {
        // force clear message so we don't get panics
        message.force_clear();

        for retransmission in self.retransmissions.drain() {
            message.free(retransmission.segment);
        }
        let min = PacketNumberSpace::Initial.new_packet_number(VarInt::ZERO);
        let max = PacketNumberSpace::Initial.new_packet_number(VarInt::MAX);
        let range = s2n_quic_core::packet::number::PacketNumberRange::new(min, max);
        for (_pn, info) in self.sent_packets.remove_range(range) {
            if let Some(segment) = info.data.retransmission {
                message.free_retransmission(segment);
            }
        }

        self.idle_timer.cancel();
        self.pto.cancel();
        self.unacked_ranges.clear();

        self.invariants();
    }

    #[inline(always)]
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    fn on_transmit_offset(
        &mut self,
        encrypt_key: &mut impl encrypt::Key,
        packet_number: VarInt,
        stream_offset: VarInt,
        included_fin: bool,
        payload_len: u16,
        transmission_type: TransmissionType,
        is_retransmission: bool,
        _clock: &impl Clock,
    ) {
        let packet_number = s2n_quic_core::packet::number::PacketNumberSpace::Initial
            .new_packet_number(packet_number);
        let is_probe = matches!(transmission_type, TransmissionType::Probe);
        if is_probe {
            probes::on_transmit_probe(
                encrypt_key.credentials().id,
                self.stream_id,
                packet_number,
                stream_offset,
                payload_len,
                is_retransmission,
            );
        } else {
            probes::on_transmit_stream(
                encrypt_key.credentials().id,
                self.stream_id,
                packet_number,
                stream_offset,
                payload_len,
                is_retransmission,
            );
        }
        self.checker.on_stream_transmission(
            stream_offset,
            payload_len,
            is_retransmission,
            is_probe,
        );
        trace!(
            stream_id = ?self.stream_id,
            stream_offset = stream_offset.as_u64(),
            payload_len,
            included_fin,
            is_retransmission,
            is_probe = transmission_type.is_probe(),
        );
    }

    #[cfg(debug_assertions)]
    #[inline]
    fn invariants(&self) {
        // TODO
    }

    #[cfg(not(debug_assertions))]
    #[inline(always)]
    fn invariants(&self) {}
}

impl<S: Segment, R: Segment> timer::Provider for State<S, R> {
    #[inline]
    fn timers<Q: timer::Query>(&self, query: &mut Q) -> timer::Result {
        // if we're in a terminal state then no timers are needed
        ensure!(!self.state.is_terminal(), Ok(()));

        if branch!(matches!(self.state, state::Sender::Send)) {
            let mut can_transmit = !self.cca.is_congestion_limited();
            can_transmit |= self.cca.requires_fast_retransmission();
            can_transmit &= self.max_sent_offset < self.max_data;
            if can_transmit {
                self.cca.timers(query)?;
            }
        }
        self.pto.timers(query)?;
        self.idle_timer.timers(query)?;
        Ok(())
    }
}

#[cfg(debug_assertions)]
impl<S: Segment, R: Segment> Drop for State<S, R> {
    #[inline]
    fn drop(&mut self) {
        // ignore any checks for leaking segments since we're cleaning everything up
        for mut retransmission in self.retransmissions.drain() {
            retransmission.segment.leak();
        }
        let min = PacketNumberSpace::Initial.new_packet_number(VarInt::ZERO);
        let max = PacketNumberSpace::Initial.new_packet_number(VarInt::MAX);
        let range = s2n_quic_core::packet::number::PacketNumberRange::new(min, max);
        for (_pn, info) in self.sent_packets.remove_range(range) {
            if let Some(mut segment) = info.data.retransmission {
                segment.leak();
            }
        }
    }
}

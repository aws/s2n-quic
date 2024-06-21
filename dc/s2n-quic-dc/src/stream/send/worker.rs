// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    congestion,
    crypto::{decrypt, encrypt, UninitSlice},
    packet::{
        self,
        stream::{self, decoder, encoder},
    },
    recovery,
    stream::{
        processing,
        send::{
            application, buffer, error::Error, filter::Filter, probes,
            transmission::Type as TransmissionType,
        },
        DEFAULT_IDLE_TIMEOUT,
    },
};
use core::{task::Poll, time::Duration};
use s2n_codec::{DecoderBufferMut, EncoderBuffer};
use s2n_quic_core::{
    dc::ApplicationParams,
    ensure,
    frame::{self, FrameMut},
    inet::ExplicitCongestionNotification,
    interval_set::IntervalSet,
    packet::number::PacketNumberSpace,
    path::{ecn, INITIAL_PTO_BACKOFF},
    random, ready,
    recovery::{Pto, RttEstimator},
    stream::state,
    time::{
        timer::{self, Provider as _},
        Clock, Timer, Timestamp,
    },
    varint::VarInt,
};
use slotmap::SlotMap;
use std::collections::{BinaryHeap, VecDeque};
use tracing::{debug, trace};

pub mod probe;
pub mod retransmission;
pub mod transmission;

type PacketMap<Info> = s2n_quic_core::packet::number::Map<Info>;

pub type Transmission = application::transmission::Event<buffer::Segment>;

slotmap::new_key_type! {
    pub struct BufferIndex;
}

#[derive(Clone, Copy, Debug)]
pub enum TransmitIndex {
    Stream(BufferIndex),
    Recovery(BufferIndex),
}

#[derive(Debug)]
pub struct SentStreamPacket {
    info: transmission::Info<BufferIndex>,
    cc_info: congestion::PacketInfo,
}

#[derive(Debug)]
pub struct SentRecoveryPacket {
    info: transmission::Info<BufferIndex>,
    cc_info: congestion::PacketInfo,
    max_stream_packet_number: VarInt,
}

#[derive(Debug)]
pub struct Worker {
    pub stream_id: stream::Id,
    pub rtt_estimator: RttEstimator,
    pub sent_stream_packets: PacketMap<SentStreamPacket>,
    pub stream_packet_buffers: SlotMap<BufferIndex, buffer::Segment>,
    pub max_stream_packet_number: VarInt,
    pub sent_recovery_packets: PacketMap<SentRecoveryPacket>,
    pub recovery_packet_buffers: SlotMap<BufferIndex, Vec<u8>>,
    pub free_packet_buffers: Vec<Vec<u8>>,
    pub recovery_packet_number: u64,
    pub last_sent_recovery_packet: Option<Timestamp>,
    pub transmit_queue: VecDeque<TransmitIndex>,
    pub state: state::Sender,
    pub control_filter: Filter,
    pub retransmissions: BinaryHeap<retransmission::Segment<BufferIndex>>,
    pub next_expected_control_packet: VarInt,
    pub cca: congestion::Controller,
    pub ecn: ecn::Controller,
    pub pto: Pto,
    pub pto_backoff: u32,
    pub inflight_timer: Timer,
    pub idle_timer: Timer,
    pub idle_timeout: Duration,
    pub error: Option<Error>,
    pub unacked_ranges: IntervalSet<VarInt>,
    pub max_sent_offset: VarInt,
    pub max_data: VarInt,
    pub local_max_data_window: VarInt,
    pub peer_activity: Option<PeerActivity>,
    pub mtu: u16,
    pub max_sent_segment_size: u16,
}

#[derive(Clone, Copy, Debug)]
pub struct PeerActivity {
    pub newly_acked_packets: bool,
}

impl Worker {
    #[inline]
    pub fn new(stream_id: stream::Id, params: &ApplicationParams) -> Self {
        let mtu = params.max_datagram_size;
        let initial_max_data = params.remote_max_data;
        let local_max_data = params.local_send_max_data;

        // initialize the pending data left to send
        let mut unacked_ranges = IntervalSet::new();
        unacked_ranges.insert(VarInt::ZERO..=VarInt::MAX).unwrap();

        let cca = congestion::Controller::new(mtu);
        let max_sent_offset = VarInt::ZERO;

        Self {
            stream_id,
            next_expected_control_packet: VarInt::ZERO,
            rtt_estimator: recovery::rtt_estimator(),
            cca,
            sent_stream_packets: Default::default(),
            stream_packet_buffers: Default::default(),
            max_stream_packet_number: VarInt::ZERO,
            sent_recovery_packets: Default::default(),
            recovery_packet_buffers: Default::default(),
            recovery_packet_number: 0,
            last_sent_recovery_packet: None,
            free_packet_buffers: Default::default(),
            transmit_queue: Default::default(),
            control_filter: Default::default(),
            ecn: ecn::Controller::default(),
            state: Default::default(),
            retransmissions: Default::default(),
            pto: Pto::default(),
            pto_backoff: INITIAL_PTO_BACKOFF,
            inflight_timer: Default::default(),
            idle_timer: Default::default(),
            idle_timeout: params.max_idle_timeout.unwrap_or(DEFAULT_IDLE_TIMEOUT),
            error: None,
            unacked_ranges,
            max_sent_offset,
            max_data: initial_max_data,
            local_max_data_window: local_max_data,
            peer_activity: None,
            mtu,
            max_sent_segment_size: 0,
        }
    }

    /// Initializes the worker as a client
    #[inline]
    pub fn init_client(&mut self, clock: &impl Clock) {
        debug_assert!(self.state.is_ready());
        // make sure a packet gets sent soon if the application doesn't
        self.force_arm_pto_timer(clock);
    }

    /// Returns the current flow offset
    #[inline]
    pub fn flow_offset(&self) -> VarInt {
        let cca_offset = {
            let extra_window = self
                .cca
                .congestion_window()
                .saturating_sub(self.cca.bytes_in_flight());

            self.max_sent_offset + extra_window as usize
        };

        let local_offset = {
            let unacked_start = self.unacked_ranges.min_value().unwrap_or_default();
            let local_max_data_window = self.local_max_data_window;

            unacked_start.saturating_add(local_max_data_window)
        };

        let remote_offset = self.max_data;

        cca_offset.min(local_offset).min(remote_offset)
    }

    #[inline]
    pub fn send_quantum_packets(&self) -> u8 {
        // TODO use div_ceil when we're on 1.73+ MSRV
        // https://doc.rust-lang.org/std/primitive.u64.html#method.div_ceil
        let send_quantum = (self.cca.send_quantum() as u64 + self.mtu as u64 - 1) / self.mtu as u64;
        send_quantum.try_into().unwrap_or(u8::MAX)
    }

    /// Called by the worker when it receives a control packet from the peer
    #[inline]
    pub fn on_control_packet<D, Clk>(
        &mut self,
        decrypt_key: &D,
        ecn: ExplicitCongestionNotification,
        packet: &mut packet::control::decoder::Packet,
        random: &mut dyn random::Generator,
        clock: &Clk,
        transmission_queue: &application::transmission::Queue<buffer::Segment>,
        segment_alloc: &buffer::Allocator,
    ) -> Result<(), processing::Error>
    where
        D: decrypt::Key,
        Clk: Clock,
    {
        match self.on_control_packet_impl(
            decrypt_key,
            ecn,
            packet,
            random,
            clock,
            transmission_queue,
            segment_alloc,
        ) {
            Ok(None) => {}
            Ok(Some(error)) => return Err(error),
            Err(error) => {
                self.on_error(error);
            }
        }

        self.invariants();

        Ok(())
    }

    #[inline(always)]
    fn on_control_packet_impl<D, Clk>(
        &mut self,
        decrypt_key: &D,
        _ecn: ExplicitCongestionNotification,
        packet: &mut packet::control::decoder::Packet,
        random: &mut dyn random::Generator,
        clock: &Clk,
        transmission_queue: &application::transmission::Queue<buffer::Segment>,
        segment_alloc: &buffer::Allocator,
    ) -> Result<Option<processing::Error>, Error>
    where
        D: decrypt::Key,
        Clk: Clock,
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

        let recv_time = clock.get_time();
        let mut newly_acked = false;
        let mut max_acked_stream = None;
        let mut max_acked_recovery = None;
        let mut loaded_transmit_queue = false;

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
                        if !core::mem::replace(&mut loaded_transmit_queue, true) {
                            // make sure we have a current view of the application transmissions
                            self.load_transmission_queue(transmission_queue);
                        }

                        if ack.ecn_counts.is_some() {
                            self.on_frame_ack::<_, _, _, true>(
                                decrypt_key,
                                &ack,
                                random,
                                &recv_time,
                                &mut newly_acked,
                                &mut max_acked_stream,
                                &mut max_acked_recovery,
                                segment_alloc,
                            )?;
                        } else {
                            self.on_frame_ack::<_, _, _, false>(
                                decrypt_key,
                                &ack,
                                random,
                                &recv_time,
                                &mut newly_acked,
                                &mut max_acked_stream,
                                &mut max_acked_recovery,
                                segment_alloc,
                            )?;
                        }
                    }
                    FrameMut::MaxData(frame) => {
                        if self.max_data < frame.maximum_data {
                            self.max_data = frame.maximum_data;
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
                            self.clean_up();
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

        for (space, pn) in [
            (stream::PacketSpace::Stream, max_acked_stream),
            (stream::PacketSpace::Recovery, max_acked_recovery),
        ] {
            if let Some(pn) = pn {
                self.detect_lost_packets(decrypt_key, random, &recv_time, space, pn)?;
            }
        }

        self.on_peer_activity(newly_acked);

        // try to transition to the final state if we've sent all of the data
        if self.unacked_ranges.is_empty()
            && self.error.is_none()
            && self.state.on_recv_all_acks().is_ok()
        {
            self.clean_up();
            // transmit one more PTO packet so we can ACK the peer's
            // CONNECTION_CLOSE frame and they can shutdown quickly. Otherwise,
            // they'll need to hang around to respond to potential loss.
            self.pto.force_transmit();
        }

        Ok(None)
    }

    #[inline]
    fn on_frame_ack<D, Ack, Clk, const IS_STREAM: bool>(
        &mut self,
        decrypt_key: &D,
        ack: &frame::Ack<Ack>,
        random: &mut dyn random::Generator,
        clock: &Clk,
        newly_acked: &mut bool,
        max_acked_stream: &mut Option<VarInt>,
        max_acked_recovery: &mut Option<VarInt>,
        segment_alloc: &buffer::Allocator,
    ) -> Result<(), Error>
    where
        D: decrypt::Key,
        Ack: frame::ack::AckRanges,
        Clk: Clock,
    {
        let mut cca_args = None;
        let mut bytes_acked = 0;

        macro_rules! impl_ack_processing {
            ($space:ident, $sent_packets:ident, $on_packet_number:expr) => {
                for range in ack.ack_ranges() {
                    let pmin = PacketNumberSpace::Initial.new_packet_number(*range.start());
                    let pmax = PacketNumberSpace::Initial.new_packet_number(*range.end());
                    let range = s2n_quic_core::packet::number::PacketNumberRange::new(pmin, pmax);
                    for (num, packet) in self.$sent_packets.remove_range(range) {
                        let num_varint = unsafe { VarInt::new_unchecked(num.as_u64()) };

                        #[allow(clippy::redundant_closure_call)]
                        ($on_packet_number)(num_varint, &packet);

                        let _ = self.unacked_ranges.remove(packet.info.tracking_range());

                        self.ecn
                            .on_packet_ack(packet.info.time_sent, packet.info.ecn);
                        bytes_acked += packet.info.cca_len() as usize;

                        // record the most recent packet
                        if cca_args
                            .as_ref()
                            .map_or(true, |prev: &(Timestamp, _)| prev.0 < packet.info.time_sent)
                        {
                            cca_args = Some((packet.info.time_sent, packet.cc_info));
                        }

                        // free the retransmission segment
                        if let Some(segment) = packet.info.retransmission {
                            if let Some(segment) = self.stream_packet_buffers.remove(segment) {
                                // push the segment so the application can reuse it
                                if segment.capacity() >= self.max_sent_segment_size as usize {
                                    segment_alloc.free(segment);
                                }
                            }
                        }

                        probes::on_packet_ack(
                            decrypt_key.credentials().id,
                            self.stream_id,
                            stream::PacketSpace::$space,
                            num.as_u64(),
                            packet.info.packet_len,
                            packet.info.stream_offset,
                            packet.info.payload_len,
                            clock
                                .get_time()
                                .saturating_duration_since(packet.info.time_sent),
                        );

                        *newly_acked = true;
                    }
                }
            };
        }

        if IS_STREAM {
            impl_ack_processing!(
                Stream,
                sent_stream_packets,
                |packet_number: VarInt, _packet: &SentStreamPacket| {
                    *max_acked_stream = (*max_acked_stream).max(Some(packet_number));
                }
            );
        } else {
            impl_ack_processing!(
                Recovery,
                sent_recovery_packets,
                |packet_number: VarInt, sent_packet: &SentRecoveryPacket| {
                    *max_acked_recovery = (*max_acked_recovery).max(Some(packet_number));
                    *max_acked_stream =
                        (*max_acked_stream).max(Some(sent_packet.max_stream_packet_number));

                    // increase the max stream packet if this was a probe
                    if sent_packet.info.retransmission.is_none() {
                        self.max_stream_packet_number = self
                            .max_stream_packet_number
                            .max(sent_packet.max_stream_packet_number + 1);
                    }
                }
            );
        };

        if let Some((time_sent, cc_info)) = cca_args {
            let rtt_sample = clock.get_time().saturating_duration_since(time_sent);

            self.rtt_estimator.update_rtt(
                ack.ack_delay(),
                rtt_sample,
                clock.get_time(),
                true,
                PacketNumberSpace::ApplicationData,
            );

            self.cca.on_packet_ack(
                cc_info.first_sent_time,
                bytes_acked,
                cc_info,
                &self.rtt_estimator,
                random,
                clock.get_time(),
            );
        }

        Ok(())
    }

    #[inline]
    fn detect_lost_packets<D, Clk>(
        &mut self,
        decrypt_key: &D,
        random: &mut dyn random::Generator,
        clock: &Clk,
        packet_space: stream::PacketSpace,
        max: VarInt,
    ) -> Result<(), Error>
    where
        D: decrypt::Key,
        Clk: Clock,
    {
        let Some(loss_threshold) = max.checked_sub(VarInt::from_u8(2)) else {
            return Ok(());
        };

        let mut is_unrecoverable = false;

        macro_rules! impl_loss_detection {
            ($sent_packets:ident, $on_packet:expr) => {{
                let lost_min = PacketNumberSpace::Initial.new_packet_number(VarInt::ZERO);
                let lost_max = PacketNumberSpace::Initial.new_packet_number(loss_threshold);
                let range = s2n_quic_core::packet::number::PacketNumberRange::new(lost_min, lost_max);
                for (num, packet) in self.$sent_packets.remove_range(range) {
                    // TODO create a path and publisher
                    // self.ecn.on_packet_loss(packet.time_sent, packet.ecn, now, path, publisher);

                    self.cca.on_packet_lost(
                        packet.info.cca_len() as _,
                        packet.cc_info,
                        random,
                        clock.get_time(),
                    );

                    probes::on_packet_lost(
                        decrypt_key.credentials().id,
                        self.stream_id,
                        packet_space,
                        num.as_u64(),
                        packet.info.packet_len,
                        packet.info.stream_offset,
                        packet.info.payload_len,
                        clock
                            .get_time()
                            .saturating_duration_since(packet.info.time_sent),
                        packet.info.retransmission.is_some(),
                    );

                    #[allow(clippy::redundant_closure_call)]
                    ($on_packet)(&packet);

                    if let Some(segment) = packet.info.retransmission {
                        // update our local packet number to be at least 1 more than the largest lost
                        // packet number
                        let min_recovery_packet_number = num.as_u64() + 1;
                        self.recovery_packet_number =
                            self.recovery_packet_number.max(min_recovery_packet_number);

                        let retransmission = retransmission::Segment {
                            segment,
                            stream_offset: packet.info.stream_offset,
                            payload_len: packet.info.payload_len,
                            ty: TransmissionType::Stream,
                            included_fin: packet.info.included_fin,
                        };
                        self.retransmissions.push(retransmission);
                    } else {
                        // we can only recover reliable streams
                        is_unrecoverable |= packet.info.payload_len > 0 && !self.stream_id.is_reliable;
                    }
                }}
            }
        }

        match packet_space {
            stream::PacketSpace::Stream => impl_loss_detection!(sent_stream_packets, |_| {}),
            stream::PacketSpace::Recovery => {
                impl_loss_detection!(sent_recovery_packets, |sent_packet: &SentRecoveryPacket| {
                    self.max_stream_packet_number = self
                        .max_stream_packet_number
                        .max(sent_packet.max_stream_packet_number + 1);
                })
            }
        }

        ensure!(!is_unrecoverable, Err(Error::RetransmissionFailure));

        self.invariants();

        Ok(())
    }

    #[inline]
    fn on_peer_activity(&mut self, newly_acked_packets: bool) {
        if let Some(prev) = self.peer_activity.as_mut() {
            prev.newly_acked_packets |= newly_acked_packets;
        } else {
            self.peer_activity = Some(PeerActivity {
                newly_acked_packets,
            });
        }
    }

    #[inline]
    pub fn before_sleep<Clk: Clock>(&mut self, clock: &Clk) {
        self.process_peer_activity();

        // make sure our timers are armed
        self.update_idle_timer(clock);
        self.update_inflight_timer(clock);
        self.update_pto_timer(clock);

        trace!(
            unacked_ranges = ?self.unacked_ranges,
            retransmissions = self.retransmissions.len(),
            stream_packets_in_flight = self.sent_stream_packets.iter().count(),
            recovery_packets_in_flight = self.sent_recovery_packets.iter().count(),
            pto_timer = ?self.pto.next_expiration(),
            inflight_timer = ?self.inflight_timer.next_expiration(),
            idle_timer = ?self.idle_timer.next_expiration(),
        );
    }

    #[inline]
    fn process_peer_activity(&mut self) {
        let Some(PeerActivity {
            newly_acked_packets,
        }) = self.peer_activity.take()
        else {
            return;
        };

        if newly_acked_packets {
            self.reset_pto_timer();
        }

        // force probing when we've sent all of the data but haven't got an ACK for the final
        // offset
        if self.state.is_data_sent() && self.stream_packet_buffers.is_empty() {
            self.pto.force_transmit();
        }

        // re-arm the idle timer as long as we're not in terminal state
        if !self.state.is_terminal() {
            self.idle_timer.cancel();
            self.inflight_timer.cancel();
        }
    }

    #[inline]
    pub fn on_time_update<Clk, Ld>(&mut self, clock: &Clk, load_last_activity: Ld)
    where
        Clk: Clock,
        Ld: FnOnce() -> Timestamp,
    {
        if self.poll_idle_timer(clock, load_last_activity).is_ready() {
            self.on_error(Error::IdleTimeout);
            // we don't actually want to send any packets on idle timeout
            let _ = self.state.on_send_reset();
            let _ = self.state.on_recv_reset_ack();
            return;
        }

        if self
            .inflight_timer
            .poll_expiration(clock.get_time())
            .is_ready()
        {
            self.on_error(Error::IdleTimeout);
            return;
        }

        if self
            .pto
            .on_timeout(self.has_inflight_packets(), clock.get_time())
            .is_ready()
        {
            // TODO where does this come from
            let max_pto_backoff = 1024;
            self.pto_backoff = self.pto_backoff.saturating_mul(2).min(max_pto_backoff);
        }
    }

    #[inline]
    fn poll_idle_timer<Clk, Ld>(&mut self, clock: &Clk, load_last_activity: Ld) -> Poll<()>
    where
        Clk: Clock,
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
    fn has_inflight_packets(&self) -> bool {
        !self.sent_stream_packets.is_empty()
            || !self.sent_recovery_packets.is_empty()
            || !self.retransmissions.is_empty()
            || !self.transmit_queue.is_empty()
    }

    #[inline]
    fn update_idle_timer(&mut self, clock: &impl Clock) {
        ensure!(!self.idle_timer.is_armed());

        let now = clock.get_time();
        self.idle_timer.set(now + self.idle_timeout);
    }

    #[inline]
    fn update_inflight_timer(&mut self, clock: &impl Clock) {
        // TODO make this configurable
        let inflight_timeout = crate::stream::DEFAULT_INFLIGHT_TIMEOUT;

        if self.has_inflight_packets() {
            if !self.inflight_timer.is_armed() {
                self.inflight_timer.set(clock.get_time() + inflight_timeout);
            }
        } else {
            self.inflight_timer.cancel();
        }
    }

    #[inline]
    fn update_pto_timer(&mut self, clock: &impl Clock) {
        ensure!(!self.pto.is_armed());

        let mut should_arm = self.has_inflight_packets();

        // if we have stream packet buffers in flight then arm the PTO
        should_arm |= !self.stream_packet_buffers.is_empty();

        // if we've sent all of the data/reset and are waiting to clean things up
        should_arm |= self.state.is_data_sent() || self.state.is_reset_sent();

        ensure!(should_arm);

        self.force_arm_pto_timer(clock);
    }

    #[inline]
    fn force_arm_pto_timer(&mut self, clock: &impl Clock) {
        let pto_period = self
            .rtt_estimator
            .pto_period(self.pto_backoff, PacketNumberSpace::Initial);
        self.pto.update(clock.get_time(), pto_period);
    }

    #[inline]
    fn reset_pto_timer(&mut self) {
        self.pto_backoff = INITIAL_PTO_BACKOFF;
        self.pto.cancel();
    }

    /// Called by the worker thread when it becomes aware of the application having transmitted a
    /// segment
    #[inline]
    pub fn load_transmission_queue(
        &mut self,
        queue: &application::transmission::Queue<buffer::Segment>,
    ) -> bool {
        let mut did_transmit_stream = false;

        for Transmission {
            packet_number,
            info,
            has_more_app_data,
        } in queue.drain()
        {
            self.max_sent_segment_size = self.max_sent_segment_size.max(info.packet_len);
            let info = info.map(|buffer| self.stream_packet_buffers.insert(buffer));
            self.on_transmit_segment(
                stream::PacketSpace::Stream,
                packet_number,
                info,
                has_more_app_data,
            );
            did_transmit_stream = true;
        }

        if did_transmit_stream {
            // if we just sent some packets then we can use those as probes
            self.reset_pto_timer();
        }

        self.invariants();

        did_transmit_stream
    }

    #[inline]
    fn on_transmit_segment(
        &mut self,
        packet_space: stream::PacketSpace,
        packet_number: VarInt,
        info: transmission::Info<BufferIndex>,
        has_more_app_data: bool,
    ) {
        // the BBR implementation requires monotonic time so track that
        let mut cca_time_sent = info.time_sent;

        match packet_space {
            stream::PacketSpace::Stream => {
                if let Some(min) = self.last_sent_recovery_packet {
                    cca_time_sent = info.time_sent.max(min);
                }
            }
            stream::PacketSpace::Recovery => {
                self.last_sent_recovery_packet = Some(info.time_sent);
            }
        }

        let cc_info = self.cca.on_packet_sent(
            cca_time_sent,
            info.cca_len(),
            has_more_app_data,
            &self.rtt_estimator,
        );

        // update the max offset that we've transmitted
        self.max_sent_offset = self.max_sent_offset.max(info.end_offset());

        // try to transition to start sending
        let _ = self.state.on_send_stream();
        if info.included_fin {
            // if the transmission included the final offset, then transition to that state
            let _ = self.state.on_send_fin();
        }

        if let stream::PacketSpace::Recovery = packet_space {
            let packet_number = PacketNumberSpace::Initial.new_packet_number(packet_number);
            let max_stream_packet_number = self.max_stream_packet_number;
            self.sent_recovery_packets.insert(
                packet_number,
                SentRecoveryPacket {
                    info,
                    cc_info,
                    max_stream_packet_number,
                },
            );
        } else {
            self.max_stream_packet_number = self.max_stream_packet_number.max(packet_number);
            let packet_number = PacketNumberSpace::Initial.new_packet_number(packet_number);
            self.sent_stream_packets
                .insert(packet_number, SentStreamPacket { info, cc_info });
        }
    }

    #[inline]
    pub fn fill_transmit_queue<E, Clk>(
        &mut self,
        encrypt_key: &E,
        source_control_port: u16,
        clock: &Clk,
    ) -> Result<(), Error>
    where
        E: encrypt::Key,
        Clk: Clock,
    {
        if let Err(error) = self.fill_transmit_queue_impl(encrypt_key, source_control_port, clock) {
            self.on_error(error);
            return Err(error);
        }

        Ok(())
    }

    #[inline]
    fn fill_transmit_queue_impl<E, Clk>(
        &mut self,
        encrypt_key: &E,
        source_control_port: u16,
        clock: &Clk,
    ) -> Result<(), Error>
    where
        E: encrypt::Key,
        Clk: Clock,
    {
        // skip a packet number if we're probing
        if self.pto.transmissions() > 0 {
            self.recovery_packet_number += 1;
        }

        self.try_transmit_retransmissions(encrypt_key, clock)?;
        self.try_transmit_probe(encrypt_key, source_control_port, clock)?;

        Ok(())
    }

    #[inline]
    fn try_transmit_retransmissions<E, Clk>(
        &mut self,
        encrypt_key: &E,
        clock: &Clk,
    ) -> Result<(), Error>
    where
        E: encrypt::Key,
        Clk: Clock,
    {
        // We'll only have retransmissions if we're reliable
        ensure!(self.stream_id.is_reliable, Ok(()));

        while let Some(retransmission) = self.retransmissions.peek() {
            // make sure we fit in the current congestion window
            let remaining_cca_window = self
                .cca
                .congestion_window()
                .saturating_sub(self.cca.bytes_in_flight());
            ensure!(
                retransmission.payload_len as u32 <= remaining_cca_window,
                break
            );

            let buffer = self.stream_packet_buffers[retransmission.segment].make_mut();

            debug_assert!(!buffer.is_empty(), "empty retransmission buffer submitted");

            let packet_number =
                VarInt::new(self.recovery_packet_number).expect("2^62 is a lot of packets");
            self.recovery_packet_number += 1;

            {
                let buffer = DecoderBufferMut::new(buffer);
                match decoder::Packet::retransmit(
                    buffer,
                    stream::PacketSpace::Recovery,
                    packet_number,
                    encrypt_key,
                ) {
                    Ok(info) => info,
                    Err(err) => {
                        // this shouldn't ever happen
                        debug_assert!(false, "{err:?}");
                        return Err(Error::RetransmissionFailure);
                    }
                }
            };

            let time_sent = clock.get_time();
            let packet_len = buffer.len() as u16;

            {
                let info = self
                    .retransmissions
                    .pop()
                    .expect("retransmission should be available");

                // enqueue the transmission
                self.transmit_queue
                    .push_back(TransmitIndex::Stream(info.segment));

                let stream_offset = info.stream_offset;
                let payload_len = info.payload_len;
                let included_fin = info.included_fin;
                let retransmission = Some(info.segment);

                // TODO store this as part of the packet queue
                let ecn = ExplicitCongestionNotification::Ect0;

                let info = transmission::Info {
                    packet_len,
                    stream_offset,
                    payload_len,
                    included_fin,
                    retransmission,
                    time_sent,
                    ecn,
                };

                probes::on_transmit_stream(
                    encrypt_key.credentials().id,
                    self.stream_id,
                    stream::PacketSpace::Recovery,
                    PacketNumberSpace::Initial.new_packet_number(packet_number),
                    stream_offset,
                    payload_len,
                    included_fin,
                    true,
                );

                self.on_transmit_segment(stream::PacketSpace::Recovery, packet_number, info, false);

                // consider this transmission a probe if needed
                if self.pto.transmissions() > 0 {
                    self.pto.on_transmit_once();
                }
            }
        }

        Ok(())
    }

    #[inline]
    pub fn try_transmit_probe<E, Clk>(
        &mut self,
        encrypt_key: &E,
        source_control_port: u16,
        clock: &Clk,
    ) -> Result<(), Error>
    where
        E: encrypt::Key,
        Clk: Clock,
    {
        while self.pto.transmissions() > 0 {
            // probes are not congestion-controlled

            let packet_number =
                VarInt::new(self.recovery_packet_number).expect("2^62 is a lot of packets");
            self.recovery_packet_number += 1;

            let mut buffer = self.free_packet_buffers.pop().unwrap_or_default();

            // resize the buffer to what we need
            {
                let min_len = stream::encoder::MAX_RETRANSMISSION_HEADER_LEN + 128;

                if buffer.capacity() < min_len {
                    buffer.reserve(min_len - buffer.len());
                }

                unsafe {
                    debug_assert!(buffer.capacity() >= min_len);
                    buffer.set_len(min_len);
                }
            }

            let offset = self.max_sent_offset;
            let final_offset = if self.state.is_data_sent() {
                Some(offset)
            } else {
                None
            };

            let mut payload = probe::Probe {
                offset,
                final_offset,
            };

            let encoder = EncoderBuffer::new(&mut buffer);
            let packet_len = encoder::encode(
                encoder,
                source_control_port,
                None,
                self.stream_id,
                stream::PacketSpace::Recovery,
                packet_number,
                self.next_expected_control_packet,
                VarInt::ZERO,
                &mut &[][..],
                VarInt::ZERO,
                &(),
                &mut payload,
                encrypt_key,
            );

            let payload_len = 0;
            let included_fin = final_offset.is_some();
            buffer.truncate(packet_len);

            debug_assert!(
                packet_len < u16::MAX as usize,
                "cannot write larger packets than 2^16"
            );
            let packet_len = packet_len as u16;

            let time_sent = clock.get_time();

            // TODO store this as part of the packet queue
            let ecn = ExplicitCongestionNotification::Ect0;

            // enqueue the transmission
            let buffer_index = self.recovery_packet_buffers.insert(buffer);
            self.transmit_queue
                .push_back(TransmitIndex::Recovery(buffer_index));

            let info = transmission::Info {
                packet_len,
                stream_offset: offset,
                payload_len,
                included_fin,
                retransmission: None, // PTO packets are not retransmitted
                time_sent,
                ecn,
            };

            self.on_transmit_segment(stream::PacketSpace::Recovery, packet_number, info, false);

            self.pto.on_transmit_once();
        }

        Ok(())
    }

    #[inline]
    pub fn transmit_queue_iter<Clk: Clock>(
        &mut self,
        clock: &Clk,
    ) -> impl Iterator<Item = (ExplicitCongestionNotification, &[u8])> + '_ {
        let ecn = self
            .ecn
            .ecn(s2n_quic_core::transmission::Mode::Normal, clock.get_time());
        let stream_packet_buffers = &self.stream_packet_buffers;
        let recovery_packet_buffers = &self.recovery_packet_buffers;

        self.transmit_queue.iter().filter_map(move |index| {
            let buf = match *index {
                TransmitIndex::Stream(index) => stream_packet_buffers.get(index)?.as_slice(),
                TransmitIndex::Recovery(index) => recovery_packet_buffers.get(index)?,
            };

            Some((ecn, buf))
        })
    }

    #[inline]
    pub fn on_transmit_queue(&mut self, count: usize) {
        for transmission in self.transmit_queue.drain(..count) {
            match transmission {
                TransmitIndex::Stream(index) => {
                    // make sure the packet wasn't freed between when we wanted to transmit and
                    // when we actually did
                    ensure!(self.stream_packet_buffers.get(index).is_some(), continue);
                }
                TransmitIndex::Recovery(index) => {
                    // make sure the packet wasn't freed between when we wanted to transmit and
                    // when we actually did
                    let Some(mut buffer) = self.recovery_packet_buffers.remove(index) else {
                        continue;
                    };
                    buffer.clear();
                    self.free_packet_buffers.push(buffer);
                }
            };
        }
    }

    #[inline]
    pub fn on_error(&mut self, error: Error) {
        ensure!(self.error.is_none());
        self.error = Some(error);
        let _ = self.state.on_queue_reset();

        self.clean_up();
    }

    #[inline]
    fn clean_up(&mut self) {
        self.retransmissions.clear();
        let min = PacketNumberSpace::Initial.new_packet_number(VarInt::ZERO);
        let max = PacketNumberSpace::Initial.new_packet_number(VarInt::MAX);
        let range = s2n_quic_core::packet::number::PacketNumberRange::new(min, max);
        let _ = self.sent_stream_packets.remove_range(range);
        let _ = self.sent_recovery_packets.remove_range(range);

        self.idle_timer.cancel();
        self.inflight_timer.cancel();
        self.pto.cancel();
        self.unacked_ranges.clear();

        self.transmit_queue.clear();
        for buffer in self.stream_packet_buffers.drain() {
            // TODO push buffer into free segment queue
            let _ = buffer;
        }
        for (_idx, mut buffer) in self.recovery_packet_buffers.drain() {
            buffer.clear();
            self.free_packet_buffers.push(buffer);
        }

        self.invariants();
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

impl timer::Provider for Worker {
    #[inline]
    fn timers<Q: timer::Query>(&self, query: &mut Q) -> timer::Result {
        // if we're in a terminal state then no timers are needed
        ensure!(!self.state.is_terminal(), Ok(()));
        self.pto.timers(query)?;
        self.idle_timer.timers(query)?;
        Ok(())
    }
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    congestion,
    credentials::Credentials,
    crypto, event,
    packet::{
        self,
        stream::{self, decoder, encoder, PacketSpace},
    },
    recovery,
    stream::{
        error::{self, Error},
        processing,
        send::{
            application::state::{Message, PushError},
            filter::Filter,
        },
        shared::Half,
        DEFAULT_IDLE_TIMEOUT,
    },
};
use core::{task::Poll, time::Duration};
use s2n_codec::{DecoderBufferMut, EncoderBuffer, EncoderValue};
use s2n_quic_core::{
    dc::ApplicationParams,
    endpoint::Location,
    ensure,
    event::IntoEvent as _,
    frame::{self, FrameMut},
    inet::ExplicitCongestionNotification,
    interval_set::IntervalSet,
    packet::number::PacketNumberSpace,
    path::{ecn, INITIAL_PTO_BACKOFF},
    random,
    recovery::{bandwidth::Bandwidth, Pto, RttEstimator},
    stream::state,
    time::{
        timer::{self, Provider as _},
        Clock, Timer, Timestamp,
    },
    varint::VarInt,
};
use std::collections::BinaryHeap;
use tracing::trace;

mod buffer_budget;
mod fin;
mod keep_alive;
mod max_data;
mod probe;
mod progress;
mod reset;
pub mod retransmission;
pub mod transmission;

type PacketMap<Info> = s2n_quic_core::packet::number::Map<Info>;

#[derive(Debug)]
pub struct SentStreamPacket {
    info: transmission::Info,
    cc_info: congestion::PacketInfo,
}

#[derive(Debug)]
pub struct SentRecoveryPacket {
    info: transmission::Info,
    cc_info: congestion::PacketInfo,
    max_stream_packet_number_lost: VarInt,
}

#[derive(Clone, Debug, Default)]
pub struct InflightCounters {
    pub probes: u32,
    pub with_payload: u32,
    pub with_final_offset: u32,
    pub with_reset: u32,
}

impl InflightCounters {
    pub fn on_transmit(&mut self, info: &transmission::Info) {
        self.update(info, |count| *count += 1);
    }

    pub fn on_finish(&mut self, info: &transmission::Info) {
        self.update(info, |count| *count -= 1);
    }

    fn update(&mut self, info: &transmission::Info, mut update: impl FnMut(&mut u32)) {
        if info.is_probe() {
            update(&mut self.probes);
        }

        if info.payload_len > 0 {
            update(&mut self.with_payload);
        }

        if info.flags.included_final_offset() {
            update(&mut self.with_final_offset);
        }

        if info.flags.included_reset() {
            update(&mut self.with_reset);
        }
    }

    pub fn has_inflight_packets(
        &self,
        unacked_ranges: &IntervalSet<VarInt>,
        fin: &fin::Fin,
        reset: &reset::Reset,
    ) -> bool {
        let mut has_inflight_packets = !unacked_ranges.is_empty() && self.with_payload > 0;
        has_inflight_packets |= !fin.is_acked() && self.with_final_offset > 0;
        has_inflight_packets |= reset.waiting_ack() && self.with_reset > 0;
        has_inflight_packets
    }
}

/// Allow up to 4 GSO segments in flight
const MAX_TX_OFFSET: VarInt = VarInt::from_u32(4 * u16::MAX as u32);

#[derive(Debug)]
pub struct State {
    rtt_estimator: RttEstimator,
    sent_stream_packets: PacketMap<SentStreamPacket>,
    max_stream_packet_number: VarInt,
    max_stream_packet_number_lost: VarInt,
    sent_recovery_packets: PacketMap<SentRecoveryPacket>,
    recovery_packet_number: u64,
    control_filter: Filter,
    next_expected_control_packet: VarInt,
    cca: congestion::Controller,
    ecn: ecn::Controller,
    pto: Pto,
    pto_backoff: u32,
    counters: InflightCounters,
    idle_timer: Timer,
    idle_timeout: Duration,
    reset: reset::Reset,
    unacked_ranges: IntervalSet<VarInt>,
    max_data: max_data::MaxData,
    max_tx_offset: VarInt,
    buffer_budget: buffer_budget::BufferBudget,
    peer_activity: Option<PeerActivity>,
    max_datagram_size: u16,
    max_sent_segment_size: u16,
    is_reliable: bool,
    retransmission_bytes_in_flight: u32,
    fin: fin::Fin,
    keep_alive: keep_alive::KeepAlive,
    retransmissions: BinaryHeap<retransmission::Segment>,
    /// Tracks the minimum unacked offset to detect lack of forward progress.
    /// If this value doesn't advance within `idle_timeout`, the stream is
    /// considered stuck and will be terminated. This acts as a safety net
    /// against retransmission amplification loops.
    progress: progress::Progress,
    #[cfg(debug_assertions)]
    pending_retransmissions: IntervalSet<VarInt>,
}

#[derive(Clone, Copy, Debug)]
pub struct PeerActivity {
    pub made_progress: bool,
}

impl State {
    #[inline]
    pub fn new(stream_id: stream::Id, params: &ApplicationParams) -> Self {
        let max_datagram_size = params.max_datagram_size();
        let initial_max_data = params.remote_max_data;
        let local_max_data = params.local_send_max_data;

        // initialize the pending data left to send
        let mut unacked_ranges = IntervalSet::new();
        unacked_ranges.insert(VarInt::ZERO..=VarInt::MAX).unwrap();

        let cca = congestion::Controller::new(max_datagram_size);

        let max_tx_offset = MAX_TX_OFFSET;

        Self {
            next_expected_control_packet: VarInt::ZERO,
            rtt_estimator: recovery::rtt_estimator(),
            cca,
            sent_stream_packets: Default::default(),
            max_stream_packet_number: VarInt::ZERO,
            max_stream_packet_number_lost: VarInt::ZERO,
            sent_recovery_packets: Default::default(),
            recovery_packet_number: 0,
            control_filter: Default::default(),
            ecn: ecn::Controller::default(),
            pto: Pto::default(),
            pto_backoff: INITIAL_PTO_BACKOFF,
            counters: Default::default(),
            idle_timer: Default::default(),
            idle_timeout: params.max_idle_timeout().unwrap_or(DEFAULT_IDLE_TIMEOUT),
            reset: Default::default(),
            unacked_ranges,
            max_data: max_data::MaxData::new(initial_max_data),
            max_tx_offset,
            buffer_budget: buffer_budget::BufferBudget::new(local_max_data, max_datagram_size),
            peer_activity: None,
            max_datagram_size,
            max_sent_segment_size: 0,
            is_reliable: stream_id.is_reliable,
            retransmission_bytes_in_flight: 0,
            fin: Default::default(),
            retransmissions: Default::default(),
            keep_alive: Default::default(),
            progress: Default::default(),
            #[cfg(debug_assertions)]
            pending_retransmissions: Default::default(),
        }
    }

    /// Initializes the worker as a client
    #[inline]
    pub fn init_client(&mut self, clock: &impl Clock) {
        // make sure a packet gets sent soon if the application doesn't
        self.force_arm_pto_timer(clock);
        self.update_idle_timer(clock);
    }

    #[inline]
    pub fn init_server(&mut self, clock: &impl Clock) {
        self.update_idle_timer(clock);
    }

    /// Returns the current flow offset
    #[inline]
    pub fn flow_offset(&self) -> VarInt {
        self.max_tx_offset
            .min(self.max_data.max_data())
            .min(self.local_offset())
            .min(self.cca_offset())
    }

    pub fn state(&self) -> state::Sender {
        if let Some(state) = self.reset.state() {
            return state;
        }

        if self.unacked_ranges.is_empty() {
            return if self.fin.is_acked() {
                state::Sender::DataRecvd
            } else {
                state::Sender::DataSent
            };
        }

        state::Sender::Send
    }

    const BBR: bool = false;
    const WINDOW: u32 = 625_000_000 * 2;

    fn cca_offset(&self) -> VarInt {
        let extra_window = if Self::BBR {
            let mut extra_window = self
                .cca
                .congestion_window()
                .saturating_sub(self.cca.bytes_in_flight());

            // only give CCA credits to the application if we were able to retransmit everything considered lost
            if !self.retransmissions.is_empty() {
                extra_window = 0;
            }

            extra_window
        } else {
            Self::WINDOW - self.cca.bytes_in_flight()
        };

        self.max_data.max_sent_offset() + extra_window as usize
    }

    fn local_offset(&self) -> VarInt {
        // Use the dynamic BDP-based window instead of a fixed-size buffer.
        // This computes how much buffer we actually need to sustain the current
        // pacing rate (pacing rate × RTT), clamped to a configured max.
        // When many streams share a packet sender that caps the aggregate rate,
        // each stream's observed bandwidth drops and its buffer shrinks
        // proportionally — coordinating memory usage without explicit signaling.
        let window = self
            .buffer_budget
            .window(self.bandwidth(), &self.rtt_estimator);
        let remaining_window = window.saturating_sub(self.cca.bytes_in_flight() as u64);
        self.max_data
            .max_sent_offset()
            .saturating_add(VarInt::try_from(remaining_window).unwrap_or(VarInt::MAX))
    }

    #[inline]
    pub fn send_quantum_packets(&self) -> u8 {
        let send_quantum = (self.cca.send_quantum() as u64).div_ceil(self.max_datagram_size as u64);
        send_quantum.try_into().unwrap_or(u8::MAX)
    }

    pub fn bandwidth(&self) -> Bandwidth {
        if Self::BBR {
            self.cca.bandwidth()
        } else {
            Bandwidth::new(Self::WINDOW as _, core::time::Duration::from_secs(1))
        }
    }

    pub fn keep_alive(&mut self, enabled: bool, clock: &impl Clock) {
        let target = self
            .idle_timer
            .next_expiration()
            .unwrap_or_else(|| clock.get_time());
        self.keep_alive.set(enabled, target, self.idle_timeout);
    }

    /// Called by the worker when it receives a control packet from the peer
    #[inline]
    pub fn on_control_packet<C, Clk, Aso, Pub>(
        &mut self,
        control_key: &C,
        ecn: ExplicitCongestionNotification,
        packet: &mut packet::control::decoder::Packet<&mut [u8]>,
        random: &mut dyn random::Generator,
        clock: &Clk,
        transmission_queue: &transmission::Queue,
        app_stream_offset: &Aso,
        publisher: &Pub,
    ) -> Result<(), processing::Error>
    where
        C: crypto::open::control::Stream,
        Clk: Clock,
        Aso: Fn() -> VarInt,
        Pub: event::ConnectionPublisher,
    {
        match self.on_control_packet_impl(
            control_key,
            ecn,
            packet,
            random,
            clock,
            transmission_queue,
            app_stream_offset,
            publisher,
        ) {
            Ok(None) => {}
            Ok(Some(error)) => return Err(error),
            Err(error) => {
                self.on_error(error, Location::Local, clock, publisher);
            }
        }

        Ok(())
    }

    #[inline(always)]
    fn on_control_packet_impl<C, Clk, Aso, Pub>(
        &mut self,
        control_key: &C,
        _ecn: ExplicitCongestionNotification,
        packet: &mut packet::control::decoder::Packet<&mut [u8]>,
        random: &mut dyn random::Generator,
        clock: &Clk,
        transmission_queue: &transmission::Queue,
        app_stream_offset: &Aso,
        publisher: &Pub,
    ) -> Result<Option<processing::Error>, Error>
    where
        C: crypto::open::control::Stream,
        Clk: Clock,
        Aso: Fn() -> VarInt,
        Pub: event::ConnectionPublisher,
    {
        // only process the packet after we know it's authentic
        let res = control_key.verify(packet.header(), packet.auth_tag());

        publisher.on_stream_control_packet_received(event::builder::StreamControlPacketReceived {
            packet_number: packet.packet_number().as_u64(),
            packet_len: packet.total_len(),
            control_data_len: packet.control_data().len(),
            is_authenticated: res.is_ok(),
        });

        // drop the packet if it failed to authenticate
        if let Err(err) = res {
            return Ok(Some(err.into()));
        }

        // check if we've already seen the packet
        ensure!(
            self.control_filter.on_packet(packet).is_ok(),
            Ok(Some(processing::Error::Duplicate))
        );

        let packet_number = packet.packet_number();

        // raise our next expected control packet
        {
            let old = self.next_expected_control_packet;
            let pn = packet_number.saturating_add(VarInt::from_u8(1));
            let pn = self.next_expected_control_packet.max(pn);
            self.next_expected_control_packet = pn;
            if pn != old {
                tracing::debug!(
                    old_next_expected = old.as_u64(),
                    new_next_expected = pn.as_u64(),
                    control_packet_number = packet_number.as_u64(),
                    "Updated next_expected_control_packet after receiving control packet"
                );
            }
        }

        let recv_time = clock.get_time();
        let mut made_progress = false;
        let mut max_acked_stream = None;
        let mut max_acked_recovery = None;
        let mut max_acked_tx_time = None;
        let mut loaded_transmit_queue = false;

        for frame in packet.control_frames_mut() {
            let frame = frame.map_err(|err| error::Kind::FrameError { decoder: err }.err())?;

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
                        // Lazily load the app stream offset to give the application
                        // maximum time to advance it before we check.
                        let offset = app_stream_offset();
                        // make sure we have a current view of the application transmissions
                        self.load_completion_queue(transmission_queue, clock, offset);
                    }

                    if ack.ecn_counts.is_some() {
                        self.on_frame_ack::<_, _, _, true>(
                            &ack,
                            random,
                            &recv_time,
                            &mut made_progress,
                            &mut max_acked_stream,
                            &mut max_acked_recovery,
                            &mut max_acked_tx_time,
                            publisher,
                        )?;
                    } else {
                        self.on_frame_ack::<_, _, _, false>(
                            &ack,
                            random,
                            &recv_time,
                            &mut made_progress,
                            &mut max_acked_stream,
                            &mut max_acked_recovery,
                            &mut max_acked_tx_time,
                            publisher,
                        )?;
                    }
                }
                FrameMut::MaxData(frame) => {
                    if let Some(diff) = self.max_data.on_max_data_frame(frame.maximum_data) {
                        publisher.on_stream_max_data_received(
                            event::builder::StreamMaxDataReceived {
                                increase: diff.as_u64(),
                                new_max_data: frame.maximum_data.as_u64(),
                            },
                        );
                    }
                }
                FrameMut::ConnectionClose(close) => {
                    let error = if close.frame_type.is_some() {
                        error::Kind::TransportError {
                            code: close.error_code,
                        }
                    } else {
                        error::Kind::from_connection_close(&close)
                    };

                    let error = error.err();
                    self.on_error(error, Location::Remote, clock, publisher);
                    return Err(error);
                }
                _ => continue,
            }
        }

        // If the ACK processing put us into a terminal state, clean up
        // any stale retransmissions and cancel the PTO timer so we don't
        // wastefully send packets for data that's already been acknowledged.
        if self.state().is_data_received() {
            self.on_success();
            return Ok(None);
        }

        // Perform loss detection across both packet number spaces using the max ACK'd TX time
        // This ensures we use a consistent loss window based on the most recently acknowledged packet
        if let Some(max_tx_time) = max_acked_tx_time {
            // Time threshold: loss_delay = max(kTimeThreshold * max(smoothed_rtt, latest_rtt), kGranularity)
            let loss_delay = {
                let rtt = self
                    .rtt_estimator
                    .smoothed_rtt()
                    .max(self.rtt_estimator.latest_rtt());
                // kTimeThreshold is typically 9/8 per RFC
                let time_threshold = rtt + rtt / 8;
                // kGranularity is typically 1ms
                time_threshold.max(Duration::from_millis(1))
            };

            let loss_time = max_tx_time.checked_sub(loss_delay);

            for (space, pn) in [
                (stream::PacketSpace::Stream, max_acked_stream),
                (stream::PacketSpace::Recovery, max_acked_recovery),
            ] {
                self.detect_lost_packets(random, &recv_time, space, pn, loss_time, publisher)?;
            }
        }

        // Notify the forward progress tracker when the minimum unacked offset advances
        if let Some(min_unacked) = self.unacked_ranges.min_value() {
            self.progress.on_progress(min_unacked);
        }
        self.on_peer_activity(made_progress);

        Ok(None)
    }

    pub fn on_fin_known(&mut self, final_offset: VarInt) {
        ensure!(self.fin.on_known(final_offset).is_ok());
        self.unacked_ranges
            .remove(final_offset..=VarInt::MAX)
            .unwrap();
        self.keep_alive.on_fin_known();

        trace!(%final_offset, ?self.unacked_ranges, "fin known");
    }

    pub fn next_expected_control_packet(&self) -> VarInt {
        self.next_expected_control_packet
    }

    pub fn max_datagram_size(&self) -> u16 {
        self.max_datagram_size
    }

    pub fn error(&self) -> Option<(&error::Error, Location)> {
        self.reset.error()
    }

    #[inline]
    fn on_frame_ack<Ack, Clk, Pub, const IS_STREAM: bool>(
        &mut self,
        ack: &frame::Ack<Ack>,
        random: &mut dyn random::Generator,
        clock: &Clk,
        made_progress: &mut bool,
        max_acked_stream: &mut Option<VarInt>,
        max_acked_recovery: &mut Option<VarInt>,
        max_acked_tx_time: &mut Option<Timestamp>,
        publisher: &Pub,
    ) -> Result<(), Error>
    where
        Ack: frame::ack::AckRanges,
        Clk: Clock,
        Pub: event::ConnectionPublisher,
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

                        // Track the max ACK'd TX time across all packet spaces for loss detection
                        *max_acked_tx_time = (*max_acked_tx_time).max(Some(packet.info.time_sent));

                        self.counters.on_finish(&packet.info);

                        // If we got an ACK for a packet that included the final offset then notify the fin state
                        if packet.info.flags.included_final_offset() {
                            self.fin.on_ack();
                        }

                        if packet.info.flags.included_reset() {
                            self.reset.on_ack();
                        }

                        if !packet.info.is_probe() {
                            *made_progress = true;
                        }

                        publisher.on_stream_packet_acked(event::builder::StreamPacketAcked {
                            packet_len: packet.info.packet_len as usize,
                            stream_offset: packet.info.stream_offset.as_u64(),
                            payload_len: packet.info.payload_len as usize,
                            packet_number: num.as_u64(),
                            time_sent: packet.info.time_sent.into_event(),
                            lifetime: clock
                                .get_time()
                                .saturating_duration_since(packet.info.time_sent),
                            is_retransmission: PacketSpace::$space.is_recovery()
                                && !packet.info.is_probe(),
                        });
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
                    // When a recovery packet is ACKed, the data it carried has
                    // been delivered. Use the stored max_stream_packet_number_lost
                    // to raise max_acked_stream — this is the stream PN up to
                    // which packets were considered lost when the recovery was
                    // created. This lets the PN threshold correctly detect
                    // stream-space loss without using the recovery PN directly
                    // (which would be far too high).
                    *max_acked_stream =
                        (*max_acked_stream).max(Some(sent_packet.max_stream_packet_number_lost));
                }
            );
        };

        if let Some((time_sent, cc_info)) = cca_args {
            let now = clock.get_time();
            let ack_delay = ack.ack_delay();
            let rtt_sample = now
                .saturating_duration_since(time_sent)
                .saturating_sub(ack_delay)
                .max(Duration::from_micros(1));

            self.rtt_estimator.update_rtt(
                Duration::ZERO,
                rtt_sample,
                now,
                true,
                PacketNumberSpace::ApplicationData,
            );

            self.cca.on_packet_ack(
                cc_info.first_sent_time,
                bytes_acked,
                cc_info,
                &self.rtt_estimator,
                random,
                now,
            );
        }

        Ok(())
    }

    #[inline]
    fn detect_lost_packets<Clk, Pub>(
        &mut self,
        random: &mut dyn random::Generator,
        clock: &Clk,
        packet_space: stream::PacketSpace,
        max_acked_pn: Option<VarInt>,
        loss_time: Option<Timestamp>,
        publisher: &Pub,
    ) -> Result<(), Error>
    where
        Clk: Clock,
        Pub: event::ConnectionPublisher,
    {
        // Packet number threshold
        let pn_threshold = max_acked_pn.and_then(|max_pn| max_pn.checked_sub(VarInt::from_u8(3)));

        let is_unrecoverable = false;

        macro_rules! impl_loss_detection {
            ($sent_packets:ident, $on_packet:expr) => {{
                let lost_min = PacketNumberSpace::Initial.new_packet_number(VarInt::ZERO);
                let mut lost_max = None;

                for (num, packet) in self.$sent_packets.iter() {
                    // A packet is considered lost if it meets either condition:
                    // 1. Time threshold: sent before loss_time
                    // 2. Packet number threshold: packet number <= max_acked_pn - 3
                    let lost_by_time = loss_time.map_or(false, |loss_time| packet.info.time_sent <= loss_time);
                    let lost_by_pn = pn_threshold
                        .map_or(false, |threshold| num.as_u64() <= threshold.as_u64());

                    if lost_by_time || lost_by_pn {
                        lost_max = Some(num);
                        continue;
                    }

                    break;
                }

                let Some(lost_max) = lost_max else {
                    return Ok(());
                };

                let range = s2n_quic_core::packet::number::PacketNumberRange::new(lost_min, lost_max);
                for (num, mut packet) in self.$sent_packets.remove_range(range) {
                    let num_varint = unsafe { VarInt::new_unchecked(num.as_u64()) };
                    // TODO create a path and publisher
                    // self.ecn.on_packet_loss(packet.time_sent, packet.ecn, now, path, publisher);

                    let now = clock.get_time();

                    self.cca.on_packet_lost(
                        packet.info.cca_len() as _,
                        packet.cc_info,
                        random,
                        now,
                    );

                    publisher.on_stream_packet_lost(event::builder::StreamPacketLost {
                        packet_len: packet.info.packet_len as _,
                        stream_offset: packet.info.stream_offset.as_u64(),
                        payload_len: packet.info.payload_len as _,
                        packet_number: num.as_u64(),
                        time_sent: packet.info.time_sent.into_event(),
                        lifetime: now.saturating_duration_since(packet.info.time_sent),
                        is_retransmission: packet_space.is_recovery() && !packet.info.is_probe(),
                    });

                    #[allow(clippy::redundant_closure_call)]
                    ($on_packet)(num_varint, &packet);

                    self.counters.on_finish(&packet.info);

                    // don't retransmit if the range is already ACK'd by another packet
                    // (e.g. a recovery packet carried the same data)
                    ensure!(
                        packet.info.payload_len > 0 &&
                        self.unacked_ranges.contains(&packet.info.stream_offset),
                        continue
                    );

                    if let Some(retransmission) = packet.info.try_retransmit() {
                        // update our local packet number to be at least 1 more than the largest lost
                        // packet number
                        let min_recovery_packet_number = num.as_u64() + 1;
                        self.recovery_packet_number =
                            self.recovery_packet_number.max(min_recovery_packet_number);

                        self.retransmissions.push(retransmission);
                    } else {
                        // TODO how do we know if the retransmission is in-flight or not?
                    }
                }
            }}
        }

        match packet_space {
            stream::PacketSpace::Stream => {
                impl_loss_detection!(sent_stream_packets, |packet_number: VarInt, _packet| {
                    // Track the highest stream PN actually declared lost
                    self.max_stream_packet_number_lost =
                        self.max_stream_packet_number_lost.max(packet_number);
                })
            }
            stream::PacketSpace::Recovery => {
                impl_loss_detection!(
                    sent_recovery_packets,
                    |_packet_number: VarInt, sent_packet: &SentRecoveryPacket| {
                        self.max_stream_packet_number_lost = self
                            .max_stream_packet_number_lost
                            .max(sent_packet.max_stream_packet_number_lost + 1);
                    }
                )
            }
        }

        ensure!(
            !is_unrecoverable,
            Err(error::Kind::RetransmissionFailure.err())
        );

        Ok(())
    }

    #[inline]
    fn on_peer_activity(&mut self, made_progress: bool) {
        if let Some(prev) = self.peer_activity.as_mut() {
            prev.made_progress |= made_progress;
        } else {
            self.peer_activity = Some(PeerActivity { made_progress });
        }
    }

    #[inline]
    pub fn before_sleep<Clk: Clock>(&mut self, clock: &Clk) {
        self.process_peer_activity();

        // make sure our timers are armed
        self.update_idle_timer(clock);
        self.update_pto_timer(clock);

        // Update the forward progress watchdog timer
        let has_inflight_payload =
            self.counters.with_payload > 0 && !self.unacked_ranges.is_empty();
        let is_flow_blocked = self.max_data.is_blocked();
        self.progress.update(
            clock,
            self.idle_timeout,
            has_inflight_payload,
            is_flow_blocked,
        );

        if self.has_inflight_packets() {
            debug_assert!(self.pto.is_armed());
        }

        if self.unacked_ranges.is_empty() && self.fin.is_acked() {
            debug_assert!(self.state().is_terminal());
        }

        trace!(
            unacked_ranges = ?self.unacked_ranges,
            retransmissions = self.retransmissions.len(),
            stream_packets_in_flight = self.sent_stream_packets.iter().count(),
            recovery_packets_in_flight = self.sent_recovery_packets.iter().count(),
            pto_timer = ?self.pto.next_expiration(),
            idle_timer = ?self.idle_timer.next_expiration(),
            ?self.counters,
            state = ?self.state(),
            ?self.fin,
        );

        self.invariants();
    }

    #[inline]
    fn process_peer_activity(&mut self) {
        let Some(PeerActivity { made_progress }) = self.peer_activity.take() else {
            return;
        };

        // If we're in a reset state then let the reset component know there was activity
        if !self.reset.is_idle() {
            self.reset.on_peer_activity();
            return;
        }

        // If the peer is making progress then reset our PTO backoff. Otherwise, we could
        // get caught in a loop.
        if made_progress || self.max_data.is_blocked() {
            self.reset_pto_timer();
        }

        // re-arm the idle timer as long as we're not in terminal state
        let state = self.state();
        if state.is_ready() || state.is_sending() {
            self.idle_timer.cancel();
        }
    }

    #[inline]
    pub fn on_time_update<Clk, Ld, Pub>(
        &mut self,
        clock: &Clk,
        load_last_activity: Ld,
        publisher: &Pub,
    ) where
        Clk: Clock,
        Ld: Fn() -> Timestamp,
        Pub: event::ConnectionPublisher,
    {
        if self.poll_idle_timer(clock, load_last_activity).is_ready() {
            self.on_error(error::Kind::IdleTimeout, Location::Local, clock, publisher);
            return;
        }

        let packets_in_flight = self.has_inflight_packets();

        if self
            .pto
            .on_timeout(packets_in_flight, clock.get_time())
            .is_ready()
        {
            // Re-queue the reset for retransmission if we're waiting for an
            // ACK to a CONNECTION_CLOSE. PTO exponential backoff throttles
            // the retransmission rate.
            self.reset.on_pto_timeout();

            // On the first PTO, only send 1 probe instead of 2. This reduces
            // wasted retransmissions for small streams where a single packet
            // carries all the data. On subsequent PTOs we keep 2 for resilience.
            if self.pto_backoff == INITIAL_PTO_BACKOFF && self.pto.transmissions() > 1 {
                self.pto.on_transmit_once();
            }

            // Cap the PTO backoff to prevent multi-second retransmission
            // delays. In a datacenter environment, the receiver throttles
            // draining-state ACKs to once per second. If PTO backoff grows
            // too large, the sender's retransmission cadence exceeds the
            // receiver's ACK rate, creating a feedback deadlock where the
            // sender waits longer than the ACK throttle window. A cap of 16
            // keeps the maximum PTO period at ~32ms (16 × ~2ms base), which
            // is well within the receiver's feedback window.
            let max_pto_backoff = 16;
            self.pto_backoff = self.pto_backoff.saturating_mul(2).min(max_pto_backoff);
        }

        let _ = self.max_data.on_timeout(clock, self.idle_timeout);
        let _ = self.keep_alive.on_timeout(clock, self.idle_timeout);

        // Check if the forward progress watchdog has expired
        if self.progress.on_timeout(clock.get_time()) {
            self.on_error(error::Kind::StreamStuck, Location::Local, clock, publisher);
        }
    }

    #[inline]
    fn poll_idle_timer<Clk, Ld>(&mut self, clock: &Clk, load_last_activity: Ld) -> Poll<()>
    where
        Clk: Clock,
        Ld: Fn() -> Timestamp,
    {
        let now = clock.get_time();

        for i in 0..2 {
            if let Some(expiration) = self.idle_timer.next_expiration() {
                if !expiration.has_elapsed(now) {
                    return Poll::Pending;
                }
                self.idle_timer.cancel();
                if i > 0 {
                    break;
                }
            }

            // if that expired then load the last activity from the peer and update the idle timer with
            // the value
            let last_peer_activity = load_last_activity();
            self.update_idle_timer(&last_peer_activity);

            // If timer still isn't armed after update attempt, it means we're in a state
            // where the timer shouldn't run (e.g., error state, terminal state)
            if self.idle_timer.next_expiration().is_none() {
                return Poll::Pending;
            }
        }

        Poll::Ready(())
    }

    #[inline]
    fn update_idle_timer(&mut self, clock: &impl Clock) {
        ensure!(!self.idle_timer.is_armed());
        // Don't re-arm the idle timer if we're in an error state - the error
        // timeout was set in on_error and shouldn't be overwritten
        ensure!(self.error().is_none());
        let state = self.state();
        ensure!(state.is_ready() || state.is_sending());

        let now = clock.get_time();
        let target = now + self.idle_timeout;
        self.idle_timer.set(target);
        self.keep_alive
            .on_idle_timer_update(target, self.idle_timeout);
    }

    #[inline]
    fn update_pto_timer(&mut self, clock: &impl Clock) {
        ensure!(!self.pto.is_armed());

        if self.has_inflight_packets() {
            self.force_arm_pto_timer(clock);
        }
    }

    fn has_inflight_packets(&self) -> bool {
        // If we're in a reset state that's waiting for an ACK, we have
        // inflight packets (the CONNECTION_CLOSE) that need PTO.
        if self.reset.waiting_ack() {
            return true;
        }

        if !self.reset.is_idle() {
            return false;
        }

        let mut has_inflight =
            self.counters
                .has_inflight_packets(&self.unacked_ranges, &self.fin, &self.reset);
        // If we're blocked on flow control we also need to keep the PTO armed
        has_inflight |= self.max_data.is_inflight();
        // The FIN also needs to be tracked — if it was sent but not yet ACKed,
        // we need PTO to probe for it even if the I/O counter is zero.
        has_inflight |= !self.fin.is_acked() && self.fin.value().is_some();
        has_inflight
    }

    #[inline]
    fn force_arm_pto_timer(&mut self, clock: &impl Clock) {
        let mut pto_period = self
            .rtt_estimator
            .pto_period(self.pto_backoff, PacketNumberSpace::Initial);

        // the `Timestamp::elapsed` function rounds up to the nearest 1ms so we need to set a min value
        // otherwise we'll prematurely trigger a PTO
        pto_period = pto_period.max(Duration::from_millis(2));

        self.pto.update(clock.get_time(), pto_period);
    }

    #[inline]
    fn reset_pto_timer(&mut self) {
        self.pto_backoff = INITIAL_PTO_BACKOFF;
        self.pto.cancel();
        // Drain any pending PTO transmissions so we don't generate spurious
        // retransmission probes after the backoff has been reset. Without this,
        // a previously-fired PTO leaves `transmissions > 0` even after cancel(),
        // causing `fill_transmit_queue_impl` to deep-copy in-flight packets as
        // PTO probes — duplicating data that loss detection has already queued
        // for retransmission and creating an amplification loop.
        while self.pto.transmissions() > 0 {
            self.pto.on_transmit_once();
        }
    }

    /// Called by the worker thread when it becomes aware of the application having transmitted a
    /// segment
    #[inline]
    pub fn load_completion_queue(
        &mut self,
        queue: &transmission::Queue,
        clock: &impl Clock,
        app_stream_offset: VarInt,
    ) {
        let is_terminal = self.state().is_terminal();

        let mut should_reset_pto = false;

        queue.drain_completion_queue(|transmission| {
            if is_terminal {
                // If we're in a terminal state, don't process completions.
                // Completions may arrive after clear_inflight_state() has been called (e.g., from on_error()),
                // and processing them would insert packets into empty maps while counters remain at zero,
                // causing invariant violations.
                return;
            }

            let (packet_number, mut info) = transmission.info;
            let transmission_time: Timestamp = transmission.transmission_time.into();

            // Use the actual transmission time rather than when it was submitted to give better RTT estimates
            debug_assert!(
                transmission_time.has_elapsed(clock.get_time()),
                "{} >= {}",
                clock.get_time(),
                transmission_time
            );
            info.time_sent = transmission_time;

            let meta = transmission.meta;

            // Determine whether the application has more data beyond this packet.
            //
            // We check several signals:
            // 1. Probes are never app-limited — they're sent because we're blocked
            //    on ACKs (CWND/flow-control limited), not because the app is idle.
            // 2. The app's stream_offset is ahead of this packet's end → more data
            //    is queued in the pipeline between the app and the worker.
            // 3. We know the final offset (from a large write with FIN) and this
            //    packet hasn't reached it → there's definitely more data to send.
            let has_more_app_data = info.flags.is_probe()
                || app_stream_offset > info.end_offset()
                || meta.final_offset.is_some_and(|fin| info.end_offset() < fin);
            self.max_sent_segment_size = self.max_sent_segment_size.max(info.packet_len);

            // Check if we need to update the fin state
            if let Some(final_offset) = meta.final_offset {
                self.on_fin_known(final_offset);
                self.fin.on_transmit();
            }

            // Store the buffer so we can retransmit if lost
            if info.payload_len > 0 {
                info.descriptor = Some(transmission.segment);
            }

            if meta.packet_space.is_stream() {
                should_reset_pto = true;
            }

            #[cfg(debug_assertions)]
            tracing::trace!(
                ?meta.packet_space,
                %packet_number,
                ?info,
                "transmission_complete"
            );

            self.on_transmit_segment(
                meta.packet_space,
                packet_number,
                info,
                has_more_app_data,
                clock,
            );
        });

        if should_reset_pto {
            // if we just sent some packets then we can use those as probes
            self.reset_pto_timer();
        }
    }

    #[inline]
    fn on_transmit_segment(
        &mut self,
        packet_space: stream::PacketSpace,
        packet_number: VarInt,
        info: transmission::Info,
        has_more_app_data: bool,
        clock: &impl Clock,
    ) {
        // create tracking state for the CCA
        let cc_info = self.cca.on_packet_sent(
            info.time_sent,
            info.cca_len(),
            has_more_app_data,
            &self.rtt_estimator,
        );

        // update the max offset that we've transmitted
        self.max_data.on_transmit(
            info.end_offset(),
            clock,
            self.idle_timeout,
            self.max_datagram_size,
        );
        self.counters.on_transmit(&info);

        if let stream::PacketSpace::Recovery = packet_space {
            let packet_number = PacketNumberSpace::Initial.new_packet_number(packet_number);
            // Capture the highest lost stream PN at recovery creation time.
            // This is already updated by detect_lost_packets when stream packets are lost.
            let max_stream_packet_number_lost = self.max_stream_packet_number_lost;

            #[cfg(debug_assertions)]
            let _ = self.pending_retransmissions.remove(info.range());

            if cfg!(debug_assertions)
                && !self.sent_recovery_packets.is_empty()
                && self
                    .sent_recovery_packets
                    .get_range()
                    .max()
                    .is_some_and(|v| v >= packet_number)
            {
                panic!("application packet numbers should be transmitted in order {packet_number:?}: {info:?} - {:?}", self.sent_recovery_packets);
            }

            // Decrement retransmission bytes in flight now that it's been sent
            self.retransmission_bytes_in_flight = self
                .retransmission_bytes_in_flight
                .saturating_sub(info.payload_len as u32);

            self.sent_recovery_packets.insert(
                packet_number,
                SentRecoveryPacket {
                    info,
                    cc_info,
                    max_stream_packet_number_lost,
                },
            );
        } else {
            if packet_number == VarInt::ZERO {
                debug_assert_eq!(packet_number, self.max_stream_packet_number);
            } else {
                debug_assert_eq!(
                    packet_number,
                    self.max_stream_packet_number + 1,
                    "application packet numbers should be transmitted in order {info:?}"
                );
            }
            self.max_stream_packet_number = self.max_stream_packet_number.max(packet_number);

            self.max_stream_packet_number_lost = self
                .max_stream_packet_number
                .max(self.max_stream_packet_number_lost);

            self.max_tx_offset = self.max_tx_offset.max(info.end_offset() + MAX_TX_OFFSET);
            self.recovery_packet_number = self
                .recovery_packet_number
                .max(self.max_stream_packet_number.as_u64() + 1);

            let packet_number = PacketNumberSpace::Initial.new_packet_number(packet_number);
            self.sent_stream_packets
                .insert(packet_number, SentStreamPacket { info, cc_info });
        }
    }

    /// Takes the oldest stream packets and tries to make them into PTO packets
    ///
    /// This ensures that we're not wasting resources by sending empty payloads, especially
    /// when there's outstanding data waiting to be ACK'd.
    fn make_stream_packets_as_pto_probes(&mut self) {
        // only reliable streams store segments
        ensure!(self.is_reliable);
        // check to see if we have in-flight segments in either packet space
        ensure!(!self.sent_stream_packets.is_empty() || !self.sent_recovery_packets.is_empty());

        let pto = self.pto.transmissions() as usize;

        // check if we already have retransmissions scheduled
        let Some(mut remaining) = pto.checked_sub(self.retransmissions.len()) else {
            return;
        };

        // iterate until remaining is 0.
        //
        // This nested loop is a bit weird but it's intentional - if we have `remaining == 2`
        // but only have a single in-flight segment then we want to transmit that segment
        // `remaining` times.
        //
        // We try stream packets first, then fall back to recovery packets. This
        // ensures that when all original stream packets have been ACKed but
        // recovery packets carrying unacked data are still in flight, we can
        // still produce PTO probes with actual payload rather than empty probes.
        while remaining > 0 {
            let mut made_progress = false;

            // First try stream packets
            for (num, packet) in self.sent_stream_packets.iter().take(remaining) {
                let Some(retransmission) = packet.info.retransmit_copy() else {
                    break;
                };

                let min_recovery_packet_number = num.as_u64() + 1;
                self.recovery_packet_number =
                    self.recovery_packet_number.max(min_recovery_packet_number);

                self.retransmissions.push(retransmission);
                remaining -= 1;
                made_progress = true;
            }

            if remaining == 0 {
                break;
            }

            // Then try recovery packets that have stored descriptors.
            // Don't use `.take(remaining)` here — probe entries (payload_len=0,
            // no descriptor) would consume the budget without producing
            // retransmissions, causing us to miss data-carrying packets deeper
            // in the map.
            for (num, packet) in self.sent_recovery_packets.iter() {
                if packet.info.payload_len > 0
                    && !self.unacked_ranges.contains(&packet.info.stream_offset)
                {
                    continue;
                }

                let Some(retransmission) = packet.info.retransmit_copy() else {
                    continue;
                };

                let min_recovery_packet_number = num.as_u64() + 1;
                self.recovery_packet_number =
                    self.recovery_packet_number.max(min_recovery_packet_number);

                self.retransmissions.push(retransmission);
                remaining -= 1;
                made_progress = true;

                if remaining == 0 {
                    break;
                }
            }

            if !made_progress {
                break;
            }
        }
    }

    fn requires_transmission(&self) -> bool {
        ensure!(!self.state().is_terminal(), false);

        let pto = self.pto.transmissions() > 0;
        let fin = self.fin.is_queued();
        let reset = self.reset.is_queued();
        let max_data = self.max_data.is_queued();
        let keep_alive = self.keep_alive.is_queued();

        let enabled = pto | fin | reset | max_data | keep_alive;

        if enabled {
            trace!(
                pto,
                fin,
                reset,
                max_data,
                keep_alive,
                "requires_transmission"
            );
        }

        enabled
    }

    #[inline]
    pub fn fill_transmit_queue<C, Clk, M, Pub>(
        &mut self,
        control_key: &C,
        credentials: &Credentials,
        stream_id: &stream::Id,
        source_queue_id: Option<VarInt>,
        clock: &Clk,
        packets: &mut M,
        publisher: &Pub,
    ) -> Result<(), Error>
    where
        C: crypto::seal::control::Stream,
        Clk: Clock,
        M: Message,
        Pub: event::ConnectionPublisher,
    {
        if let Err(error) = self.fill_transmit_queue_impl(
            control_key,
            credentials,
            stream_id,
            source_queue_id,
            clock,
            packets,
            publisher,
        ) {
            self.on_error(error, Location::Local, clock, publisher);
            return Err(error);
        }

        Ok(())
    }

    #[inline]
    fn fill_transmit_queue_impl<C, Clk, M, Pub>(
        &mut self,
        control_key: &C,
        credentials: &Credentials,
        stream_id: &stream::Id,
        source_queue_id: Option<VarInt>,
        clock: &Clk,
        packets: &mut M,
        publisher: &Pub,
    ) -> Result<(), Error>
    where
        C: crypto::seal::control::Stream,
        Clk: Clock,
        M: Message,
        Pub: event::ConnectionPublisher,
    {
        self.process_peer_activity();

        // skip a packet number if we're probing
        if self.pto.transmissions() > 0 {
            self.recovery_packet_number =
                (self.recovery_packet_number + 1).max(*self.max_stream_packet_number + 1);

            // On PTO, proactively retransmit in-flight stream packets rather than
            // sending empty probes. This is especially beneficial for small streams
            // where the entire payload fits in a single packet - the data arrives
            // immediately instead of requiring an extra round trip for loss detection.
            self.make_stream_packets_as_pto_probes();
        }

        self.try_transmit_retransmissions(control_key, clock, packets, publisher)?;
        self.try_transmit_probe(
            control_key,
            credentials,
            stream_id,
            source_queue_id,
            packets,
            clock,
            publisher,
        )?;

        Ok(())
    }

    #[inline]
    fn try_transmit_retransmissions<C, Clk, M, Pub>(
        &mut self,
        control_key: &C,
        clock: &Clk,
        packets: &mut M,
        publisher: &Pub,
    ) -> Result<(), Error>
    where
        C: crypto::seal::control::Stream,
        Clk: Clock,
        M: Message,
        Pub: event::ConnectionPublisher,
    {
        // We'll only have retransmissions if we're reliable
        ensure!(self.is_reliable, Ok(()));

        while let Some(retransmission) = self.retransmissions.peek() {
            // Skip retransmissions whose data has already been ACKed. This can
            // happen when PTO probes are created via `retransmit_copy()` and the
            // original data is ACKed before the copy is transmitted. Without this
            // check, stale entries would be needlessly transmitted only to be
            // immediately ACKed by the receiver.
            if retransmission.payload_len > 0
                && !self.unacked_ranges.contains(&retransmission.stream_offset)
            {
                self.retransmissions.pop();
                continue;
            }

            // Limit retransmission bytes in flight to avoid flooding the send wheel.
            // Allow up to 5x max_datagram_size worth of retransmissions in the wheel.
            let max_retransmission_bytes = self.max_datagram_size as u32 * 5;
            ensure!(
                self.retransmission_bytes_in_flight + retransmission.payload_len as u32
                    <= max_retransmission_bytes,
                break
            );

            // If the CCA is requesting fast retransmission we can bypass the CWND check
            if !self.cca.requires_fast_retransmission() {
                // make sure we fit in the current congestion window
                let remaining_cca_window = self
                    .cca
                    .congestion_window()
                    .saturating_sub(self.cca.bytes_in_flight());
                ensure!(
                    retransmission.payload_len as u32 <= remaining_cca_window,
                    break
                );
            }

            let mut info = self
                .retransmissions
                .pop()
                .expect("retransmission should be available");

            let packet_number =
                VarInt::new(self.recovery_packet_number).expect("2^62 is a lot of packets");
            self.recovery_packet_number += 1;

            let packet_len = {
                let buffer = info.descriptor.payload_mut();

                debug_assert!(!buffer.is_empty(), "empty retransmission buffer submitted");

                {
                    let buffer = DecoderBufferMut::new(buffer);
                    match decoder::Packet::retransmit(
                        buffer,
                        stream::PacketSpace::Recovery,
                        packet_number,
                        control_key,
                    ) {
                        Ok(info) => info,
                        Err(err) => {
                            // this shouldn't ever happen
                            debug_assert!(false, "{err:?}");
                            return Err(error::Kind::RetransmissionFailure.err());
                        }
                    }
                }

                buffer.len() as u16
            };

            let time_sent = clock.get_time();

            {
                let stream_offset = info.stream_offset;
                let payload_len = info.payload_len;
                let flags = info.flags;
                debug_assert!(!flags.is_probe(), "probes should not be retransmitted");
                let descriptor = info.descriptor;

                // TODO store this as part of the packet queue
                let ecn = ExplicitCongestionNotification::Ect0;

                let info = transmission::Info {
                    packet_len,
                    stream_offset,
                    payload_len,
                    flags,
                    // The descriptor gets stored later from the completion queue
                    descriptor: None,
                    time_sent,
                    ecn,
                };

                #[cfg(debug_assertions)]
                let _ = self.pending_retransmissions.insert(info.range());

                let meta = transmission::Meta {
                    packet_space: PacketSpace::Recovery,
                    final_offset: self.fin.value(),
                    half: Half::Write,
                    span: Default::default(),
                };

                let event = transmission::Event {
                    info,
                    meta,
                    packet_number,
                };

                publisher.on_stream_packet_transmitted(event::builder::StreamPacketTransmitted {
                    packet_len: packet_len as usize,
                    stream_offset: stream_offset.as_u64(),
                    payload_len: payload_len as usize,
                    packet_number: packet_number.as_u64(),
                    is_fin: flags.included_final_byte(),
                    is_retransmission: true,
                });

                // consider this transmission a probe if needed
                if self.pto.transmissions() > 0 {
                    self.pto.on_transmit_once();
                }
                self.keep_alive.on_transmit();

                // Track retransmission bytes in flight
                self.retransmission_bytes_in_flight += payload_len as u32;

                packets.push(event, descriptor);
            }
        }

        Ok(())
    }

    #[inline]
    pub fn try_transmit_probe<C, M, Clk, Pub>(
        &mut self,
        control_key: &C,
        credentials: &Credentials,
        stream_id: &stream::Id,
        source_queue_id: Option<VarInt>,
        packets: &mut M,
        clock: &Clk,
        publisher: &Pub,
    ) -> Result<(), Error>
    where
        C: crypto::seal::control::Stream,
        Clk: Clock,
        M: Message,
        Pub: event::ConnectionPublisher,
    {
        while self.requires_transmission() {
            // probes are not congestion-controlled
            let res = packets.push_with(|mut buffer| {
                let min_len = stream::encoder::MAX_RETRANSMISSION_HEADER_LEN + 128;
                assert!(buffer.len() >= min_len);

                let packet_number =
                    VarInt::new(self.recovery_packet_number).expect("2^62 is a lot of packets");
                self.recovery_packet_number += 1;

                let offset = self.max_data.max_sent_offset();
                let final_offset = self.fin.try_transmit();

                let included_final_byte = Some(offset) == final_offset;
                let included_final_offset = final_offset.is_some();
                let mut flags = transmission::Flags::empty()
                    .with_included_final_byte(included_final_byte)
                    .with_included_final_offset(included_final_offset)
                    .with_probe(true);

                let mut payload = probe::Probe {
                    offset,
                    final_offset,
                };

                // Include DATA_BLOCKED frame when blocked on flow control
                let data_blocked = self.max_data.try_transmit_data_blocked();
                let reset_frame = self.reset.try_transmit();

                let ping = if self.pto.transmissions() > 0 || self.keep_alive.is_queued() {
                    Some(frame::Ping)
                } else {
                    None
                };

                if data_blocked.is_some() {
                    publisher.on_stream_data_blocked_transmitted(
                        event::builder::StreamDataBlockedTransmitted {
                            stream_offset: offset.as_u64(),
                            packet_number: packet_number.as_u64(),
                        },
                    );
                }

                if reset_frame.is_some() {
                    flags = flags.with_included_reset(true);
                }

                let control_data = (ping, (data_blocked, reset_frame));
                let control_data_len = VarInt::try_from(control_data.encoding_size()).unwrap();

                let encoder = EncoderBuffer::new(&mut buffer);
                let packet_len = encoder::probe(
                    encoder,
                    source_queue_id,
                    *stream_id,
                    packet_number,
                    self.next_expected_control_packet,
                    VarInt::ZERO,
                    &mut &[][..],
                    control_data_len,
                    &control_data,
                    &mut payload,
                    control_key,
                    credentials,
                );

                let payload_len = 0;

                debug_assert!(
                    packet_len < u16::MAX as usize,
                    "cannot write larger packets than 2^16"
                );
                let packet_len = packet_len as u16;

                let time_sent = clock.get_time();

                let ecn = ExplicitCongestionNotification::Ect0;

                let info = transmission::Info {
                    packet_len,
                    stream_offset: offset,
                    payload_len,
                    flags,
                    descriptor: None,
                    time_sent,
                    ecn,
                };

                let meta = transmission::Meta {
                    packet_space: PacketSpace::Recovery,
                    final_offset,
                    half: Half::Write,
                    span: Default::default(),
                };

                transmission::Event {
                    packet_number,
                    info,
                    meta,
                }
            });

            match res {
                Ok(_) => {}
                Err(PushError::Alloc) => {
                    // We weren't able to allocate any packets so we'll need to try again later
                    // TODO how do we know when?
                    break;
                }
                Err(PushError::EmptyPacket) => {
                    // We shouldn't ever write an empty packet
                    debug_assert!(false, "probe packet should never be empty");
                    break;
                }
            }

            if self.pto.transmissions() > 0 {
                self.pto.on_transmit_once();
            }
            self.keep_alive.on_transmit();
        }

        Ok(())
    }

    #[inline]
    #[track_caller]
    pub fn on_error<E, Clk, Pub>(
        &mut self,
        error: E,
        source: Location,
        clock: &Clk,
        publisher: &Pub,
    ) where
        Error: From<E>,
        Clk: Clock,
        Pub: event::ConnectionPublisher,
    {
        let error = Error::from(error);

        // If the sender has already successfully finished (all data ACKed),
        // ignore late errors. A FlowReset arriving after DataRecvd shouldn't
        // poison a completed transfer.
        ensure!(!self.state().is_data_received());

        if matches!(error.kind, error::Kind::IdleTimeout) {
            self.idle_timer.cancel();
        }

        ensure!(self.reset.on_error(error, source).is_ok());
        if error.kind().is_abandoned() {
            publisher.on_stream_abandoned(event::builder::StreamAbandoned { error, source });
        } else {
            publisher
                .on_stream_sender_errored(event::builder::StreamSenderErrored { error, source });
        }

        // Emit abandon events for all unacknowledged stream packets
        let now = clock.get_time();
        for (num, packet) in self.sent_stream_packets.iter() {
            publisher.on_stream_packet_abandoned(event::builder::StreamPacketAbandoned {
                packet_len: packet.info.packet_len as usize,
                stream_offset: packet.info.stream_offset.as_u64(),
                payload_len: packet.info.payload_len as usize,
                packet_number: num.as_u64(),
                time_sent: packet.info.time_sent.into_event(),
                lifetime: now.saturating_duration_since(packet.info.time_sent),
                is_retransmission: false,
            });
        }

        // Emit abandon events for all unacknowledged recovery packets
        for (num, packet) in self.sent_recovery_packets.iter() {
            publisher.on_stream_packet_abandoned(event::builder::StreamPacketAbandoned {
                packet_len: packet.info.packet_len as usize,
                stream_offset: packet.info.stream_offset.as_u64(),
                payload_len: packet.info.payload_len as usize,
                packet_number: num.as_u64(),
                time_sent: packet.info.time_sent.into_event(),
                lifetime: now.saturating_duration_since(packet.info.time_sent),
                is_retransmission: true,
            });
        }

        self.clear_inflight_state();

        if source.is_remote() {
            // Remote errors don't need any further transmission, cancel everything
            self.idle_timer.cancel();
        } else {
            // Local errors need a short timeout to send the reset and wait for an ACK.
            // Use a shorter timeout (1s) instead of the full idle timeout.
            let error_timeout = Duration::from_secs(1);
            self.idle_timer.cancel();
            self.idle_timer.set(now + error_timeout);
        }
    }

    /// Called when the stream is finished and all data has been ACKed
    fn on_success(&mut self) {
        debug_assert!(self.state().is_data_received());

        self.clear_inflight_state();

        // TODO why does this cause tests to fail if we uncomment this?
        // self.idle_timer.cancel();
    }

    /// Clears all in-flight packet state, retransmission queues, counters,
    /// the PTO timer, and the progress watchdog.
    ///
    /// This is the shared cleanup between `on_error` and `on_success`. It
    /// ensures that stale counters don't trick `has_inflight_packets` into
    /// returning true, that leftover PTO transmissions don't cause
    /// `fill_transmit_queue` to bump `recovery_packet_number` or generate
    /// spurious probes, and that debug-only `pending_retransmissions` stay
    /// in sync with the cleared packet maps.
    fn clear_inflight_state(&mut self) {
        self.retransmissions.clear();
        self.sent_stream_packets.clear();
        self.sent_recovery_packets.clear();
        self.unacked_ranges.clear();
        self.counters = Default::default();
        self.keep_alive.on_shutdown();
        self.progress.cancel();

        #[cfg(debug_assertions)]
        self.pending_retransmissions.clear();

        // Cancel the PTO timer and drain any pending transmissions so
        // fill_transmit_queue won't try to send probes or bump
        // recovery_packet_number for a stream that is shutting down.
        self.pto.cancel();
        while self.pto.transmissions() > 0 {
            self.pto.on_transmit_once();
        }
    }

    #[cfg(not(debug_assertions))]
    #[inline(always)]
    fn invariants(&self) {}
}

#[cfg(debug_assertions)]
impl State {
    #[inline]
    fn invariants(&self) {
        self.invariant_unacked_coverage();
        self.invariant_counters();
        self.invariant_packet_numbers();
        self.invariant_fin_consistency();
        self.invariant_terminal_state();
        self.invariant_retransmission_ranges();
    }

    /// Every unacked byte must be accounted for by an in-flight packet (with a descriptor),
    /// a pending retransmission segment, or a pending_retransmissions entry.
    fn invariant_unacked_coverage(&self) {
        if !self.unacked_ranges.is_empty() {
            let mut unacked_ranges = self.unacked_ranges.clone();
            let last = unacked_ranges.inclusive_ranges().next_back().unwrap();
            unacked_ranges.remove(last).unwrap();

            for (_pn, packet) in self.sent_stream_packets.iter() {
                if packet.info.payload_len == 0 {
                    continue;
                }

                if packet.info.descriptor.is_some() {
                    unacked_ranges.remove(packet.info.range()).unwrap();
                }
            }

            for (_pn, packet) in self.sent_recovery_packets.iter() {
                if packet.info.payload_len == 0 {
                    continue;
                }

                if packet.info.descriptor.is_some() {
                    unacked_ranges.remove(packet.info.range()).unwrap();
                }
            }

            for v in self.retransmissions.iter() {
                if v.payload_len == 0 {
                    continue;
                }
                unacked_ranges.remove(v.range()).unwrap();
            }

            for range in self.pending_retransmissions.inclusive_ranges() {
                unacked_ranges.remove(range).unwrap();
            }

            assert!(
                unacked_ranges.is_empty(),
                "unacked ranges should be empty: {unacked_ranges:?}\n state\n {self:#?}"
            );

            if let (Some(expected), Some(actual)) =
                (self.fin.value(), self.unacked_ranges.max_value())
            {
                assert!(
                    expected >= actual,
                    "once the final offset is known the unacked ranges should be clamped"
                );
            }
        }
    }

    /// The `counters` fields must exactly match the actual in-flight packets in
    /// `sent_stream_packets` and `sent_recovery_packets`.
    fn invariant_counters(&self) {
        let mut expected = InflightCounters::default();

        for (_pn, packet) in self.sent_stream_packets.iter() {
            expected.on_transmit(&packet.info);
        }

        for (_pn, packet) in self.sent_recovery_packets.iter() {
            expected.on_transmit(&packet.info);
        }

        assert_eq!(
            self.counters.probes, expected.probes,
            "probes counter mismatch: tracked={}, actual={}",
            self.counters.probes, expected.probes,
        );
        assert_eq!(
            self.counters.with_payload, expected.with_payload,
            "with_payload counter mismatch: tracked={}, actual={}",
            self.counters.with_payload, expected.with_payload,
        );
        assert_eq!(
            self.counters.with_final_offset, expected.with_final_offset,
            "with_final_offset counter mismatch: tracked={}, actual={}",
            self.counters.with_final_offset, expected.with_final_offset,
        );
        assert_eq!(
            self.counters.with_reset, expected.with_reset,
            "with_reset counter mismatch: tracked={}, actual={}",
            self.counters.with_reset, expected.with_reset,
        );
    }

    /// Packet number relationships:
    /// - `max_stream_packet_number` must equal the maximum key in `sent_stream_packets` (if non-empty)
    /// - `recovery_packet_number` must be > all in-flight recovery packet numbers
    /// - `max_stream_packet_number_lost` must be <= `max_stream_packet_number + 1`
    /// - All recovery packet numbers must be > `max_stream_packet_number`
    fn invariant_packet_numbers(&self) {
        // max_stream_packet_number must be >= the maximum key in sent_stream_packets
        if !self.sent_stream_packets.is_empty() {
            let max_inflight = self.sent_stream_packets.get_range().end().as_u64();
            let max_inflight = unsafe { VarInt::new_unchecked(max_inflight) };
            assert!(
                self.max_stream_packet_number >= max_inflight,
                "max_stream_packet_number ({}) < max in-flight stream packet ({})",
                self.max_stream_packet_number,
                max_inflight,
            );
        }

        // recovery_packet_number must be > all in-flight recovery packet numbers
        if !self.sent_recovery_packets.is_empty() {
            let max_recovery_inflight = self.sent_recovery_packets.get_range().end().as_u64();
            assert!(
                self.recovery_packet_number > max_recovery_inflight,
                "recovery_packet_number ({}) <= max in-flight recovery packet ({})",
                self.recovery_packet_number,
                max_recovery_inflight,
            );
        }

        // The next recovery_packet_number to assign must be > max_stream_packet_number
        // because recovery packets are always numbered above stream packets at the time
        // of assignment. (In-flight recovery packets may have numbers <= the current
        // max_stream_packet_number if new stream packets were loaded after those recovery
        // packets were sent.)
        if !self.sent_stream_packets.is_empty() || !self.sent_recovery_packets.is_empty() {
            assert!(
                self.recovery_packet_number >= *self.max_stream_packet_number as u64 + 1,
                "recovery_packet_number ({}) < max_stream_packet_number + 1 ({})",
                self.recovery_packet_number,
                *self.max_stream_packet_number as u64 + 1,
            );
        }
    }

    /// Fin state must be consistent with unacked_ranges:
    /// - If fin value is known, unacked_ranges must not contain offsets >= final_offset
    ///   (except the trailing sentinel when fin isn't fully acked yet)
    ///
    /// Note: fin can be acked while data ranges are still unacked — the fin
    /// was carried in a packet that covered the tail of the stream, but earlier
    /// data may still be in flight. The `state()` function correctly requires
    /// BOTH `unacked_ranges.is_empty()` AND `fin.is_acked()` for DataRecvd.
    fn invariant_fin_consistency(&self) {
        if let Some(final_offset) = self.fin.value() {
            // No data offsets beyond final_offset should be unacked
            // (VarInt::MAX sentinel is expected when fin isn't fully acked yet)
            for range in self.unacked_ranges.inclusive_ranges() {
                let start = *range.start();
                if start >= final_offset && start != VarInt::MAX {
                    panic!(
                        "unacked_ranges contains offset {} beyond final_offset {}",
                        start, final_offset,
                    );
                }
            }
        }
    }

    /// When the state is terminal, there must be no in-flight packets,
    /// no pending retransmissions, and PTO must not be armed.
    fn invariant_terminal_state(&self) {
        if !self.state().is_terminal() {
            return;
        }

        assert!(
            self.sent_stream_packets.is_empty(),
            "terminal state but sent_stream_packets is non-empty",
        );
        assert!(
            self.sent_recovery_packets.is_empty(),
            "terminal state but sent_recovery_packets is non-empty",
        );
        assert!(
            self.retransmissions.is_empty(),
            "terminal state but retransmissions queue is non-empty",
        );
        assert!(
            self.unacked_ranges.is_empty(),
            "terminal state but unacked_ranges is non-empty: {:?}",
            self.unacked_ranges,
        );
        assert_eq!(
            self.pto.transmissions(),
            0,
            "terminal state but PTO has pending transmissions",
        );
    }

    /// Retransmissions may transiently include stale entries (e.g., PTO
    /// probe copies whose data was ACKed before the copy was transmitted).
    /// These get cleaned up lazily in `try_transmit_retransmissions`. This
    /// invariant ensures the stale count stays bounded — it should never
    /// exceed the number of in-flight packets, since each stale entry
    /// originated as a copy of one.
    fn invariant_retransmission_ranges(&self) {
        let mut stale_count = 0u32;
        for segment in self.retransmissions.iter() {
            if segment.payload_len == 0 {
                continue;
            }
            if !self.unacked_ranges.contains(&segment.stream_offset) {
                stale_count += 1;
            }
        }

        // Each stale retransmission was a deep copy of an in-flight packet
        // created by `retransmit_copy()`. The number of copies is bounded by
        // PTO transmissions (typically 1-2 per PTO fire). If stale entries
        // accumulate beyond the in-flight count, we're leaking retransmissions.
        let inflight_count =
            self.sent_stream_packets.iter().count() + self.sent_recovery_packets.iter().count();
        // Use max(inflight, 2) to handle the edge case where all in-flight
        // packets have been ACKed but PTO copies haven't been drained yet.
        let max_stale = inflight_count.max(2);
        assert!(
            (stale_count as usize) <= max_stale,
            "too many stale retransmissions: {stale_count} stale entries but only {inflight_count} in-flight packets",
        );
    }
}

impl timer::Provider for State {
    #[inline]
    fn timers<Q: timer::Query>(&self, query: &mut Q) -> timer::Result {
        // if we're in a terminal state then no timers are needed
        ensure!(!self.state().is_terminal(), Ok(()));
        self.pto.timers(query)?;
        self.max_data.timers(query)?;
        self.idle_timer.timers(query)?;
        self.keep_alive.timers(query)?;
        self.progress.timers(query)?;
        Ok(())
    }
}

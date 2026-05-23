// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! ACK processing and loss detection for the send path.
//!
//! Processes incoming ACK frames against the send::Context's inflight map. When packets
//! are acknowledged, their constituent frames get completion notifications. When packets
//! are declared lost, frames are individually evaluated for retransmission (TTL, cancellation)
//! and survivors are requeued to the wheel.

use crate::{
    congestion,
    endpoint::{
        frame::{self, Frame, TransmissionStatus},
        send,
    },
    intrusive::Entry,
    socket::channel::UnboundedSender,
    tracing::*,
};
use core::time::Duration;
use s2n_quic_core::{
    frame::{
        self as quic_frame,
        ack::{AckRanges, EcnCounts},
    },
    packet::number::{PacketNumber, PacketNumberRange, PacketNumberSpace},
    random,
    varint::VarInt,
};

pub(crate) mod state;

/// Process an ACK frame against the send context.
///
/// Removes ACKed packets from the inflight map:
/// - **completed**: frames the writer needs to hear about (successfully ACKed or TTL-exhausted)
/// - **lost**: retransmittable frames (TTL remaining, still transmittable)
/// - **cancelled**: `should_transmit()` is false (writer already gone) — silently dropped
pub(crate) fn process_ack<Clk, Rand>(
    ack: &quic_frame::Ack<impl AckRanges>,
    ack_delay: Duration,
    context: &mut send::Context,
    counters: &super::counters::Send,
    completed: &mut impl UnboundedSender<Entry<Frame>>,
    lost: &mut impl UnboundedSender<Entry<Frame>>,
    cancelled: &mut impl UnboundedSender<Entry<Frame>>,
    clock: &Clk,
    random: &mut Rand,
    deferred: &mut Vec<PacketNumber>,
) where
    Clk: s2n_quic_core::time::Clock + ?Sized,
    Rand: random::Generator,
{
    let now = clock.get_time();

    let mut max_acked_tx_time = None;
    let mut bytes_acked = 0usize;
    let mut packets_acked = 0u64;
    let mut cca_args = None;
    // RTT sample from an ack-eliciting ACK-only packet (read-heavy path).
    // Set when an ACK range covers the pending PN recorded by `rtt_tracker`.
    let mut ack_only_rtt_sample: Option<s2n_quic_core::time::Timestamp> = None;

    let max_acked_pn = ack.largest_acknowledged();

    for range in ack.ack_ranges() {
        let start = *range.start();
        let end = *range.end();

        // Check whether this range covers the outstanding ack-eliciting ACK-only
        // packet (if any) and collect the RTT sample.
        if let Some(time_sent) = context.rtt_tracker.check_range(start, end) {
            ack_only_rtt_sample = Some(time_sent);
        }

        let pmin = PacketNumberSpace::Initial.new_packet_number(start);
        let pmax = PacketNumberSpace::Initial.new_packet_number(end);
        let range = PacketNumberRange::new(pmin, pmax);

        // Phase 1: remove ACKed entries from the inflight map.
        //
        // Shell entries (probed_to.is_some()) have empty `frames`; the live frames
        // reside at the tail of the probe chain at a higher PN. We defer chain
        // following until after the iterator is dropped (so the borrow on
        // `context.inflight` is released).

        for (num, mut packet) in context.inflight.remove_range(range) {
            packets_acked += 1;

            if let Some(tx_info) = packet.transmission_info.take() {
                let time_sent = tx_info.time_sent;
                max_acked_tx_time = max_acked_tx_time.max(Some(time_sent));

                if cca_args
                    .as_ref()
                    .map_or(true, |(prev_time, _): &(_, congestion::PacketInfo)| {
                        *prev_time < time_sent
                    })
                {
                    cca_args = Some((time_sent, tx_info.cc_info));
                }

                bytes_acked += tx_info.sent_bytes as usize;
            }

            trace!(
                credentials = %context.credentials.id,
                sender_idx = %context.sender_idx,
                packet_number = num.as_u64(),
                "packet ACKed"
            );

            if let Some(probe_pn) = packet.probed_to {
                // Shell: the live frames are at the tail of the probe chain.
                // Defer completion to Phase 2 (after the iterator is dropped).
                // If we somehow exceed capacity, the tail frames will remain in
                // the inflight map and be completed when the probe entry itself
                // is ACKed or swept by loss detection.
                deferred.push(probe_pn);
            } else {
                for mut entry in packet.frames {
                    counters.on_acked_frame(&entry.header);
                    entry.status = TransmissionStatus::Acknowledged;
                    let _ = completed.send(entry);
                }
            }
        }
        // remove_range iterator is dropped here; borrow on `context.inflight` released.

        // Phase 2: follow deferred probe chains and complete the tail frames.
        for probe_pn in deferred.drain(..) {
            let removal = context.inflight.remove_chain(probe_pn);
            if removal.discarded_bytes > 0 {
                context.cca.on_packet_discarded(removal.discarded_bytes);
            }
            for mut entry in removal.frames {
                counters.on_acked_frame(&entry.header);
                entry.status = TransmissionStatus::Acknowledged;
                let _ = completed.send(entry);
            }
        }
    }

    counters.ack_packets.record_value(packets_acked);
    counters.on_inflight_drain_ack(packets_acked);

    // Finalize loss detection for the ACK-only RTT tracker. This must be called
    // after all ranges have been processed so that the loss heuristic does not
    // fire on the first (largest) range and discard a slot that would have been
    // covered by a later (smaller) range in the same ACK frame.
    context.rtt_tracker.on_ack_done(max_acked_pn);

    // Update RTT estimator and CCA.
    //
    // Data ACKs take priority: if any inflight data packet was acknowledged we
    // compute the RTT sample from the most recently sent one. Otherwise, fall
    // back to the ack-only RTT sample (read-heavy path) if one is available.
    if let Some((time_sent, cc_info)) = cca_args {
        let rtt_sample = now
            .saturating_duration_since(time_sent)
            .saturating_sub(ack_delay)
            .max(Duration::from_micros(1));

        context.rtt_estimator.update_rtt(
            Duration::ZERO,
            rtt_sample,
            now,
            true,
            PacketNumberSpace::ApplicationData,
        );

        context.cca.on_packet_ack(
            cc_info.first_sent_time,
            bytes_acked,
            cc_info,
            &context.rtt_estimator,
            random,
            now,
        );
    } else if let Some(ack_only_time_sent) = ack_only_rtt_sample {
        // No data was ACKed in this frame, but the peer acknowledged our
        // ack-eliciting ACK-only packet. Use this to keep the RTT estimate fresh.
        let rtt_sample = now
            .saturating_duration_since(ack_only_time_sent)
            .saturating_sub(ack_delay)
            .max(Duration::from_micros(1));

        trace!(
            credentials = %context.credentials.id,
            sender_idx = %context.sender_idx,
            ?rtt_sample,
            "RTT updated from ack-only packet (read-heavy path)"
        );

        context.rtt_estimator.update_rtt(
            Duration::ZERO,
            rtt_sample,
            now,
            true,
            PacketNumberSpace::ApplicationData,
        );
    }

    // Process ECN feedback from the peer
    if let Some(ecn_counts) = ack.ecn_counts {
        let prev = context.peer_ecn_counts;
        context.peer_ecn_counts = ecn_counts.max(prev);
        let mut delta = context.peer_ecn_counts;
        delta -= prev;
        if delta != EcnCounts::default() {
            counters.on_peer_ecn(&delta);
            let ce_delta = delta.ce_count.as_u64();
            if ce_delta > 0 {
                context.cca.on_explicit_congestion(ce_delta, now);
            }
        }
    }

    // Run loss detection
    if let Some(max_tx_time) = max_acked_tx_time {
        detect_loss(
            context,
            max_acked_pn,
            max_tx_time,
            counters,
            completed,
            lost,
            cancelled,
            now,
            random,
        );
    }

    // Update PTO
    let has_remaining_inflight = context.inflight.has_inflight();
    context.pto.on_ack_received(has_remaining_inflight);
    if !has_remaining_inflight {
        context.pto_wheel.target_time = None;
    }

    // If the tx wheel entry is now stale (scheduling reason removed by the ACK), clear
    // target_time so the wheel treats it as expired on next tick rather than firing the
    // invariant. The assembler handles stale pops gracefully (produces zero segments).
    if context.tx_wheel.is_scheduled()
        && !context.has_pending_acks()
        && !context.pto.probe_state.is_requested()
        && !(context.has_pending_data() && context.can_send_pending_frames())
    {
        context.tx_wheel.target_time = None;
    }

    // Sample CCA state after all mutations (ack, ECN, loss) have run.
    counters.on_cca_state(
        context.cca.congestion_window(),
        context.cca.bandwidth().as_bytes_per_second(),
        context.cca.is_congestion_limited(),
    );

    // Publish the load score after ALL CCA mutations have run:
    // on_packet_ack, on_explicit_congestion (ECN), and on_packet_lost (loss detection).
    // This ensures pick-two sees the fully-updated pacing and congestion state.
    context.publish_sender_load_score(now);
    context.invariants();
}

/// Detect lost packets using QUIC PN-threshold and time-threshold algorithms.
///
/// Packets sent before `max_acked_pn` are declared lost when either:
/// - packet number <= max_acked_pn - 3, or
/// - time_sent <= max_tx_time - loss_time_threshold()
///
/// For each lost packet,
/// frames are individually evaluated:
/// - should_transmit false → sent to `cancelled` (writer already gone, no notification)
/// - TTL exhausted → sent to `completed` (writer needs failure notification)
/// - Otherwise → decrement TTL and send to `lost` for retransmission
fn detect_loss<Rand>(
    context: &mut send::Context,
    max_acked_pn: VarInt,
    max_tx_time: s2n_quic_core::time::Timestamp,
    counters: &super::counters::Send,
    completed: &mut impl UnboundedSender<Entry<Frame>>,
    lost: &mut impl UnboundedSender<Entry<Frame>>,
    cancelled: &mut impl UnboundedSender<Entry<Frame>>,
    now: s2n_quic_core::time::Timestamp,
    random: &mut Rand,
) where
    Rand: random::Generator,
{
    let pn_threshold = max_acked_pn.checked_sub(VarInt::from_u8(3));
    let time_threshold = context.rtt_estimator.loss_time_threshold();
    let time_loss_cutoff = max_tx_time.checked_sub(time_threshold);

    let largest_acked = PacketNumberSpace::Initial.new_packet_number(max_acked_pn);
    let pn_loss_cutoff = pn_threshold.map(|v| PacketNumberSpace::Initial.new_packet_number(v));

    let Some(lost_max) = context
        .inflight
        .loss_cutoff(largest_acked, pn_loss_cutoff, time_loss_cutoff)
    else {
        return;
    };

    let lost_min = PacketNumberSpace::Initial.new_packet_number(VarInt::ZERO);
    let range = PacketNumberRange::new(lost_min, lost_max);
    let mut lost_count = 0usize;
    let mut cancelled_count = 0usize;
    let mut ttl_exhausted_count = 0usize;

    for (num, mut packet) in context.inflight.remove_range(range) {
        let tx_info = packet.transmission_info.take().unwrap();

        trace!(
            pn = num.as_u64(),
            max_acked = max_acked_pn.as_u64(),
            time_sent = ?tx_info.time_sent,
            "Packet lost by PN threshold"
        );

        context
            .cca
            .on_packet_lost(tx_info.sent_bytes as u32, tx_info.cc_info, random, now);

        for mut entry in packet.frames {
            if !entry.should_transmit() {
                entry.status = TransmissionStatus::Failed(frame::FailureReason::Cancelled);
                let _ = cancelled.send(entry);
                cancelled_count += 1;
                continue;
            }

            if entry.ttl == 0 {
                debug_assert_ne!(entry.ttl, 0, "frame TTL should never be exhausted");
                error!(
                    credentials = %context.credentials.id,
                    sender_idx = %context.sender_idx,
                    frame = ?*entry,
                    "frame TTL exhausted - this should never happen"
                );
                entry.status = TransmissionStatus::Failed(frame::FailureReason::TransmissionError);
                let _ = completed.send(entry);
                ttl_exhausted_count += 1;
                continue;
            }

            entry.ttl -= 1;
            let _ = lost.send(entry);
            lost_count += 1;
        }
    }

    if ttl_exhausted_count > 0 {
        counters.ttl_exhausted.add(ttl_exhausted_count as u64);
    }

    if lost_count + cancelled_count + ttl_exhausted_count > 0 {
        counters
            .on_inflight_drain_loss((lost_count + cancelled_count + ttl_exhausted_count) as u64);
        debug!(
            lost_count,
            cancelled_count,
            ttl_exhausted_count,
            max_acked = max_acked_pn.as_u64(),
            pn_threshold = pn_threshold.map(|v| v.as_u64()),
            time_threshold = ?time_threshold,
            time_cutoff = ?time_loss_cutoff,
            rtt = ?context.rtt_estimator.smoothed_rtt(),
            "Loss detection triggered"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        byte_vec::ByteVec,
        counter::Registry,
        endpoint::{
            frame::{Frame, Header, TransmissionStatus, DEFAULT_TTL},
            id::LocalSenderId,
            inflight::{Packet, TransmissionInfo},
        },
        packet::datagram::QueuePair,
        path::secret::map::Entry as PathSecretEntry,
        xorshift,
    };
    use bytes::Bytes;
    use std::sync::Arc;

    #[derive(Default)]
    struct CollectFrames(Vec<Entry<Frame>>);

    impl UnboundedSender<Entry<Frame>> for CollectFrames {
        fn send(&mut self, value: Entry<Frame>) -> Result<(), Entry<Frame>> {
            self.0.push(value);
            Ok(())
        }
    }

    fn make_ts(millis: u64) -> s2n_quic_core::time::Timestamp {
        unsafe { s2n_quic_core::time::Timestamp::from_duration(Duration::from_millis(millis)) }
    }

    fn make_pn(n: u64) -> s2n_quic_core::packet::number::PacketNumber {
        PacketNumberSpace::Initial.new_packet_number(VarInt::new(n).unwrap())
    }

    fn make_path_secret_entry() -> Arc<PathSecretEntry> {
        let peer: std::net::SocketAddr = "127.0.0.1:9999".parse().unwrap();
        let entry = PathSecretEntry::builder(peer).socket_sender_count(1).build();
        entry.set_peer_data_addrs(&[peer]);
        entry
    }

    fn make_context(entry: &Arc<PathSecretEntry>) -> send::Context {
        let registry = Registry::default();
        send::Context::new(
            entry,
            registry.register_queue_gauge("test.inflight"),
            registry.register_queue_gauge("test.ack"),
            registry.register_queue_gauge("test.pending"),
            LocalSenderId::new(VarInt::ZERO),
            &crate::time::bach::Clock::default(),
        )
        .expect("context should initialize in tests")
    }

    fn make_packet(
        context: &mut send::Context,
        entry: Arc<PathSecretEntry>,
        time_sent: s2n_quic_core::time::Timestamp,
    ) -> Packet {
        let mut payload = ByteVec::new();
        payload.push_back(Bytes::from_static(b"x"));

        let mut frames = crate::intrusive::Queue::new();
        frames.push_back(
            Frame {
                header: Header::FlowData {
                    queue_pair: QueuePair {
                        source_queue_id: VarInt::from_u8(1),
                        dest_queue_id: VarInt::from_u8(2),
                    },
                    stream_id: VarInt::from_u8(1),
                    offset: VarInt::ZERO,
                    is_fin: false,
                },
                source_sender_id: LocalSenderId::UNSPECIFIED,
                payload,
                path_secret_entry: entry,
                completion: None,
                status: TransmissionStatus::Pending,
                ttl: DEFAULT_TTL,
                transmission_time: None,
            }
            .into(),
        );

        let cc_info = context
            .cca
            .on_packet_sent(time_sent, 100, false, &context.rtt_estimator);
        Packet::new(
            frames,
            TransmissionInfo {
                cc_info,
                time_sent,
                sent_bytes: 100,
            },
        )
    }

    #[test]
    fn detect_loss_applies_time_threshold_without_pn_threshold() {
        let entry = make_path_secret_entry();
        let mut context = make_context(&entry);
        let counters =
            super::super::counters::Send::new(&Registry::default(), LocalSenderId::new(VarInt::ZERO));
        let mut completed = CollectFrames::default();
        let mut lost = CollectFrames::default();
        let mut cancelled = CollectFrames::default();
        let mut random = xorshift::Rng::with_seed(1);

        // With max_acked=2, PN threshold underflows (no PN-based loss), but packet 1 is
        // old enough relative to packet 2's tx time to be declared lost by time threshold.
        let packet1 = make_packet(&mut context, entry.clone(), make_ts(100));
        context.inflight.insert(make_pn(1), packet1);
        let packet2 = make_packet(&mut context, entry.clone(), make_ts(104));
        context.inflight.insert(make_pn(2), packet2);
        context.next_packet_number = VarInt::from_u8(3);

        detect_loss(
            &mut context,
            VarInt::from_u8(2),
            make_ts(104),
            &counters,
            &mut completed,
            &mut lost,
            &mut cancelled,
            make_ts(110),
            &mut random,
        );

        assert_eq!(lost.0.len(), 1, "old packet should be declared lost by time");
        assert!(cancelled.0.is_empty());
        assert!(completed.0.is_empty());
        assert!(context.inflight.remove(make_pn(1)).is_none());
        assert!(context.inflight.remove(make_pn(2)).is_some());
    }
}

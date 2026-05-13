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
    intrusive_queue::Entry,
    random,
    socket::channel::UnboundedSender,
    stream3::{
        endpoint::send,
        frame::{self, Frame, TransmissionStatus},
    },
};
use arrayvec::ArrayVec;
use core::time::Duration;
use s2n_quic_core::{
    frame::{self as quic_frame, ack::AckRanges},
    packet::number::{PacketNumber, PacketNumberRange, PacketNumberSpace},
    varint::VarInt,
};

/// Process an ACK frame against the send context.
///
/// Removes ACKed packets from the inflight map:
/// - **completed**: frames the writer needs to hear about (successfully ACKed or TTL-exhausted)
/// - **lost**: retransmittable frames (TTL remaining, still transmittable)
/// - **cancelled**: `should_transmit()` is false (writer already gone) — silently dropped
pub(crate) fn process_ack<Clk, Rand>(
    ack: &quic_frame::Ack<impl AckRanges>,
    context: &mut send::Context,
    completed: &mut impl UnboundedSender<Entry<Frame>>,
    lost: &mut impl UnboundedSender<Entry<Frame>>,
    cancelled: &mut impl UnboundedSender<Entry<Frame>>,
    clock: &Clk,
    random: &mut Rand,
) where
    Clk: s2n_quic_core::time::Clock + ?Sized,
    Rand: random::Generator,
{
    let now = clock.get_time();
    let ack_delay = ack.ack_delay();

    let mut max_acked_tx_time = None;
    let mut bytes_acked = 0usize;
    let mut cca_args = None;

    let max_acked_pn = ack.largest_acknowledged();

    for range in ack.ack_ranges() {
        let pmin = PacketNumberSpace::Initial.new_packet_number(*range.start());
        let pmax = PacketNumberSpace::Initial.new_packet_number(*range.end());
        let range = PacketNumberRange::new(pmin, pmax);

        // Phase 1: remove ACKed entries from the inflight map.
        //
        // Shell entries (probed_to.is_some()) have empty `frames`; the live frames
        // reside at the tail of the probe chain at a higher PN. We defer chain
        // following until after the iterator is dropped (so the borrow on
        // `context.inflight` is released) and use a small fixed-size ArrayVec
        // (no heap allocation, no zeroed-memory initialisation) to record which
        // chain heads to follow.
        //
        // The maximum number of shells in a single ACK range is bounded by the PTO
        // backoff cap (16×, ~4 doublings), so 8 slots is more than sufficient.
        let mut deferred: ArrayVec<PacketNumber, 8> = ArrayVec::new();

        for (num, mut packet) in context.inflight.remove_range(range) {
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

            tracing::trace!(
                credentials = %context.credentials.id,
                sender_idx = context.sender_idx,
                packet_number = num.as_u64(),
                "packet ACKed"
            );

            if let Some(probe_pn) = packet.probed_to {
                // Shell: the live frames are at the tail of the probe chain.
                // Defer completion to Phase 2 (after the iterator is dropped).
                // If we somehow exceed capacity, the tail frames will remain in
                // the inflight map and be completed when the probe entry itself
                // is ACKed or swept by loss detection.
                let _ = deferred.try_push(probe_pn);
            } else {
                for mut entry in packet.frames {
                    entry.status = TransmissionStatus::Acknowledged;
                    let _ = completed.send(entry);
                }
            }
        }
        // remove_range iterator is dropped here; borrow on `context.inflight` released.

        // Phase 2: follow deferred probe chains and complete the tail frames.
        for probe_pn in &deferred {
            let (_, tail_frames) = context.inflight.take_chain_tail_frames(*probe_pn);
            for mut entry in tail_frames {
                entry.status = TransmissionStatus::Acknowledged;
                let _ = completed.send(entry);
            }
        }
    }

    // Update RTT estimator and CCA
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

        context.publish_next_transmission_time(now);
    }

    // Run loss detection
    if let Some(max_tx_time) = max_acked_tx_time {
        detect_loss(
            context,
            max_acked_pn,
            max_tx_time,
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
}

/// Detect lost packets using the QUIC PN-threshold algorithm.
///
/// Packets with number <= max_acked_pn - 3 are declared lost. For each lost packet,
/// frames are individually evaluated:
/// - should_transmit false → sent to `cancelled` (writer already gone, no notification)
/// - TTL exhausted → sent to `completed` (writer needs failure notification)
/// - Otherwise → decrement TTL and send to `lost` for retransmission
///
/// TODO: Add time-based loss detection (kTimeThreshold = 9/8 * max(smoothed_rtt, latest_rtt)).
fn detect_loss<Rand>(
    context: &mut send::Context,
    max_acked_pn: VarInt,
    max_tx_time: s2n_quic_core::time::Timestamp,
    completed: &mut impl UnboundedSender<Entry<Frame>>,
    lost: &mut impl UnboundedSender<Entry<Frame>>,
    cancelled: &mut impl UnboundedSender<Entry<Frame>>,
    now: s2n_quic_core::time::Timestamp,
    random: &mut Rand,
) where
    Rand: random::Generator,
{
    let pn_threshold = max_acked_pn.checked_sub(VarInt::from_u8(3));

    let lost_min = PacketNumberSpace::Initial.new_packet_number(VarInt::ZERO);
    let lost_max = pn_threshold.map(|v| PacketNumberSpace::Initial.new_packet_number(v));

    let Some(lost_max) = lost_max else {
        return;
    };

    let range = PacketNumberRange::new(lost_min, lost_max);
    let mut lost_count = 0usize;
    let mut cancelled_count = 0usize;

    for (num, mut packet) in context.inflight.remove_range(range) {
        let tx_info = packet.transmission_info.take().unwrap();

        tracing::trace!(
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
                entry.status = TransmissionStatus::Failed(frame::FailureReason::TransmissionError);
                let _ = completed.send(entry);
                lost_count += 1;
                continue;
            }

            entry.ttl -= 1;
            let _ = lost.send(entry);
            lost_count += 1;
        }
    }

    if lost_count + cancelled_count > 0 {
        tracing::debug!(
            lost_count,
            cancelled_count,
            max_acked = max_acked_pn.as_u64(),
            threshold = pn_threshold.map(|v| v.as_u64()),
            rtt = ?context.rtt_estimator.smoothed_rtt(),
            "Loss detection triggered"
        );
    }
}

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
    intrusive_queue::Queue,
    random,
    stream3::{
        endpoint::{inflight, send},
        frame::{self, Frame, TransmissionStatus},
    },
};
use core::time::Duration;
use s2n_quic_core::{
    frame::{self as quic_frame, ack::AckRanges},
    packet::number::{PacketNumberRange, PacketNumberSpace},
    varint::VarInt,
};

/// Process an ACK frame against the send context.
///
/// Removes ACKed packets from the inflight map, sends acknowledged frames to `acked`
/// (for completion batching, logging, etc.), retransmittable frames to `lost` (for
/// resubmission to the wheel), and failed/cancelled frames to `cancelled` (for
/// completion notification with failure status).
pub(crate) fn process_ack<Clk, Rand>(
    ack: &quic_frame::Ack<impl AckRanges>,
    context: &mut send::Context,
    acked: &mut impl crate::socket::channel::UnboundedSender<Queue<Frame>>,
    lost: &mut impl crate::socket::channel::UnboundedSender<Queue<Frame>>,
    cancelled: &mut impl crate::socket::channel::UnboundedSender<Queue<Frame>>,
    clock: &Clk,
    random: &mut Rand,
) where
    Clk: s2n_quic_core::time::Clock + ?Sized,
    Rand: random::Generator,
{
    let now = clock.get_time();
    let ack_delay = ack.ack_delay();

    let mut max_acked_pn = None;
    let mut max_acked_tx_time = None;
    let mut bytes_acked = 0usize;
    let mut cca_args = None;

    // Use the ACK frame's largest_acknowledged as the authoritative max PN.
    max_acked_pn = Some(ack.largest_acknowledged());

    // Process each ACK range
    let mut acked_frames = Queue::new();
    for range in ack.ack_ranges() {
        let pmin = PacketNumberSpace::Initial.new_packet_number(*range.start());
        let pmax = PacketNumberSpace::Initial.new_packet_number(*range.end());
        let range = PacketNumberRange::new(pmin, pmax);

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

            tracing::trace!(packet_number = num.as_u64(), "Packet ACKed");

            // Mark all frames in this packet as acknowledged
            for mut entry in packet.frames {
                entry.status = TransmissionStatus::Acknowledged;
                acked_frames.push_back(entry);
            }
        }
    }

    let _ = acked.send(acked_frames);

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

        // Publish updated load estimate: the CCA bandwidth sample has changed.
        context.publish_next_transmission_time(now);
    }

    // Run loss detection
    if let Some(max_acked_pn) = max_acked_pn {
        if let Some(max_tx_time) = max_acked_tx_time {
            detect_loss(
                context,
                max_acked_pn,
                max_tx_time,
                lost,
                cancelled,
                now,
                random,
            );
        }
    }

    // Update PTO
    let has_remaining_inflight = context.inflight.has_inflight();
    context.pto.on_ack_received(has_remaining_inflight);
}

/// Detect lost packets using the QUIC PN-threshold algorithm.
///
/// Packets with number <= max_acked_pn - 3 are declared lost. For each lost packet,
/// frames are individually evaluated:
/// - should_transmit false (cancelled) or TTL exhausted → send to `cancelled`
/// - Otherwise → decrement TTL and send to `lost` for retransmission
///
/// TODO: Add time-based loss detection (kTimeThreshold = 9/8 * max(smoothed_rtt, latest_rtt)).
fn detect_loss<Rand>(
    context: &mut send::Context,
    max_acked_pn: VarInt,
    max_tx_time: s2n_quic_core::time::Timestamp,
    lost: &mut impl crate::socket::channel::UnboundedSender<Queue<Frame>>,
    cancelled: &mut impl crate::socket::channel::UnboundedSender<Queue<Frame>>,
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
    let mut retransmit_queue = Queue::new();
    let mut cancelled_queue = Queue::new();

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

        lost_count += 1;

        for mut entry in packet.frames {
            if !entry.should_transmit() {
                entry.status = TransmissionStatus::Failed(frame::FailureReason::Cancelled);
                cancelled_queue.push_back(entry);
                continue;
            }

            if entry.ttl == 0 {
                entry.status = TransmissionStatus::Failed(frame::FailureReason::TransmissionError);
                cancelled_queue.push_back(entry);
                continue;
            }

            entry.ttl -= 1;
            retransmit_queue.push_back(entry);
        }
    }

    if lost_count > 0 {
        tracing::debug!(
            lost_count,
            retransmit = retransmit_queue.len(),
            cancelled = cancelled_queue.len(),
            max_acked = max_acked_pn.as_u64(),
            threshold = pn_threshold.map(|v| v.as_u64()),
            rtt = ?context.rtt_estimator.smoothed_rtt(),
            "Loss detection triggered"
        );
    }

    let _ = lost.send(retransmit_queue);
    let _ = cancelled.send(cancelled_queue);
}

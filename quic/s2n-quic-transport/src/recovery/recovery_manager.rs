// TODO: Remove when used
#![allow(dead_code)]

use crate::recovery::{SentPacketInfo, SentPackets};
use core::{cmp::max, time::Duration};
use s2n_quic_core::{
    frame::{ack::AckRanges, Ack},
    packet::number::{PacketNumber, PacketNumberRange, PacketNumberSpace},
    recovery::RTTEstimator,
    time::Timestamp,
    transport::error::TransportError,
};

pub struct RecoveryManager {
    packet_space: PacketNumberSpace,
    rtt_estimator: RTTEstimator,
    // max_ack_delay:
    // The maximum amount of time by which the receiver intends to delay acknowledgments for packets in the ApplicationData packet number space. The actual ack_delay in a received ACK frame may be larger due to late timers, reordering, or lost ACK frames.
    // loss_detection_timer:
    // Multi-modal timer used for loss detection.
    // pto_count:
    // The number of times a PTO has been sent without receiving an ack.
    // time_of_last_ack_eliciting_packet[kPacketNumberSpace]:
    // The time the most recent ack-eliciting packet was sent.
    // largest_acked_packet[kPacketNumberSpace]:
    // The largest packet number acknowledged in the packet number space so far.
    largest_acked_packet: Option<PacketNumber>,

    // The time at which the next packet in that packet number space will be considered lost based on exceeding the reordering window in time.
    loss_time: Option<Timestamp>,

    // An association of packet numbers in a packet number space to information about them.
    sent_packets: SentPackets,
}

// Maximum reordering in packets before packet threshold loss detection considers a packet lost.
const K_PACKET_THRESHOLD: u8 = 3;

// Maximum reordering in time before time threshold loss detection considers a packet lost. Specified as an RTT multiplier.
const K_TIME_THRESHOLD: f32 = 9.0 / 8.0;

// Timer granularity
const K_GRANULARITY: Duration = Duration::from_millis(1);

impl RecoveryManager {
    //OnDatagramReceived

    //OnAckReceived(ack, pn_space)
    pub fn on_ack_received<A: AckRanges>(
        &mut self,
        ack: Ack<A>,
        acked_packets: PacketNumberRange,
    ) -> Result<(), TransportError> {
        if let Some(largest_acked_packet) = self.largest_acked_packet {
            self.largest_acked_packet = Some(max(largest_acked_packet, acked_packets.end()));
        } else {
            self.largest_acked_packet = Some(acked_packets.end());
        }

        // detect_and_remove_acked_packets finds packets that are newly
        // acknowledged and removes them from sent_packets.
        let newly_acked_packets = self.detect_and_remove_acked_packets(acked_packets);
        // Nothing to do if there are no newly acked packets.
        if newly_acked_packets.is_empty() {
            return Ok(());
        }

        let largest_newly_acked = newly_acked_packets
            .last()
            .expect("there must be at least one newly acked packet at this point");

        // If the largest acknowledged is newly acked and
        // at least one ack-eliciting was newly acked, update the RTT.
        if largest_newly_acked.0 == acked_packets.end() {
            let latest_rtt = s2n_quic_platform::time::now() - largest_newly_acked.1.time_sent;
            self.rtt_estimator.update_rtt(
                Duration::from_micros(ack.ack_delay.as_u64()),
                latest_rtt,
                largest_newly_acked.0.space(),
            );
        };

        // Process ECN information if present.
        if ack.ecn_counts.is_some() {
            // TODO: ProcessECN(ack, pn_space)
        }

        let lost_packets = self.detect_and_remove_lost_packets();
        if !lost_packets.is_empty() {
            // TODO: OnPacketsLost(lost_packets)
        }
        // TODO: OnPacketsAcked(newly_acked_packets)

        // Reset pto_count unless the client is unsure if
        // the server has validated the client's address.
        // TODO: if (PeerCompletedAddressValidation()):
        //     pto_count = 0
        // TODO: SetLossDetectionTimer()

        Ok(())
    }

    //SetLossDetectionTimer

    //OnLossDetectionTimeout

    //DetectAndRemoveLostPackets

    // Finds packets that are newly acknowledged and removes them from sent_packets.
    fn detect_and_remove_acked_packets(
        &mut self,
        acked_packets: PacketNumberRange,
    ) -> Vec<(PacketNumber, SentPacketInfo)> {
        let mut newly_acked_packets = Vec::new();

        for acked_packet in acked_packets {
            if let Some((packet_number, sent_packet_info)) = self.sent_packets.remove(acked_packet)
            {
                newly_acked_packets.push((packet_number, sent_packet_info));
            }
        }

        newly_acked_packets
    }

    /// detect_and_remove_lost_packets is called every time an ACK is received or the time threshold
    /// loss detection timer expires. This function operates on the sent_packets for that packet
    /// number space and returns a list of packets newly detected as lost.
    fn detect_and_remove_lost_packets(&mut self) -> Vec<PacketNumber> {
        let largest_acked_packet = &self
            .largest_acked_packet
            .expect("This function is only called after an ack has been received");
        self.loss_time = None;
        let mut lost_packets = Vec::new();
        let loss_delay = max(
            self.rtt_estimator.latest_rtt(),
            self.rtt_estimator.smoothed_rtt(),
        )
        .mul_f32(K_TIME_THRESHOLD);

        // Minimum time of K_GRANULARITY before packets are deemed lost.
        let loss_delay = max(loss_delay, K_GRANULARITY);

        // Packets sent before this time are deemed lost.
        let lost_send_time = s2n_quic_platform::time::now() - loss_delay;

        let mut sent_packets_to_remove = Vec::new();

        for (unacked_packet_number, unacked_sent_info) in self.sent_packets.iter() {
            if unacked_packet_number > largest_acked_packet {
                continue;
            }

            // Mark packet as lost, or set time when it should be marked.
            if unacked_sent_info.time_sent <= lost_send_time
                || largest_acked_packet
                    .checked_distance(*unacked_packet_number)
                    .expect("largest_acked_packet >= unacked_packet_number")
                    >= K_PACKET_THRESHOLD as u64
            {
                sent_packets_to_remove.push(*unacked_packet_number);

                if unacked_sent_info.in_flight {
                    lost_packets.push(*unacked_packet_number)
                }
            } else if self.loss_time.is_none() {
                self.loss_time = Some(unacked_sent_info.time_sent + loss_delay);
            } else {
                self.loss_time = self
                    .loss_time
                    .min(Some(unacked_sent_info.time_sent + loss_delay));
            }
        }

        for packet_number in sent_packets_to_remove {
            self.sent_packets.remove(packet_number);
        }

        lost_packets
    }
}

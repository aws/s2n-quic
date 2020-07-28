use crate::recovery::{SentPacketInfo, SentPackets};
use core::{cmp::max, ops::RangeInclusive, time::Duration};
use s2n_quic_core::{
    ack_set::AckSet,
    packet::number::{PacketNumber, PacketNumberSpace},
    recovery::RTTEstimator,
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
    // loss_time[kPacketNumberSpace]:
    // The time at which the next packet in that packet number space will be considered lost based on exceeding the reordering window in time.
    // sent_packets[kPacketNumberSpace]:
    // An association of packet numbers in a packet number space to information about them.
    sent_packets: SentPackets,
}

impl RecoveryManager {
    //OnDatagramReceived

    //OnAckReceived(ack, pn_space)
    pub fn on_ack_received(
        &mut self,
        acked_packets: RangeInclusive<PacketNumber>,
        ack_delay: Duration,
    ) -> Result<(), TransportError> {
        if let Some(largest_acked_packet) = self.largest_acked_packet {
            self.largest_acked_packet = Some(max(largest_acked_packet, acked_packets.largest()));
        } else {
            self.largest_acked_packet = Some(acked_packets.largest());
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
        if largest_newly_acked.0 == acked_packets.largest() {
            let latest_rtt = s2n_quic_platform::time::now() - largest_newly_acked.1.time_sent;
            self.rtt_estimator
                .update_rtt(ack_delay, latest_rtt, largest_newly_acked.0.space());
        };

        // Process ECN information if present.

        Ok(())
    }

    //SetLossDetectionTimer

    //OnLossDetectionTimeout

    //DetectAndRemoveLostPackets

    // Finds packets that are newly acknowledged and removes them from sent_packets.
    fn detect_and_remove_acked_packets(
        &mut self,
        acked_packets: RangeInclusive<PacketNumber>,
    ) -> Vec<(PacketNumber, SentPacketInfo)> {
        let mut newly_acked_packets = Vec::new();

        for acked_packet in acked_packets {
            if let Some((packet_number, sent_packet_info)) = self.sent_packets.remove(acked_packet)
            {
                newly_acked_packets.push((packet_number, sent_packet_info));
            }
        }
        //
        // let newly_acked_packets = self
        //     .sent_packets
        //     .range(ack_set.smallest()..=ack_set.largest())
        //     .map(|(pn, spi)| (pn.clone(), spi.clone()))
        //     .collect();
        //
        // for &(packet_number, _sent_packet_info) in &newly_acked_packets {
        //     self.sent_packets.remove(packet_number);
        // }

        newly_acked_packets
    }
}

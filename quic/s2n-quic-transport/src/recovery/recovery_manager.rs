// TODO: Remove when used
#![allow(dead_code)]

use crate::recovery::{SentPacketInfo, SentPackets};
use core::{cmp::max, time::Duration};
use s2n_quic_core::{
    frame::{ack::AckRanges, ack_elicitation::AckElicitation, Ack},
    inet::DatagramInfo,
    packet::number::{PacketNumber, PacketNumberRange, PacketNumberSpace},
    recovery::RTTEstimator,
    time::Timestamp,
};
use s2n_quic_platform::time;

pub struct RecoveryManager {
    // The packet number space this recovery manager is managing
    pn_space: PacketNumberSpace,

    // A round trip time estimator used for keeping track of estimated RTT
    rtt_estimator: RTTEstimator,

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#A.3
    //# The maximum amount of time by which the receiver intends to delay acknowledgments for packets
    //# in the ApplicationData packet number space. The actual ack_delay in a received ACK frame may
    //# be larger due to late timers, reordering, or lost ACK frames.
    max_ack_delay: Duration,

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#A.3
    //# The time the most recent ack-eliciting packet was sent.
    time_of_last_ack_eliciting_packet: Option<Timestamp>,

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#A.3
    //# The largest packet number acknowledged in the packet number space so far.
    largest_acked_packet: Option<PacketNumber>,

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#A.3
    //# The time at which the next packet in that packet number space will be considered lost based
    //# on exceeding the reordering window in time.
    loss_time: Option<Timestamp>,

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#A.3
    //# An association of packet numbers in a packet number space to information about them.
    //  These are packets that are pending acknowledgement.
    sent_packets: SentPackets,

    // True if calls to `on_ack_received` resulted in new packets being acknowledged. This is used
    // by `on_ack_received_finish` to determine what additional actions to take after processing an
    // ack frame. Calling `on_ack_received_finish` resets this to false.
    newly_acked: bool,
}

//= https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#A.2
//# Maximum reordering in packets before packet threshold loss detection considers a packet lost.
const K_PACKET_THRESHOLD: u8 = 3;

//= https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#A.2
//# Maximum reordering in time before time threshold loss detection considers a packet lost.
//# Specified as an RTT multiplier.
const K_TIME_THRESHOLD: f32 = 9.0 / 8.0;

//= https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#A.2
//# Timer granularity.
const K_GRANULARITY: Duration = Duration::from_millis(1);

type SentPacket = (PacketNumber, SentPacketInfo);

impl RecoveryManager {
    /// Constructs a new `RecoveryManager` for the given `PacketNumberSpace`
    pub fn new(
        pn_space: PacketNumberSpace,
        rtt_estimator: RTTEstimator,
        max_ack_delay: Duration,
    ) -> Self {
        Self {
            pn_space,
            rtt_estimator,
            max_ack_delay,
            time_of_last_ack_eliciting_packet: None,
            largest_acked_packet: None,
            loss_time: None,
            sent_packets: SentPackets::default(),
            newly_acked: false,
        }
    }

    #[allow(clippy::collapsible_if)]
    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#A.5
    //# After a packet is sent, information about the packet is stored.
    pub fn on_packet_sent(
        &mut self,
        packet_number: PacketNumber,
        ack_elicitation: AckElicitation,
        in_flight: bool,
        sent_bytes: u64,
    ) {
        let time_sent = time::now();

        if ack_elicitation.is_ack_eliciting() {
            let sent_packet_info = SentPacketInfo::new(in_flight, sent_bytes, time_sent);
            self.sent_packets.insert(packet_number, sent_packet_info);
        }

        if in_flight {
            if ack_elicitation.is_ack_eliciting() {
                self.time_of_last_ack_eliciting_packet = Some(time_sent);
            }
            // TODO: self.congestion_controller.on_packet_sent_cc(sent_bytes)
            // TODO: self.loss_detection_timer.set()
        }
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#A.6
    //# When a server is blocked by anti-amplification limits, receiving a datagram unblocks it,
    //# even if none of the packets in the datagram are successfully processed. In such a case,
    //# the PTO timer will need to be re-armed
    pub fn on_datagram_received(_datagram: DatagramInfo) {
        // If this datagram unblocks the server, arm the
        // PTO timer to avoid deadlock.
        // TODO: if (server was at anti-amplification limit):
        //          self.loss_detection_timer.set()
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#A.7
    //# When an ACK frame is received, it may newly acknowledge any number of packets.
    pub fn on_ack_received(
        &mut self,
        acked_packets: PacketNumberRange,
        largest_acked: PacketNumber,
        ack_delay: Duration,
    ) {
        let largest_newly_acked = self.sent_packets.range(acked_packets).last();

        // Nothing to do if there are no newly acked packets.
        if largest_newly_acked.is_none() {
            return;
        }

        // There are newly acked packets, so set new_acked to true for use in on_ack_received_finish
        self.newly_acked = true;

        let largest_newly_acked = largest_newly_acked
            .expect("there must be at least one newly acked packet at this point");

        if let Some(largest_acked_packet) = self.largest_acked_packet {
            self.largest_acked_packet = Some(max(largest_acked_packet, *largest_newly_acked.0));
        } else {
            self.largest_acked_packet = Some(*largest_newly_acked.0);
        }

        // If the largest acknowledged is newly acked and
        // at least one ack-eliciting was newly acked, update the RTT.
        if *largest_newly_acked.0 == largest_acked {
            let latest_rtt = time::now() - largest_newly_acked.1.time_sent;
            self.rtt_estimator
                .update_rtt(ack_delay, latest_rtt, largest_newly_acked.0.space());
        };

        // TODO: self.congestion_controller.on_packets_acked(self.sent_packets.range(acked_packets));

        // TODO: Investigate a more efficient mechanism for managing sent_packets
        //       See https://github.com/awslabs/s2n-quic/issues/69
        let acked_packets_to_remove: Vec<PacketNumber> = self
            .sent_packets
            .range(acked_packets)
            .map(|p| p.0)
            .cloned()
            .collect();

        for packet_number in acked_packets_to_remove {
            self.sent_packets.remove(packet_number);
        }
    }

    /// Finishes processing an ack frame. This should be called after on_ack_received has
    /// been called for each range of packets being acknowledged in the ack frame.
    pub fn on_ack_received_finish<A: AckRanges>(&mut self, ack: Ack<A>) {
        if self.newly_acked {
            // Process ECN information if present.
            if ack.ecn_counts.is_some() {
                // TODO: self.congestion_controller.process_ecn(ack, pn_space)
            }

            let lost_packets = self.detect_and_remove_lost_packets();
            if !lost_packets.is_empty() {
                // TODO: self.congestion_controller.on_packets_lost(lost_packets)
            }

            // Reset pto_count unless the client is unsure if
            // the server has validated the client's address.
            if self.peer_completed_address_validation() {
                // TODO: self.loss_detection_timer.set()
                // TODO: pto_count = 0;
            }
            // TODO: self.loss_detection_timer.set()

            self.newly_acked = false;
        }
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#A.9
    //# When the loss detection timer expires, the timer's mode determines the action to be performed.
    fn on_loss_detection_timeout(&mut self) {
        // earliest_loss_time = loss_detection_timer.get_loss_time_and_space();
        // if earliest_loss_time.is_some() {
        //     // Time threshold loss detection
        //     let lost_packets = self.detect_and_remove_lost_packets();
        //     assert!(!lost_packets.is_empty());
        //     // self.congestion_controller.on_packets_lost(lost_packets);
        //     // self.loss_detection_timer.set();
        //     return;
        // }

        // if self.congestion_controller.bytes_in_flight() > 0 {
        // PTO. Send new data if available, else retransmit old data.
        // If neither is available, send a single PING frame.
        // _, pn_space = loss_detection_timer.get_pto_time_and_space();
        // send_one_or_two_ack_eliciting_packets(pn_space)
        // else {
        // TODO: implement client
        // }

        // self.lost_detection_timer.increment_pto_count();
        // self.lost_detection_timer.set();
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#A.10
    //# DetectAndRemoveLostPackets is called every time an ACK is received or the time threshold
    //# loss detection timer expires. This function operates on the sent_packets for that packet
    //# number space and returns a list of packets newly detected as lost.
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
        let lost_send_time = time::now() - loss_delay;

        // TODO: Investigate a more efficient mechanism for managing sent_packets
        //       See https://github.com/awslabs/s2n-quic/issues/69
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

    fn peer_completed_address_validation(&self) -> bool {
        true
        // TODO: Implement client
        // Assume clients validate the server's address implicitly.
        // if (endpoint is server):
        // return true
        // Servers complete address validation when a
        // protected packet is received.
        // return has received Handshake ACK ||
        //     has received 1-RTT ACK ||
        //     has received HANDSHAKE_DONE
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use core::time::Duration;
    use s2n_quic_core::{time::Clock, varint::VarInt};
    use s2n_quic_platform::time::testing;
    use std::sync::Arc;

    #[compliance::tests("https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#A.5")]
    #[test]
    fn on_packet_sent() {
        let clock = Arc::new(testing::MockClock::new());
        testing::set_local_clock(clock.clone());
        let pn_space = PacketNumberSpace::ApplicationData;
        let rtt_estimator = RTTEstimator::new(Duration::from_millis(10));

        let mut recovery_manager =
            RecoveryManager::new(pn_space, rtt_estimator, Duration::from_millis(100));

        for i in 1..=10 {
            let sent_packet = pn_space.new_packet_number(VarInt::from_u8(i));
            let ack_elicitation = {
                if i % 2 == 0 {
                    AckElicitation::Eliciting
                } else {
                    AckElicitation::NonEliciting
                }
            };
            let in_flight = { i % 3 == 0 };
            let sent_bytes = (2 * i) as u64;

            recovery_manager.on_packet_sent(sent_packet, ack_elicitation, in_flight, sent_bytes);

            if ack_elicitation == AckElicitation::Eliciting {
                assert!(recovery_manager.sent_packets.get(sent_packet).is_some());
                let actual_sent_packet = recovery_manager.sent_packets.get(sent_packet).unwrap();
                assert_eq!(actual_sent_packet.sent_bytes, sent_bytes);
                assert_eq!(actual_sent_packet.in_flight, in_flight);
                assert_eq!(actual_sent_packet.time_sent, clock.get_time())
            } else {
                assert!(recovery_manager.sent_packets.get(sent_packet).is_none());
            }

            clock.adjust_by(Duration::from_millis(10));
        }

        assert_eq!(recovery_manager.sent_packets.iter().count(), 5);
    }

    #[compliance::tests("https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#A.7")]
    #[test]
    fn on_ack_received() {
        let clock = Arc::new(testing::MockClock::new());
        testing::set_local_clock(clock.clone());
        let pn_space = PacketNumberSpace::ApplicationData;
        let rtt_estimator = RTTEstimator::new(Duration::from_millis(10));

        let mut recovery_manager =
            RecoveryManager::new(pn_space, rtt_estimator, Duration::from_millis(100));

        let packets = PacketNumberRange::new(
            pn_space.new_packet_number(VarInt::from_u8(1)),
            pn_space.new_packet_number(VarInt::from_u8(10)),
        );

        for packet in packets {
            recovery_manager.on_packet_sent(packet, AckElicitation::Eliciting, true, 128);
        }

        assert_eq!(recovery_manager.sent_packets.iter().count(), 10);

        clock.adjust_by(Duration::from_millis(500));

        let acked_packets = PacketNumberRange::new(
            pn_space.new_packet_number(VarInt::from_u8(4)),
            pn_space.new_packet_number(VarInt::from_u8(7)),
        );

        recovery_manager.on_ack_received(
            acked_packets,
            pn_space.new_packet_number(VarInt::from_u8(9)),
            Duration::from_millis(10),
        );

        // The largest packet wasn't part of this call so the RTT is not updated
        assert_eq!(
            recovery_manager.rtt_estimator.latest_rtt(),
            Duration::from_millis(0)
        );

        assert_eq!(recovery_manager.sent_packets.iter().count(), 6);
        for packet in acked_packets {
            assert!(recovery_manager.sent_packets.get(packet).is_none());
        }
        assert_eq!(
            recovery_manager.largest_acked_packet.unwrap(),
            acked_packets.end()
        );

        // Acknowledging already acked packets does nothing
        recovery_manager.on_ack_received(
            PacketNumberRange::new(
                pn_space.new_packet_number(VarInt::from_u8(4)),
                pn_space.new_packet_number(VarInt::from_u8(7)),
            ),
            pn_space.new_packet_number(VarInt::from_u8(9)),
            Duration::from_millis(10),
        );

        let acked_packets = PacketNumberRange::new(
            pn_space.new_packet_number(VarInt::from_u8(8)),
            pn_space.new_packet_number(VarInt::from_u8(9)),
        );

        recovery_manager.on_ack_received(
            acked_packets,
            pn_space.new_packet_number(VarInt::from_u8(9)),
            Duration::from_millis(10),
        );

        // Now the largest packet number has been acked so the RTT is updated
        assert_eq!(
            recovery_manager.rtt_estimator.latest_rtt(),
            Duration::from_millis(500)
        );
    }
}

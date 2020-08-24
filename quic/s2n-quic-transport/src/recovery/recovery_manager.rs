// TODO: Remove when used
#![allow(dead_code)]

use crate::recovery::{SentPacketInfo, SentPackets};
use core::{cmp::max, time::Duration};
use s2n_quic_core::{
    frame::{ack::ECNCounts, ack_elicitation::AckElicitation},
    inet::DatagramInfo,
    packet::number::{PacketNumber, PacketNumberRange},
    recovery::RTTEstimator,
    time::Timestamp,
};

pub struct RecoveryManager {
    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#A.3
    //# The maximum amount of time by which the receiver intends to delay acknowledgments for packets
    //# in the ApplicationData packet number space. The actual ack_delay in a received ACK frame may
    //# be larger due to late timers, reordering, or lost ACK frames.
    max_ack_delay: Duration,

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#A.3
    //# The largest packet number acknowledged in the packet number space so far.
    largest_acked_packet: Option<PacketNumber>,

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
//# The value recommended in Section 6.1.1 is 3.
const K_PACKET_THRESHOLD: u64 = 3;

//= https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#A.2
//# Timer granularity. This is a system-dependent value, and Section 6.1.2 recommends a value of 1ms.
pub(crate) const K_GRANULARITY: Duration = Duration::from_millis(1);

type SentPacket = (PacketNumber, SentPacketInfo);

impl RecoveryManager {
    /// Constructs a new `RecoveryManager`
    pub fn new(max_ack_delay: Duration) -> Self {
        Self {
            max_ack_delay,
            largest_acked_packet: None,
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
        time_sent: Timestamp,
        time_of_last_ack_eliciting_packet: &mut Option<Timestamp>,
    ) {
        if ack_elicitation.is_ack_eliciting() {
            self.sent_packets.insert(
                packet_number,
                SentPacketInfo::new(in_flight, sent_bytes, time_sent),
            );
        }

        if in_flight {
            if ack_elicitation.is_ack_eliciting() {
                *time_of_last_ack_eliciting_packet = Some(time_sent);
            }
            // TODO: self.congestion_controller.on_packet_sent_cc(sent_bytes)
            // The loss detection timer is set after packets are sent in ConnectionImpl.on_transmit.
            // This differs from the pseudo-code in Appendix A.5, which sets the timer after every
            // packet, in order to reduce multiple calls to the loss detection timer.
        }
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#A.6
    //# When a server is blocked by anti-amplification limits, receiving a datagram unblocks it,
    //# even if none of the packets in the datagram are successfully processed. In such a case,
    //# the PTO timer will need to be re-armed
    pub fn on_datagram_received(_datagram: DatagramInfo, at_anti_amplification_limit: bool) {
        // If this datagram unblocks the server, arm the
        // PTO timer to avoid deadlock.
        if at_anti_amplification_limit {
            // TODO self.loss_detection_timer.set()
        }
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#A.7
    //# When an ACK frame is received, it may newly acknowledge any number of packets.
    pub fn on_ack_received(
        &mut self,
        acked_packets: PacketNumberRange,
        largest_acked: PacketNumber,
        ack_delay: Duration,
        ecn_counts: Option<ECNCounts>,
        receive_time: Timestamp,
        rtt_estimator: &mut RTTEstimator,
    ) {
        let largest_newly_acked = if let Some(last) = self.sent_packets.range(acked_packets).last()
        {
            // There are newly acked packets, so set newly_acked to true for use in on_ack_received_finish
            self.newly_acked = true;
            last
        } else {
            // Nothing to do if there are no newly acked packets.
            return;
        };

        self.largest_acked_packet = Some(
            self.largest_acked_packet
                .map_or(*largest_newly_acked.0, |pn| pn.max(*largest_newly_acked.0)),
        );

        // If the largest acknowledged is newly acked and
        // at least one ack-eliciting was newly acked, update the RTT.
        if *largest_newly_acked.0 == largest_acked {
            let latest_rtt = receive_time - largest_newly_acked.1.time_sent;
            rtt_estimator.update_rtt(ack_delay, latest_rtt, largest_acked.space());

            // Process ECN information if present.
            if ecn_counts.is_some() {
                // TODO: self.congestion_controller.process_ecn(ecn_counts, largest_newly_acked, largest_acked.space())
            }
        };

        // TODO: self.congestion_controller.on_packets_acked(self.sent_packets.range(acked_packets));

        for packet_number in acked_packets {
            self.sent_packets.remove(packet_number);
        }
    }

    /// Finishes processing an ack frame. This should be called after on_ack_received has
    /// been called for each range of packets being acknowledged in the ack frame.
    pub fn on_ack_received_finish(
        &mut self,
        receive_time: Timestamp,
        peer_completed_address_validation: bool,
        rtt_estimator: &RTTEstimator,
        loss_time: &mut Option<Timestamp>,
    ) {
        if self.newly_acked {
            let lost_packets = self.detect_and_remove_lost_packets(
                rtt_estimator.latest_rtt(),
                rtt_estimator.smoothed_rtt(),
                receive_time,
                loss_time,
            );
            if !lost_packets.is_empty() {
                // TODO: self.congestion_controller.on_packets_lost(lost_packets)
            }

            // Reset pto_count unless the client is unsure if
            // the server has validated the client's address.
            if peer_completed_address_validation {
                // TODO: pto_count = 0;
            }
            // TODO: self.loss_detection_timer.set()

            self.newly_acked = false;
        }
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#A.10
    //# DetectAndRemoveLostPackets is called every time an ACK is received or the time threshold
    //# loss detection timer expires. This function operates on the sent_packets for that packet
    //# number space and returns a list of packets newly detected as lost.
    pub fn detect_and_remove_lost_packets(
        &mut self,
        latest_rtt: Duration,
        smoothed_rtt: Duration,
        now: Timestamp,
        loss_time: &mut Option<Timestamp>,
    ) -> Vec<PacketNumber> {
        *loss_time = None;
        let loss_delay = self.calculate_loss_delay(latest_rtt, smoothed_rtt);

        // Packets sent before this time are deemed lost.
        let lost_send_time = now - loss_delay;

        // TODO: Investigate a more efficient mechanism for managing sent_packets
        //       See https://github.com/awslabs/s2n-quic/issues/69
        let mut sent_packets_to_remove = Vec::new();

        let largest_acked_packet = &self
            .largest_acked_packet
            .expect("This function is only called after an ack has been received");
        let mut lost_packets = Vec::new();

        for (unacked_packet_number, unacked_sent_info) in self.sent_packets.iter() {
            if unacked_packet_number > largest_acked_packet {
                // sent_packets is ordered by packet number, so all remaining packets will be larger
                break;
            }

            // Mark packet as lost, or set time when it should be marked.
            if unacked_sent_info.time_sent <= lost_send_time
                || largest_acked_packet
                    .checked_distance(*unacked_packet_number)
                    .expect("largest_acked_packet >= unacked_packet_number")
                    >= K_PACKET_THRESHOLD
            {
                sent_packets_to_remove.push(*unacked_packet_number);

                if unacked_sent_info.in_flight {
                    lost_packets.push(*unacked_packet_number)
                }
            } else {
                *loss_time = Some(unacked_sent_info.time_sent + loss_delay);
                // assuming sent_packets is ordered by packet number and sent time, all remaining
                // packets will have a larger packet number and sent time, and are thus not lost.
                break;
            }
        }

        for packet_number in sent_packets_to_remove {
            self.sent_packets.remove(packet_number);
        }

        lost_packets
    }

    fn calculate_loss_delay(&self, latest_rtt: Duration, smoothed_rtt: Duration) -> Duration {
        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#A.2
        // 9/8 is the K_TIME_THRESHOLD, the maximum reordering in time
        // before time threshold loss detection considers a packet lost.
        let loss_delay = max(latest_rtt, smoothed_rtt) * 9 / 8;

        // Minimum time of K_GRANULARITY before packets are deemed lost.
        max(loss_delay, K_GRANULARITY)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use core::time::Duration;
    use s2n_quic_core::{packet::number::PacketNumberSpace, varint::VarInt};

    #[compliance::tests("https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#A.5")]
    #[test]
    fn on_packet_sent() {
        let pn_space = PacketNumberSpace::ApplicationData;
        let mut recovery_manager = RecoveryManager::new(Duration::from_millis(100));
        let mut time_sent = s2n_quic_platform::time::now();

        for i in 1..=10 {
            let sent_packet = pn_space.new_packet_number(VarInt::from_u8(i));
            let ack_elicitation = if i % 2 == 0 {
                AckElicitation::Eliciting
            } else {
                AckElicitation::NonEliciting
            };
            let in_flight = i % 3 == 0;
            let sent_bytes = (2 * i) as u64;
            let mut time_of_last_ack_eliciting_packet = None;

            recovery_manager.on_packet_sent(
                sent_packet,
                ack_elicitation,
                in_flight,
                sent_bytes,
                time_sent,
                &mut time_of_last_ack_eliciting_packet,
            );

            if ack_elicitation == AckElicitation::Eliciting {
                assert!(recovery_manager.sent_packets.get(sent_packet).is_some());
                let actual_sent_packet = recovery_manager.sent_packets.get(sent_packet).unwrap();
                assert_eq!(actual_sent_packet.sent_bytes, sent_bytes);
                assert_eq!(actual_sent_packet.in_flight, in_flight);
                assert_eq!(actual_sent_packet.time_sent, time_sent);
                if in_flight {
                    assert_eq!(Some(time_sent), time_of_last_ack_eliciting_packet);
                }
            } else {
                assert!(recovery_manager.sent_packets.get(sent_packet).is_none());
            }

            time_sent += Duration::from_millis(10);
        }

        assert_eq!(recovery_manager.sent_packets.iter().count(), 5);
    }

    #[compliance::tests("https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#A.7")]
    #[test]
    fn on_ack_received() {
        let pn_space = PacketNumberSpace::ApplicationData;
        let mut rtt_estimator = RTTEstimator::new(Duration::from_millis(10));
        let mut recovery_manager = RecoveryManager::new(Duration::from_millis(100));

        let packets = PacketNumberRange::new(
            pn_space.new_packet_number(VarInt::from_u8(1)),
            pn_space.new_packet_number(VarInt::from_u8(10)),
        );

        let time_sent = s2n_quic_platform::time::now() + Duration::from_secs(10);
        let mut time_of_last_ack_eliciting_packet = None;

        for packet in packets {
            recovery_manager.on_packet_sent(
                packet,
                AckElicitation::Eliciting,
                true,
                128,
                time_sent,
                &mut time_of_last_ack_eliciting_packet,
            );
        }

        assert_eq!(recovery_manager.sent_packets.iter().count(), 10);

        let ack_receive_time = time_sent + Duration::from_millis(500);

        let acked_packets = PacketNumberRange::new(
            pn_space.new_packet_number(VarInt::from_u8(4)),
            pn_space.new_packet_number(VarInt::from_u8(7)),
        );

        recovery_manager.on_ack_received(
            acked_packets,
            pn_space.new_packet_number(VarInt::from_u8(9)),
            Duration::from_millis(10),
            None,
            ack_receive_time,
            &mut rtt_estimator,
        );

        // The largest packet wasn't part of this call so the RTT is not updated
        assert_eq!(rtt_estimator.latest_rtt(), Duration::from_millis(0));

        assert_eq!(recovery_manager.sent_packets.iter().count(), 6);
        for packet in acked_packets {
            assert!(recovery_manager.sent_packets.get(packet).is_none());
        }
        assert_eq!(
            recovery_manager.largest_acked_packet,
            Some(acked_packets.end())
        );

        // Acknowledging already acked packets does nothing
        recovery_manager.on_ack_received(
            PacketNumberRange::new(
                pn_space.new_packet_number(VarInt::from_u8(4)),
                pn_space.new_packet_number(VarInt::from_u8(7)),
            ),
            pn_space.new_packet_number(VarInt::from_u8(9)),
            Duration::from_millis(10),
            None,
            ack_receive_time,
            &mut rtt_estimator,
        );

        let acked_packets = PacketNumberRange::new(
            pn_space.new_packet_number(VarInt::from_u8(8)),
            pn_space.new_packet_number(VarInt::from_u8(9)),
        );

        recovery_manager.on_ack_received(
            acked_packets,
            pn_space.new_packet_number(VarInt::from_u8(9)),
            Duration::from_millis(10),
            None,
            ack_receive_time,
            &mut rtt_estimator,
        );

        // Now the largest packet number has been acked so the RTT is updated
        assert_eq!(rtt_estimator.latest_rtt(), Duration::from_millis(500));

        assert!(recovery_manager.newly_acked);
        recovery_manager.on_ack_received_finish(ack_receive_time, true, &rtt_estimator, &mut None);
        assert!(!recovery_manager.newly_acked);
    }

    #[compliance::tests("https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#A.10")]
    #[test]
    fn detect_and_remove_lost_packets() {
        let pn_space = PacketNumberSpace::ApplicationData;
        let mut rtt_estimator = RTTEstimator::new(Duration::from_millis(10));

        let mut recovery_manager = RecoveryManager::new(Duration::from_millis(100));

        recovery_manager.largest_acked_packet =
            Some(pn_space.new_packet_number(VarInt::from_u8(10)));

        let mut time_sent = s2n_quic_platform::time::now();
        let mut time_of_last_ack_eliciting_packet = None;

        // Send a packet that was sent too long ago (lost)
        let old_packet_time_sent = pn_space.new_packet_number(VarInt::from_u8(8));
        recovery_manager.on_packet_sent(
            old_packet_time_sent,
            AckElicitation::Eliciting,
            true,
            1,
            time_sent,
            &mut time_of_last_ack_eliciting_packet,
        );

        time_sent += Duration::from_secs(10);

        //Send a packet with a packet number K_PACKET_THRESHOLD away from the largest (lost)
        let old_packet_packet_number =
            pn_space.new_packet_number(VarInt::new(10 - K_PACKET_THRESHOLD).unwrap());
        recovery_manager.on_packet_sent(
            old_packet_packet_number,
            AckElicitation::Eliciting,
            true,
            1,
            time_sent,
            &mut time_of_last_ack_eliciting_packet,
        );

        // Send a packet that is less than the largest acked but not lost
        let not_lost = pn_space.new_packet_number(VarInt::from_u8(9));
        recovery_manager.on_packet_sent(
            not_lost,
            AckElicitation::Eliciting,
            true,
            1,
            time_sent,
            &mut time_of_last_ack_eliciting_packet,
        );

        // Send a packet larger than the largest acked (not lost)
        let larger_than_largest = recovery_manager
            .largest_acked_packet
            .unwrap()
            .next()
            .unwrap();
        recovery_manager.on_packet_sent(
            larger_than_largest,
            AckElicitation::Eliciting,
            true,
            1,
            time_sent,
            &mut time_of_last_ack_eliciting_packet,
        );

        rtt_estimator.update_rtt(
            Duration::from_millis(10),
            Duration::from_millis(150),
            pn_space,
        );

        let now = time_sent;
        let loss_time = &mut None;

        let lost_packets = recovery_manager.detect_and_remove_lost_packets(
            rtt_estimator.latest_rtt(),
            rtt_estimator.smoothed_rtt(),
            now,
            loss_time,
        );
        let sent_packets = &recovery_manager.sent_packets;
        assert!(lost_packets.contains(&old_packet_time_sent));
        assert!(sent_packets.get(old_packet_time_sent).is_none());

        assert!(lost_packets.contains(&old_packet_packet_number));
        assert!(sent_packets.get(old_packet_packet_number).is_none());

        assert!(!lost_packets.contains(&larger_than_largest));
        assert!(sent_packets.get(larger_than_largest).is_some());

        assert!(!lost_packets.contains(&not_lost));
        assert!(sent_packets.get(not_lost).is_some());

        let expected_loss_time = Some(
            sent_packets.get(not_lost).unwrap().time_sent + rtt_estimator.latest_rtt() * 9 / 8,
        );
        assert!(loss_time.is_some());
        assert_eq!(expected_loss_time, *loss_time);
    }
}

// TODO: Remove when used
#![allow(dead_code)]

use crate::recovery::{SentPacketInfo, SentPackets};
use core::{cmp::max, time::Duration};
use s2n_quic_core::{
    frame::{ack::AckRanges, Ack},
    inet::DatagramInfo,
    packet::number::{PacketNumber, PacketNumberRange, PacketNumberSpace},
    recovery::RTTEstimator,
    time::Timestamp,
    transport::error::TransportError,
};

pub struct RecoveryManager {
    pn_space: PacketNumberSpace,

    rtt_estimator: RTTEstimator,

    // The maximum amount of time by which the receiver intends to delay acknowledgments for packets
    // in the ApplicationData packet number space. The actual ack_delay in a received ACK frame may
    // be larger due to late timers, reordering, or lost ACK frames.
    max_ack_delay: Duration,

    // The time the most recent ack-eliciting packet was sent.
    time_of_last_ack_eliciting_packet: Option<Timestamp>,

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
        }
    }

    /// After a packet is sent, information about the packet is stored.
    pub fn on_packet_sent(
        &mut self,
        packet_number: PacketNumber,
        ack_eliciting: bool,
        in_flight: bool,
        sent_bytes: u64,
    ) {
        if ack_eliciting {
            let sent_packet_info =
                SentPacketInfo::new(in_flight, sent_bytes, s2n_quic_platform::time::now());
            self.sent_packets.insert(packet_number, sent_packet_info);

            if in_flight {
                self.time_of_last_ack_eliciting_packet = Some(sent_packet_info.time_sent);
            }
        }

        if in_flight {
            // TODO: self.congestion_controller.on_packet_sent_cc(sent_bytes)
            // TODO: self.loss_detection_timer.set()
        }
    }

    /// When a server is blocked by anti-amplification limits, receiving a datagram unblocks it,
    /// even if none of the packets in the datagram are successfully processed. In such a case,
    /// the PTO timer will need to be re-armed
    pub fn on_datagram_received(_datagram: DatagramInfo) {
        // If this datagram unblocks the server, arm the
        // PTO timer to avoid deadlock.
        // TODO: if (server was at anti-amplification limit):
        //          self.loss_detection_timer.set()
    }

    /// When an ACK frame is received, it may newly acknowledge any number of packets.
    pub fn on_ack_received<A: AckRanges>(&mut self, ack: Ack<A>) -> Result<(), TransportError> {
        let largest_acked = self.pn_space.new_packet_number(ack.largest_acked());

        if let Some(largest_acked_packet) = self.largest_acked_packet {
            self.largest_acked_packet = Some(max(largest_acked_packet, largest_acked));
        } else {
            self.largest_acked_packet = Some(largest_acked);
        }

        // detect_and_remove_acked_packets finds packets that are newly
        // acknowledged and removes them from sent_packets.
        let (largest_newly_acked, newly_acked_packets) = self.detect_and_remove_acked_packets(&ack);
        // Nothing to do if there are no newly acked packets.
        if newly_acked_packets.is_empty() {
            return Ok(());
        }

        let largest_newly_acked = largest_newly_acked
            .expect("there must be at least one newly acked packet at this point");

        // If the largest acknowledged is newly acked and
        // at least one ack-eliciting was newly acked, update the RTT.
        if largest_newly_acked.0 == largest_acked {
            let latest_rtt = s2n_quic_platform::time::now() - largest_newly_acked.1.time_sent;
            self.rtt_estimator.update_rtt(
                Duration::from_micros(ack.ack_delay.as_u64()),
                latest_rtt,
                largest_newly_acked.0.space(),
            );
        };

        // Process ECN information if present.
        if ack.ecn_counts.is_some() {
            // TODO: self.congestion_controller.process_ecn(ack, pn_space)
        }

        let lost_packets = self.detect_and_remove_lost_packets();
        if !lost_packets.is_empty() {
            // TODO: self.congestion_controller.on_packets_lost(lost_packets)
        }
        // TODO: self.congestion_controller.on_packets_acked(newly_acked_packets)

        // Reset pto_count unless the client is unsure if
        // the server has validated the client's address.
        if self.peer_completed_address_validation() {
            // TODO: self.loss_detection_timer.set()
            // TODO: pto_count = 0;
        }
        // TODO: self.loss_detection_timer.set()

        Ok(())
    }

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

    // Finds packets that are newly acknowledged and removes them from sent_packets.
    fn detect_and_remove_acked_packets<A: AckRanges>(
        &mut self,
        ack: &Ack<A>,
    ) -> (
        Option<(PacketNumber, SentPacketInfo)>,
        Vec<(PacketNumber, SentPacketInfo)>,
    ) {
        let mut newly_acked_packets = Vec::new();
        let mut largest_newly_acked: Option<(PacketNumber, SentPacketInfo)> = None;

        for ack_range in ack.ack_ranges() {
            let start = self.pn_space.new_packet_number(*ack_range.start());
            let end = self.pn_space.new_packet_number(*ack_range.end());

            for acked_packet in PacketNumberRange::new(start, end) {
                if let Some((packet_number, sent_packet_info)) =
                    self.sent_packets.remove(acked_packet)
                {
                    newly_acked_packets.push((packet_number, sent_packet_info));

                    if largest_newly_acked.map_or(true, |a| a.0 < acked_packet) {
                        largest_newly_acked = Some((packet_number, sent_packet_info));
                    }
                }
            }
        }

        (largest_newly_acked, newly_acked_packets)
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

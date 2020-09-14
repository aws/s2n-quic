// TODO: Remove when used
#![allow(dead_code)]

use crate::{
    contexts::WriteContext,
    frame_exchange_interests::{FrameExchangeInterestProvider, FrameExchangeInterests},
    recovery::{SentPacketInfo, SentPackets},
    timer::VirtualTimer,
};
use core::{cmp::max, time::Duration};
use s2n_quic_core::{
    endpoint::EndpointType,
    frame::{self, ack::ECNCounts, ack_elicitation::AckElicitation},
    inet::DatagramInfo,
    packet::number::{PacketNumber, PacketNumberRange, PacketNumberSpace},
    path::Path,
    recovery::RTTEstimator,
    time::Timestamp,
    transport::error::TransportError,
};

#[derive(Debug)]
pub struct Manager {
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

    loss_timer: VirtualTimer,
    time_threshold: Duration,

    pto: Pto,

    bytes_in_flight: u64,

    time_of_last_ack_eliciting_packet: Option<Timestamp>,
}

//= https://tools.ietf.org/id/draft-ietf-quic-recovery-30.txt#6.1.1
//# The RECOMMENDED initial value for the packet reordering threshold
//# (kPacketThreshold) is 3, based on best practices for TCP loss
//# detection ([RFC5681], [RFC6675]).  In order to remain similar to TCP,
//# implementations SHOULD NOT use a packet threshold less than 3; see
//# [RFC5681].
const K_PACKET_THRESHOLD: u64 = 3;

fn apply_k_time_threshold(duration: Duration) -> Duration {
    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#6.1.2
    //# The RECOMMENDED time threshold (kTimeThreshold), expressed as a
    //# round-trip time multiplier, is 9/8.
    duration * 9 / 8
}

//= https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#6.1.2
//# The RECOMMENDED value of the
//# timer granularity (kGranularity) is 1ms.
pub(crate) const K_GRANULARITY: Duration = Duration::from_millis(1);

type SentPacket = (PacketNumber, SentPacketInfo);

#[must_use = "Ignoring loss information would lead to permanent data loss"]
#[derive(Copy, Clone, Default)]
pub struct LossInfo {
    /// Lost bytes in flight
    pub bytes_in_flight: u64,

    /// A PTO timer expired
    pub pto_expired: bool,

    /// The PTO count should be reset
    pub pto_reset: bool,
}

impl LossInfo {
    pub fn should_update(&self) -> bool {
        self.bytes_in_flight > 0 || self.pto_expired || self.pto_reset
    }
}

impl core::ops::Add for LossInfo {
    type Output = Self;

    fn add(self, rhs: Self) -> Self {
        Self {
            bytes_in_flight: self.bytes_in_flight + rhs.bytes_in_flight,
            pto_expired: self.pto_expired || rhs.pto_expired,
            pto_reset: self.pto_reset || rhs.pto_reset,
        }
    }
}

impl core::ops::AddAssign for LossInfo {
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
}

#[derive(Debug, Default)]
pub struct Pto {
    timer: VirtualTimer,
    state: PtoState,
    max_ack_delay: Duration,
}

#[derive(Debug)]
enum PtoState {
    Idle,
    RequiresTransmission(u8),
}

impl Default for PtoState {
    fn default() -> Self {
        Self::Idle
    }
}

impl Pto {
    pub fn new(max_ack_delay: Duration) -> Self {
        Self {
            max_ack_delay,
            ..Self::default()
        }
    }

    pub fn timers(&self) -> impl Iterator<Item = &Timestamp> {
        self.timer.iter()
    }

    /// Called when a timeout has occurred. Returns true if the PTO timer had expired.
    pub fn on_timeout(&mut self, bytes_in_flight: u64, timestamp: Timestamp) -> bool {
        if self.timer.poll_expiration(timestamp).is_ready() {
            //= https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#6.2.4
            //# When a PTO timer expires, a sender MUST send at least one ack-
            //# eliciting packet in the packet number space as a probe

            //= https://tools.ietf.org/id/draft-ietf-quic-recovery-30.txt#6.2.2.1
            //# Since the server could be blocked until more datagrams are received
            //# from the client, it is the client's responsibility to send packets to
            //# unblock the server until it is certain that the server has finished
            //# its address validation
            let transmission_count = if bytes_in_flight > 0 { 2 } else { 1 };

            //= https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#6.2.4
            //# An endpoint MAY send up to two full-
            //# sized datagrams containing ack-eliciting packets

            //= https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#6.2.4
            //# Sending two packets on PTO
            //# expiration increases resilience to packet drops, thus reducing the
            //# probability of consecutive PTO events.

            self.state = PtoState::RequiresTransmission(transmission_count);
            true
        } else {
            false
        }
    }

    /// Queries the component for any outgoing frames that need to get sent
    pub fn on_transmit<W: WriteContext>(&mut self, context: &mut W) {
        match self.state {
            PtoState::RequiresTransmission(0) => self.state = PtoState::Idle,
            PtoState::RequiresTransmission(remaining) => {
                //= https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#6.2.4
                //# When there is no data to send, the sender SHOULD send
                //# a PING or other ack-eliciting frame in a single packet, re-arming the
                //# PTO timer.
                if context.ack_elicitation().is_ack_eliciting()
                    || context.write_frame(&frame::Ping).is_some()
                {
                    let remaining = remaining - 1;
                    self.state = if remaining == 0 {
                        PtoState::Idle
                    } else {
                        PtoState::RequiresTransmission(remaining)
                    };
                }

                //= https://tools.ietf.org/id/draft-ietf-quic-recovery-30.txt#6.2.2.1
                //# When the PTO fires, the client MUST send a Handshake packet if it has Handshake
                //# keys, otherwise it MUST send an Initial packet in a UDP datagram of at least
                //# 1200 bytes.

                //= https://tools.ietf.org/id/draft-ietf-quic-recovery-30.txt#A.9
                // Client sends an anti-deadlock packet: Initial is padded
                // to earn more anti-amplification credit,
                // a Handshake packet proves address ownership.

                // The early transmission will automatically ensure all initial packets sent by the
                // client are padded to 1200 bytes
            }
            _ => {}
        }
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#6.2.1
    //# A sender recomputes and may need to reset its PTO timer every time an
    //# ack-eliciting packet is sent or acknowledged, when the handshake is
    //# confirmed, or when Initial or Handshake keys are discarded.
    pub fn update(&mut self, path: &Path, backoff: u32, base_timestamp: Timestamp) {
        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#6.2.1
        //# When an ack-eliciting packet is transmitted, the sender schedules a
        //# timer for the PTO period as follows:
        //#
        //# PTO = smoothed_rtt + max(4*rttvar, kGranularity) + max_ack_delay

        let mut pto_period = path.rtt_estimator.smoothed_rtt();

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-30.txt#6.2.1
        //# The PTO period MUST be set to at least kGranularity, to avoid the
        //# timer expiring immediately.
        pto_period += max(4 * path.rtt_estimator.rttvar(), K_GRANULARITY);

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#6.2.1
        //# When the PTO is armed for Initial or Handshake packet number spaces,
        //# the max_ack_delay is 0
        pto_period += self.max_ack_delay;

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#6.2.1
        //# Even when there are ack-
        //# eliciting packets in-flight in multiple packet number spaces, the
        //# exponential increase in probe timeout occurs across all spaces to
        //# prevent excess load on the network.  For example, a timeout in the
        //# Initial packet number space doubles the length of the timeout in the
        //# Handshake packet number space.
        pto_period *= backoff;

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#6.2.1
        //# The PTO period is the amount of time that a sender ought to wait for
        //# an acknowledgement of a sent packet.
        self.timer.set(base_timestamp + pto_period);
    }

    /// Cancels the PTO timer
    pub fn cancel(&mut self) {
        self.timer.cancel();
    }
}

impl FrameExchangeInterestProvider for Pto {
    fn frame_exchange_interests(&self) -> FrameExchangeInterests {
        // TODO put a fast ack on interests
        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#6.2.4
        //# If the sender wants to elicit a faster acknowledgement on PTO, it can
        //# skip a packet number to eliminate the ack delay.

        FrameExchangeInterests {
            delivery_notifications: false,
            transmission: matches!(self.state, PtoState::RequiresTransmission(_)),
        }
    }
}

impl Manager {
    /// Constructs a new `recovery::Manager`
    pub fn new(max_ack_delay: Duration) -> Self {
        Self {
            max_ack_delay,
            largest_acked_packet: None,
            sent_packets: SentPackets::default(),
            newly_acked: false,
            loss_timer: VirtualTimer::default(),
            pto: Pto::new(max_ack_delay),
            time_threshold: Duration::from_secs(0),
            bytes_in_flight: 0,
            time_of_last_ack_eliciting_packet: None,
        }
    }

    pub fn timers(&self) -> impl Iterator<Item = &Timestamp> {
        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-30.txt#6.2.1
        //# The probe timer MUST NOT be set if the time threshold (Section 6.1.2)
        //# loss detection timer is set.  The time threshold loss detection timer
        //# is expected to both expire earlier than the PTO and be less likely to
        //# spuriously retransmit data.

        core::iter::empty()
            .chain(self.pto.timers())
            .filter(move |_| !self.loss_timer.is_armed())
            .chain(self.loss_timer.iter())
    }

    pub fn on_timeout<Ctx: Context>(
        &mut self,
        timestamp: Timestamp,
        context: &mut Ctx,
    ) -> LossInfo {
        let mut loss_info = LossInfo::default();

        if self.loss_timer.is_armed() {
            if self.loss_timer.poll_expiration(timestamp).is_ready() {
                loss_info = self.detect_and_remove_lost_packets(timestamp, |packet_number_range| {
                    context.on_packet_loss(&packet_number_range);
                })
            }
        } else {
            loss_info.pto_expired = self.pto.on_timeout(self.bytes_in_flight, timestamp);
        }

        loss_info
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#A.5
    //# After a packet is sent, information about the packet is stored.
    pub fn on_packet_sent(
        &mut self,
        packet_number: PacketNumber,
        ack_elicitation: AckElicitation,
        in_flight: bool,
        sent_bytes: usize,
        time_sent: Timestamp,
    ) {
        if ack_elicitation.is_ack_eliciting() {
            self.sent_packets.insert(
                packet_number,
                SentPacketInfo::new(in_flight, sent_bytes as u64, time_sent),
            );
        }

        if in_flight {
            if ack_elicitation.is_ack_eliciting() {
                self.time_of_last_ack_eliciting_packet = Some(time_sent);
            }
            self.bytes_in_flight += sent_bytes as u64;
        }
    }

    /// Updates the time threshold used by the loss timer and sets the PTO timer
    pub fn update(
        &mut self,
        path: &Path,
        pto_backoff: u32,
        now: Timestamp,
        space: PacketNumberSpace,
        is_handshake_confirmed: bool,
    ) {
        self.update_time_threshold(&path.rtt_estimator);

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#6.2.2.1
        //# If no additional data can be sent, the server's PTO timer MUST NOT be
        //# armed until datagrams have been received from the client
        if path.at_amplification_limit() {
            // The server's timer is not set if nothing can be sent.
            self.pto.cancel();
            return;
        }

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#6.2.2.1
        //# it is the client's responsibility to send packets to unblock the server
        //# until it is certain that the server has finished its address validation
        if self.sent_packets.is_empty() && path.is_peer_validated() {
            // There is nothing to detect lost, so no timer is set.
            // However, the client needs to arm the timer if the
            // server might be blocked by the anti-amplification limit.
            self.pto.cancel();
            return;
        }

        let pto_base_timestamp = if self.sent_packets.is_empty() {
            // Arm PTO from now when there are no inflight packets.
            now
        } else {
            self.time_of_last_ack_eliciting_packet
                .expect("sent_packets is non-empty, so there must be an ack eliciting packet")
        };

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#6.2.1
        //# An endpoint MUST NOT set its PTO timer for the application data
        //# packet number space until the handshake is confirmed.
        if space.is_application_data() && !is_handshake_confirmed {
            self.pto.timer.cancel();
        } else {
            self.pto.update(path, pto_backoff, pto_base_timestamp);
        }
    }

    /// Queries the component for any outgoing frames that need to get sent
    pub fn on_transmit<W: WriteContext>(&mut self, context: &mut W) {
        self.pto.on_transmit(context)
    }

    /// Gets the number of bytes currently in flight
    pub fn bytes_in_flight(&self) -> u64 {
        self.bytes_in_flight
    }

    pub fn on_ack_frame<A: frame::ack::AckRanges, Ctx: Context>(
        &mut self,
        datagram: &DatagramInfo,
        frame: frame::Ack<A>,
        path: &mut Path,
        backoff: u32,
        context: &mut Ctx,
    ) -> Result<LossInfo, TransportError> {
        let mut loss_info = LossInfo::default();
        let mut has_any_newly_acked = false;
        let largest_acked = frame.largest_acknowledged();
        let largest_acked = Ctx::SPACE.new_packet_number(largest_acked);
        let ack_delay = frame.ack_delay();
        let ecn_counts = frame.ecn_counts;

        for ack_range in frame.ack_ranges() {
            let (start, end) = ack_range.into_inner();

            let start = Ctx::SPACE.new_packet_number(start);
            let end = Ctx::SPACE.new_packet_number(end);

            let packet_number_range = PacketNumberRange::new(start, end);

            context.validate_packet_ack(datagram, &packet_number_range)?;

            let has_newly_acked = self.on_packet_ack(
                packet_number_range,
                largest_acked,
                ack_delay,
                ecn_counts,
                datagram.timestamp,
                &mut path.rtt_estimator,
            );

            // notify components of packets that are newly acked
            if has_newly_acked {
                context.on_new_packet_ack(datagram, &packet_number_range);
            }
            context.on_packet_ack(datagram, &packet_number_range);

            //= https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#6.2.1
            //# The PTO backoff factor is reset when an acknowledgement is received,
            //# except in the following case.  A server might take longer to respond
            //# to packets during the handshake than otherwise.  To protect such a
            //# server from repeated client probes, the PTO backoff is not reset at a
            //# client that is not yet certain that the server has finished
            //# validating the client's address.  That is, a client does not reset
            //# the PTO backoff factor on receiving acknowledgements until it
            //# receives a HANDSHAKE_DONE frame or an acknowledgement for one of its
            //# Handshake or 1-RTT packets.

            loss_info.pto_reset |= has_newly_acked && path.is_peer_validated();

            has_any_newly_acked |= has_newly_acked;
        }

        if has_any_newly_acked {
            //= https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#6.2.1
            //# A sender recomputes and may need to reset its PTO timer every time an
            //# ack-eliciting packet is sent or acknowledged,
            self.update(
                &path,
                backoff,
                datagram.timestamp,
                Ctx::SPACE,
                Ctx::SPACE.is_application_data() && context.is_handshake_confirmed(),
            );

            //= https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#6.2.2
            //# Once a later packet within the same packet number space has been
            //# acknowledged, an endpoint SHOULD declare an earlier packet lost if it
            //# was sent a threshold amount of time in the past.
            loss_info +=
                self.detect_and_remove_lost_packets(datagram.timestamp, |packet_number_range| {
                    context.on_packet_loss(&packet_number_range);
                });
        }

        Ok(loss_info)
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#A.7
    //# When an ACK frame is received, it may newly acknowledge any number of packets.
    fn on_packet_ack(
        &mut self,
        acked_packets: PacketNumberRange,
        largest_acked: PacketNumber,
        ack_delay: Duration,
        ecn_counts: Option<ECNCounts>,
        receive_time: Timestamp,
        rtt_estimator: &mut RTTEstimator,
    ) -> bool {
        let largest_newly_acked = if let Some(last) = self.sent_packets.range(acked_packets).last()
        {
            // There are newly acked packets, so set newly_acked to true for use in on_ack_received_finish
            last
        } else {
            // Nothing to do if there are no newly acked packets.
            return false;
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
                // TODO: path.congestion_controller.process_ecn(ecn_counts, largest_newly_acked, largest_acked.space())
            }
        };

        // TODO: path.congestion_controller.on_packets_acked(self.sent_packets.range(acked_packets));

        for packet_number in acked_packets {
            self.sent_packets.remove(packet_number);
        }

        true
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#A.10
    //# DetectAndRemoveLostPackets is called every time an ACK is received or the time threshold
    //# loss detection timer expires. This function operates on the sent_packets for that packet
    //# number space and returns a list of packets newly detected as lost.
    fn detect_and_remove_lost_packets<OnLoss: FnMut(PacketNumberRange)>(
        &mut self,
        now: Timestamp,
        mut on_loss: OnLoss,
    ) -> LossInfo {
        // Cancel the loss timer. It will be armed again if any unacknowledged packets are
        // older than the largest acked packet, but not old enough to be considered lost yet
        self.loss_timer.cancel();
        // Packets sent before this time are deemed lost.
        let lost_send_time = now - self.time_threshold;

        // TODO: Investigate a more efficient mechanism for managing sent_packets
        //       See https://github.com/awslabs/s2n-quic/issues/69
        let mut sent_packets_to_remove = Vec::new();

        let loss_info = LossInfo::default();

        let largest_acked_packet = &self
            .largest_acked_packet
            .expect("This function is only called after an ack has been received");

        for (unacked_packet_number, unacked_sent_info) in self.sent_packets.iter() {
            if unacked_packet_number > largest_acked_packet {
                // sent_packets is ordered by packet number, so all remaining packets will be larger
                break;
            }

            // Mark packet as lost, or set time when it should be marked.
            if unacked_sent_info.time_sent <= lost_send_time // Time threshold
                ||
                largest_acked_packet // Packet threshold
                    .checked_distance(*unacked_packet_number)
                    .expect("largest_acked_packet >= unacked_packet_number")
                    >= K_PACKET_THRESHOLD
            {
                sent_packets_to_remove.push(*unacked_packet_number);

                if unacked_sent_info.in_flight {
                    // TODO merge contiguous packet numbers
                    let range =
                        PacketNumberRange::new(*unacked_packet_number, *unacked_packet_number);
                    on_loss(range);
                }
            } else {
                //= https://tools.ietf.org/id/draft-ietf-quic-recovery-30.txt#6.1.2
                //# If packets sent prior to the largest acknowledged packet cannot yet
                //# be declared lost, then a timer SHOULD be set for the remaining time.
                self.loss_timer
                    .set(unacked_sent_info.time_sent + self.time_threshold);

                //= https://tools.ietf.org/id/draft-ietf-quic-recovery-30.txt#6.2.1
                //# The probe timer MUST NOT be set if the time threshold (Section 6.1.2)
                //# loss detection timer is set.  The time threshold loss detection timer
                //# is expected to both expire earlier than the PTO and be less likely to
                //# spuriously retransmit data.
                self.pto.cancel();

                // assuming sent_packets is ordered by packet number and sent time, all remaining
                // packets will have a larger packet number and sent time, and are thus not lost.
                break;
            }
        }

        for packet_number in sent_packets_to_remove {
            self.sent_packets.remove(packet_number);
        }

        loss_info
    }

    fn update_time_threshold(&mut self, rtt_estimator: &RTTEstimator) {
        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#6.1.2
        //# The time threshold is:
        //#
        //# max(kTimeThreshold * max(smoothed_rtt, latest_rtt), kGranularity)

        let mut time_threshold = max(rtt_estimator.smoothed_rtt(), rtt_estimator.latest_rtt());

        time_threshold = apply_k_time_threshold(time_threshold);

        self.time_threshold = max(time_threshold, K_GRANULARITY);
    }
}

pub trait Context {
    const SPACE: PacketNumberSpace;
    const ENDPOINT_TYPE: EndpointType;

    fn is_handshake_confirmed(&self) -> bool;

    fn validate_packet_ack(
        &mut self,
        datagram: &DatagramInfo,
        packet_number_range: &PacketNumberRange,
    ) -> Result<(), TransportError>;

    fn on_new_packet_ack(
        &mut self,
        datagram: &DatagramInfo,
        packet_number_range: &PacketNumberRange,
    );
    fn on_packet_ack(&mut self, datagram: &DatagramInfo, packet_number_range: &PacketNumberRange);
    fn on_packet_loss(&mut self, packet_number_range: &PacketNumberRange);
}

impl FrameExchangeInterestProvider for Manager {
    fn frame_exchange_interests(&self) -> FrameExchangeInterests {
        FrameExchangeInterests {
            delivery_notifications: !self.sent_packets.is_empty(),
            transmission: false,
        } + self.pto.frame_exchange_interests()
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
        let mut recovery_manager = Manager::new(pn_space, Duration::from_millis(100));
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

            recovery_manager.on_packet_sent(
                sent_packet,
                ack_elicitation,
                in_flight,
                sent_bytes,
                time_sent,
            );

            if ack_elicitation == AckElicitation::Eliciting {
                assert!(recovery_manager.sent_packets.get(sent_packet).is_some());
                let actual_sent_packet = recovery_manager.sent_packets.get(sent_packet).unwrap();
                assert_eq!(actual_sent_packet.sent_bytes, sent_bytes);
                assert_eq!(actual_sent_packet.in_flight, in_flight);
                assert_eq!(actual_sent_packet.time_sent, time_sent);
                if in_flight {
                    assert_eq!(
                        Some(time_sent),
                        recovery_manager.time_of_last_ack_eliciting_packet
                    );
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
        let mut recovery_manager = Manager::new(pn_space, Duration::from_millis(100));

        let packets = PacketNumberRange::new(
            pn_space.new_packet_number(VarInt::from_u8(1)),
            pn_space.new_packet_number(VarInt::from_u8(10)),
        );

        let time_sent = s2n_quic_platform::time::now() + Duration::from_secs(10);

        for packet in packets {
            recovery_manager.on_packet_sent(
                packet,
                AckElicitation::Eliciting,
                true,
                128,
                time_sent,
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
        recovery_manager.on_ack_received_finish(ack_receive_time, &rtt_estimator);
        assert!(!recovery_manager.newly_acked);
    }

    #[compliance::tests("https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#A.10")]
    #[test]
    fn detect_and_remove_lost_packets() {
        let pn_space = PacketNumberSpace::ApplicationData;
        let mut rtt_estimator = RTTEstimator::new(Duration::from_millis(10));

        let mut recovery_manager = Manager::new(pn_space, Duration::from_millis(100));

        recovery_manager.largest_acked_packet =
            Some(pn_space.new_packet_number(VarInt::from_u8(10)));

        let mut time_sent = s2n_quic_platform::time::now();

        // Send a packet that was sent too long ago (lost)
        let old_packet_time_sent = pn_space.new_packet_number(VarInt::from_u8(8));
        recovery_manager.on_packet_sent(
            old_packet_time_sent,
            AckElicitation::Eliciting,
            true,
            1,
            time_sent,
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
        );

        // Send a packet that is less than the largest acked but not lost
        let not_lost = pn_space.new_packet_number(VarInt::from_u8(9));
        recovery_manager.on_packet_sent(not_lost, AckElicitation::Eliciting, true, 1, time_sent);

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

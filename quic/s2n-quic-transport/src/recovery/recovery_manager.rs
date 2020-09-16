// TODO: Remove when used
#![allow(dead_code)]

use crate::{
    contexts::WriteContext,
    frame_exchange_interests::{FrameExchangeInterestProvider, FrameExchangeInterests},
    recovery::{SentPacketInfo, SentPackets},
    space::INITIAL_PTO_BACKOFF,
    timer::VirtualTimer,
};
use core::{cmp::max, time::Duration};
use s2n_quic_core::{
    endpoint::EndpointType,
    frame::{self, ack_elicitation::AckElicitation},
    inet::DatagramInfo,
    packet::number::{PacketNumber, PacketNumberRange, PacketNumberSpace},
    path::Path,
    recovery::RTTEstimator,
    time::Timestamp,
    transport::error::TransportError,
};

#[derive(Debug)]
pub struct Manager {
    // The packet space for this recovery manager
    space: PacketNumberSpace,

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
    /// The recovery manager requires updating if a PTO expired/needs to be reset, or
    /// loss packets were detected.
    pub fn updated_required(&self) -> bool {
        self.bytes_in_flight > 0 || self.pto_expired || self.pto_reset
    }
}

#[allow(clippy::suspicious_arithmetic_impl)]
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
        //# The PTO period MUST be at least kGranularity, to avoid the timer
        //# expiring immediately.
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
    pub fn new(space: PacketNumberSpace, max_ack_delay: Duration) -> Self {
        Self {
            space,
            max_ack_delay,
            largest_acked_packet: None,
            sent_packets: SentPackets::default(),
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

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-30.txt#6.2.1
        //# An endpoint MUST NOT set its PTO timer for the application data
        //# packet number space until the handshake is confirmed.
        if self.space.is_application_data() && !is_handshake_confirmed {
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
        let mut has_newly_acked = false;
        let largest_acked_in_frame = self.space.new_packet_number(frame.largest_acknowledged());

        // Update the largest acked packet if the largest packet acked in this frame is larger
        self.largest_acked_packet = Some(
            self.largest_acked_packet
                .map_or(largest_acked_in_frame, |pn| pn.max(largest_acked_in_frame)),
        );

        for ack_range in frame.ack_ranges() {
            let (start, end) = ack_range.into_inner();

            let acked_packets = PacketNumberRange::new(
                self.space.new_packet_number(start),
                self.space.new_packet_number(end),
            );

            context.validate_packet_ack(datagram, &acked_packets)?;
            context.on_packet_ack(datagram, &acked_packets);

            if let Some((largest_newly_acked, largest_newly_acked_info)) =
                self.sent_packets.range(acked_packets).last()
            {
                //= https://tools.ietf.org/id/draft-ietf-quic-recovery-30.txt#5.1
                //# An RTT sample is generated using only the largest acknowledged packet in the
                //# received ACK frame. This is because a peer reports acknowledgment delays for
                //# only the largest acknowledged packet in an ACK frame.
                if *largest_newly_acked == largest_acked_in_frame {
                    let latest_rtt = datagram.timestamp - largest_newly_acked_info.time_sent;
                    path.rtt_estimator.update_rtt(
                        frame.ack_delay(),
                        latest_rtt,
                        largest_newly_acked.space(),
                    );
                }
            } else {
                // Nothing to do if there are no newly acked packets.
                continue;
            };

            // TODO: path.congestion_controller.on_packets_acked(self.sent_packets.range(acked_packets));

            for packet_number in acked_packets {
                if let Some(acked_packet_info) = self.sent_packets.remove(packet_number) {
                    self.bytes_in_flight -= acked_packet_info.sent_bytes;
                }
            }

            // notify components of packets that are newly acked
            context.on_new_packet_ack(datagram, &acked_packets);

            has_newly_acked = true;
        }

        let mut loss_info = LossInfo::default();

        if has_newly_acked {
            // Process ECN information if present.
            if frame.ecn_counts.is_some() {
                // TODO: path.congestion_controller.process_ecn(ecn_counts, largest_newly_acked, largest_newly_acked_packet.space())
            }

            //= https://tools.ietf.org/id/draft-ietf-quic-recovery-30.txt#6.1.2
            //# Once a later packet within the same packet number space has been
            //# acknowledged, an endpoint SHOULD declare an earlier packet lost if it
            //# was sent a threshold amount of time in the past.
            loss_info =
                self.detect_and_remove_lost_packets(datagram.timestamp, |packet_number_range| {
                    context.on_packet_loss(&packet_number_range);
                });

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
            loss_info.pto_reset = path.is_peer_validated();

            // If there is a pending pto reset, use the initial pto_backoff when updating the PTO timer
            let pto_backoff = if loss_info.pto_reset {
                INITIAL_PTO_BACKOFF
            } else {
                backoff
            };

            //= https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#6.2.1
            //# A sender recomputes and may need to reset its PTO timer every time an
            //# ack-eliciting packet is sent or acknowledged,
            self.update(
                &path,
                pto_backoff,
                datagram.timestamp,
                self.space.is_application_data() && context.is_handshake_confirmed(),
            );
        }

        Ok(loss_info)
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

        let mut loss_info = LossInfo::default();

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

                loss_info.bytes_in_flight += unacked_sent_info.sent_bytes;

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
    const ENDPOINT_TYPE: EndpointType;

    fn is_handshake_confirmed(&self) -> bool {
        panic!("Handshake status is not currently available in this packet space")
    }

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
    use crate::{recovery, space::rx_packet_numbers::ack_ranges::AckRanges};
    use core::time::Duration;
    use s2n_quic_core::{connection, packet::number::PacketNumberSpace, varint::VarInt};
    use std::collections::HashSet;

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
            let sent_bytes = (2 * i) as usize;

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
                assert_eq!(actual_sent_packet.sent_bytes, sent_bytes as u64);
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
    fn on_ack_frame() {
        let pn_space = PacketNumberSpace::ApplicationData;
        let rtt_estimator = RTTEstimator::new(Duration::from_millis(10));
        let mut recovery_manager = Manager::new(pn_space, Duration::from_millis(100));
        let packet_bytes = 128;

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
                packet_bytes,
                time_sent,
            );
        }

        assert_eq!(recovery_manager.sent_packets.iter().count(), 10);

        // Ack packets 1 to 3
        let acked_packets = PacketNumberRange::new(
            pn_space.new_packet_number(VarInt::from_u8(1)),
            pn_space.new_packet_number(VarInt::from_u8(3)),
        );

        let ack_receive_time = time_sent + Duration::from_millis(500);

        let datagram = DatagramInfo {
            timestamp: ack_receive_time,
            remote_address: Default::default(),
            payload_len: 0,
            ecn: Default::default(),
        };

        let mut ack_range = AckRanges::new(acked_packets.count());

        for acked_packet in acked_packets {
            ack_range.insert_packet_number(acked_packet);
        }

        let frame = frame::Ack {
            ack_delay: VarInt::from_u8(10),
            ack_ranges: (&ack_range),
            ecn_counts: None,
        };
        let mut path = Path::new(
            Default::default(),
            connection::Id::EMPTY,
            rtt_estimator,
            true,
        );

        let mut context = MockContext::default();
        let result = recovery_manager.on_ack_frame(&datagram, frame, &mut path, 1, &mut context);

        assert_eq!(result.unwrap().bytes_in_flight, 0);
        assert_eq!(recovery_manager.sent_packets.iter().count(), 7);
        for packet in acked_packets {
            assert!(recovery_manager.sent_packets.get(packet).is_none());
        }
        assert_eq!(
            recovery_manager.largest_acked_packet,
            Some(acked_packets.end())
        );
        assert_eq!(context.on_packet_ack_count, 1);
        assert_eq!(context.on_new_packet_ack_count, 1);
        assert_eq!(context.validate_packet_ack_count, 1);
        assert_eq!(context.on_packet_loss_count, 0);

        // Now the largest packet number has been acked so the RTT is updated
        assert_eq!(path.rtt_estimator.latest_rtt(), Duration::from_millis(500));

        let ack_receive_time = ack_receive_time + Duration::from_secs(1);

        let datagram = DatagramInfo {
            timestamp: ack_receive_time,
            remote_address: Default::default(),
            payload_len: 0,
            ecn: Default::default(),
        };

        // Acknowledging already acked packets
        let frame = frame::Ack {
            ack_delay: VarInt::from_u8(10),
            ack_ranges: (&ack_range),
            ecn_counts: None,
        };
        let result = recovery_manager.on_ack_frame(&datagram, frame, &mut path, 1, &mut context);

        // Acknowledging already acked packets does not call on_new_packet_ack or change RTT
        assert_eq!(result.unwrap().bytes_in_flight, 0);
        assert_eq!(context.on_packet_ack_count, 2);
        assert_eq!(context.on_new_packet_ack_count, 1);
        assert_eq!(context.validate_packet_ack_count, 2);
        assert_eq!(context.on_packet_loss_count, 0);

        assert_eq!(path.rtt_estimator.latest_rtt(), Duration::from_millis(500));

        // Ack packets 7 to 9 (4 - 6 will be considered lost)
        let acked_packets = PacketNumberRange::new(
            pn_space.new_packet_number(VarInt::from_u8(7)),
            pn_space.new_packet_number(VarInt::from_u8(9)),
        );

        let ack_receive_time = ack_receive_time + Duration::from_secs(1);

        let datagram = DatagramInfo {
            timestamp: ack_receive_time,
            remote_address: Default::default(),
            payload_len: 0,
            ecn: Default::default(),
        };

        let mut ack_range = AckRanges::new(acked_packets.count());

        for acked_packet in acked_packets {
            ack_range.insert_packet_number(acked_packet);
        }

        let frame = frame::Ack {
            ack_delay: VarInt::from_u8(10),
            ack_ranges: (&ack_range),
            ecn_counts: None,
        };
        let result = recovery_manager.on_ack_frame(&datagram, frame, &mut path, 1, &mut context);

        assert_eq!(context.on_packet_ack_count, 3);
        assert_eq!(context.on_new_packet_ack_count, 2);
        assert_eq!(context.validate_packet_ack_count, 3);
        assert_eq!(context.on_packet_loss_count, 3);

        assert_eq!(path.rtt_estimator.latest_rtt(), Duration::from_millis(2500));

        let loss_info = result.expect("loss info should exist");
        assert_eq!(loss_info.bytes_in_flight, (packet_bytes * 3) as u64);
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

        recovery_manager.time_threshold = Duration::from_secs(9);
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
        let mut lost_packets: HashSet<PacketNumber> = HashSet::default();

        let loss_info = recovery_manager.detect_and_remove_lost_packets(now, |packet_range| {
            lost_packets.insert(packet_range.start());
        });

        // Two packets lost, each size 1 byte
        assert_eq!(loss_info.bytes_in_flight, 2);

        let sent_packets = &recovery_manager.sent_packets;
        assert!(lost_packets.contains(&old_packet_time_sent));
        assert!(sent_packets.get(old_packet_time_sent).is_none());

        assert!(lost_packets.contains(&old_packet_packet_number));
        assert!(sent_packets.get(old_packet_packet_number).is_none());

        assert!(!lost_packets.contains(&larger_than_largest));
        assert!(sent_packets.get(larger_than_largest).is_some());

        assert!(!lost_packets.contains(&not_lost));
        assert!(sent_packets.get(not_lost).is_some());

        let expected_loss_time =
            sent_packets.get(not_lost).unwrap().time_sent + recovery_manager.time_threshold;
        assert!(recovery_manager.loss_timer.is_armed());
        assert_eq!(
            Some(&expected_loss_time),
            recovery_manager.loss_timer.iter().next()
        );
    }

    #[derive(Default)]
    struct MockContext {
        validate_packet_ack_count: u8,
        on_new_packet_ack_count: u8,
        on_packet_ack_count: u8,
        on_packet_loss_count: u8,
    }

    impl recovery::Context for MockContext {
        const ENDPOINT_TYPE: EndpointType = EndpointType::Client;

        fn is_handshake_confirmed(&self) -> bool {
            true
        }

        fn validate_packet_ack(
            &mut self,
            _datagram: &DatagramInfo,
            _packet_number_range: &PacketNumberRange,
        ) -> Result<(), TransportError> {
            self.validate_packet_ack_count += 1;
            Ok(())
        }

        fn on_new_packet_ack(
            &mut self,
            _datagram: &DatagramInfo,
            _packet_number_range: &PacketNumberRange,
        ) {
            self.on_new_packet_ack_count += 1;
        }

        fn on_packet_ack(
            &mut self,
            _datagram: &DatagramInfo,
            _packet_number_range: &PacketNumberRange,
        ) {
            self.on_packet_ack_count += 1;
        }

        fn on_packet_loss(&mut self, _packet_number_range: &PacketNumberRange) {
            self.on_packet_loss_count += 1;
        }
    }
}

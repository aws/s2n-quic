use crate::{
    connection::transmission,
    contexts::WriteContext,
    frame_exchange_interests::{FrameExchangeInterestProvider, FrameExchangeInterests},
    recovery::{loss_info::LossInfo, SentPacketInfo, SentPackets},
    space::INITIAL_PTO_BACKOFF,
    timer::VirtualTimer,
};
use core::{cmp::max, time::Duration};
use s2n_quic_core::{
    endpoint::EndpointType,
    frame,
    inet::DatagramInfo,
    packet::number::{PacketNumber, PacketNumberRange, PacketNumberSpace},
    path::Path,
    recovery::{CongestionController, RTTEstimator, K_GRANULARITY},
    time::Timestamp,
    transport::error::TransportError,
    varint::VarInt,
};
use smallvec::SmallVec;

#[derive(Debug)]
pub struct Manager {
    // The packet space for this recovery manager
    space: PacketNumberSpace,

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-30.txt#A.3
    //# The maximum amount of time by which the receiver
    //# intends to delay acknowledgments for packets in the Application
    //# Data packet number space, as defined by the eponymous transport
    //# parameter (Section 18.2 of [QUIC-TRANSPORT]).  Note that the
    //# actual ack_delay in a received ACK frame may be larger due to late
    //# timers, reordering, or loss.
    max_ack_delay: Duration,

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-30.txt#A.3
    //# The largest packet number acknowledged in the packet number space so far.
    largest_acked_packet: Option<PacketNumber>,

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-30.txt#A.3
    //# An association of packet numbers in a packet number space to information about them.
    //  These are packets that are pending acknowledgement.
    sent_packets: SentPackets,

    // Timer set when packets may be declared lost at a time in the future
    loss_timer: VirtualTimer,

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-30.txt#6.1.2
    //# Once a later packet within the same packet number space has been
    //# acknowledged, an endpoint SHOULD declare an earlier packet lost if it
    //# was sent a threshold amount of time in the past.
    time_threshold: Duration,

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-30.txt#6.2
    //# A Probe Timeout (PTO) triggers sending one or two probe datagrams
    //# when ack-eliciting packets are not acknowledged within the expected
    //# period of time or the server may not have validated the client's
    //# address.  A PTO enables a connection to recover from loss of tail
    //# packets or acknowledgements.
    pto: Pto,

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-30.txt#A.3
    //# The time the most recent ack-eliciting packet was sent.
    time_of_last_ack_eliciting_packet: Option<Timestamp>,

    ecn_ce_counter: VarInt,

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-31.txt#B.2
    //# The time that the first RTT sample was obtained.
    first_rtt_sample: Option<Timestamp>,
}

//= https://tools.ietf.org/id/draft-ietf-quic-recovery-30.txt#6.1.1
//# The RECOMMENDED initial value for the packet reordering threshold
//# (kPacketThreshold) is 3, based on best practices for TCP loss
//# detection ([RFC5681], [RFC6675]).  In order to remain similar to TCP,
//# implementations SHOULD NOT use a packet threshold less than 3; see
//# [RFC5681].
const K_PACKET_THRESHOLD: u64 = 3;

/// Initial capacity of the SmallVec used for keeping track of packets
/// acked in an ack frame
// TODO: Determine if there is a more appropriate default
const ACKED_PACKETS_INITIAL_CAPACITY: usize = 10;

fn apply_k_time_threshold(duration: Duration) -> Duration {
    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-30.txt#6.1.2
    //# The RECOMMENDED time threshold (kTimeThreshold), expressed as a
    //# round-trip time multiplier, is 9/8.
    duration * 9 / 8
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
            time_of_last_ack_eliciting_packet: None,
            ecn_ce_counter: VarInt::default(),
            first_rtt_sample: None,
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
            loss_info.pto_expired = self
                .pto
                .on_timeout(!self.sent_packets.is_empty(), timestamp);
        }

        loss_info
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-30.txt#A.5
    //# After a packet is sent, information about the packet is stored.
    pub fn on_packet_sent<CC: CongestionController>(
        &mut self,
        packet_number: PacketNumber,
        outcome: transmission::Outcome,
        time_sent: Timestamp,
        path: &mut Path<CC>,
    ) {
        if outcome.ack_elicitation.is_ack_eliciting() {
            self.sent_packets.insert(
                packet_number,
                SentPacketInfo::new(
                    outcome.is_congestion_controlled,
                    outcome.bytes_sent,
                    time_sent,
                ),
            );
        }

        if outcome.is_congestion_controlled && outcome.ack_elicitation.is_ack_eliciting() {
            self.time_of_last_ack_eliciting_packet = Some(time_sent);
        }

        if outcome.is_congestion_controlled {
            path.congestion_controller
                .on_packet_sent(time_sent, outcome.bytes_sent);
        }
    }

    /// Updates the time threshold used by the loss timer and sets the PTO timer
    pub fn update<CC: CongestionController>(
        &mut self,
        path: &Path<CC>,
        pto_backoff: u32,
        now: Timestamp,
        is_handshake_confirmed: bool,
    ) {
        self.update_time_threshold(&path.rtt_estimator);

        // Record the timestamp of the first rtt sample for use in
        // determining persistent congestion
        self.first_rtt_sample = self
            .first_rtt_sample
            .or_else(|| path.rtt_estimator.first_rtt_sample());

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-30.txt#6.2.2.1
        //# If no additional data can be sent, the server's PTO timer MUST NOT be
        //# armed until datagrams have been received from the client
        if path.at_amplification_limit() {
            // The server's timer is not set if nothing can be sent.
            self.pto.cancel();
            return;
        }

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-30.txt#6.2.2.1
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
            self.pto.cancel();
        } else {
            self.pto.update(path, pto_backoff, pto_base_timestamp);
        }
    }

    /// Queries the component for any outgoing frames that need to get sent
    pub fn on_transmit<W: WriteContext>(&mut self, context: &mut W) {
        self.pto.on_transmit(context)
    }

    pub fn on_ack_frame<A: frame::ack::AckRanges, CC: CongestionController, Ctx: Context>(
        &mut self,
        datagram: &DatagramInfo,
        frame: frame::Ack<A>,
        path: &mut Path<CC>,
        backoff: u32,
        context: &mut Ctx,
    ) -> Result<LossInfo, TransportError> {
        let largest_acked_in_frame = self.space.new_packet_number(frame.largest_acknowledged());
        let mut newly_acked_packets =
            SmallVec::<[SentPacketInfo; ACKED_PACKETS_INITIAL_CAPACITY]>::new();
        self.first_rtt_sample = self.first_rtt_sample.or(Some(datagram.timestamp));

        // Update the largest acked packet if the largest packet acked in this frame is larger
        self.largest_acked_packet = Some(
            self.largest_acked_packet
                .map_or(largest_acked_in_frame, |pn| pn.max(largest_acked_in_frame)),
        );

        let mut largest_newly_acked: Option<(PacketNumber, SentPacketInfo)> = None;

        for ack_range in frame.ack_ranges() {
            let (start, end) = ack_range.into_inner();

            let acked_packets = PacketNumberRange::new(
                self.space.new_packet_number(start),
                self.space.new_packet_number(end),
            );

            context.validate_packet_ack(datagram, &acked_packets)?;
            context.on_packet_ack(datagram, &acked_packets);

            let mut new_packet_ack = false;
            for packet_number in acked_packets {
                if let Some(acked_packet_info) = self.sent_packets.remove(packet_number) {
                    newly_acked_packets.push(acked_packet_info);

                    if largest_newly_acked.map_or(true, |(pn, _)| packet_number > pn) {
                        largest_newly_acked = Some((packet_number, acked_packet_info));
                    }

                    new_packet_ack = true;
                }
            }

            if new_packet_ack {
                // notify components of packets that are newly acked
                context.on_new_packet_ack(datagram, &acked_packets);
            }
        }

        if largest_newly_acked.is_none() {
            // Nothing to do if there are no newly acked packets.
            return Ok(LossInfo::default());
        }

        let largest_newly_acked = largest_newly_acked.expect("There are newly acked packets");

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-30.txt#5.1
        //# An RTT sample is generated using only the largest acknowledged packet in the
        //# received ACK frame. This is because a peer reports acknowledgment delays for
        //# only the largest acknowledged packet in an ACK frame.
        if largest_newly_acked.0 == largest_acked_in_frame {
            let latest_rtt = datagram.timestamp - largest_newly_acked.1.time_sent;
            path.rtt_estimator.update_rtt(
                frame.ack_delay(),
                latest_rtt,
                datagram.timestamp,
                largest_acked_in_frame.space(),
            );

            // Update the congestion controller with the latest RTT estimate
            path.congestion_controller
                .on_rtt_update(largest_newly_acked.1.time_sent, &path.rtt_estimator);
        }

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-31.txt#7.1
        //# If a path has been validated to support ECN ([RFC3168], [RFC8311]),
        //# QUIC treats a Congestion Experienced (CE) codepoint in the IP header
        //# as a signal of congestion.
        if let Some(ecn_counts) = frame.ecn_counts {
            if ecn_counts.ce_count > self.ecn_ce_counter {
                self.ecn_ce_counter = ecn_counts.ce_count;
                path.congestion_controller
                    .on_congestion_event(datagram.timestamp);
            }
        }

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-30.txt#6.1.2
        //# Once a later packet within the same packet number space has been
        //# acknowledged, an endpoint SHOULD declare an earlier packet lost if it
        //# was sent a threshold amount of time in the past.
        let mut loss_info =
            self.detect_and_remove_lost_packets(datagram.timestamp, |packet_number_range| {
                context.on_packet_loss(&packet_number_range);
            });

        if loss_info.bytes_in_flight > 0 {
            path.congestion_controller.on_packets_lost(
                loss_info,
                path.rtt_estimator.persistent_congestion_threshold(),
                datagram.timestamp,
            );
        }

        for acked_packet_info in newly_acked_packets {
            path.congestion_controller.on_packet_ack(
                largest_newly_acked.1.time_sent,
                acked_packet_info.sent_bytes as usize,
                &path.rtt_estimator,
                datagram.timestamp,
            );
        }

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-30.txt#6.2.1
        //# The PTO backoff factor is reset when an acknowledgement is received,
        //# except in the following case.  A server might take longer to respond
        //# to packets during the handshake than otherwise.  To protect such a
        //# server from repeated client probes, the PTO backoff is not reset at a
        //# client that is not yet certain that the server has finished
        //# validating the client's address.  That is, a client does not reset
        //# the PTO backoff factor on receiving acknowledgements until the
        //# handshake is confirmed; see Section 4.1.2 of [QUIC-TLS].
        loss_info.pto_reset = path.is_peer_validated();

        // If there is a pending pto reset, use the initial pto_backoff when updating the PTO timer
        let pto_backoff = if loss_info.pto_reset {
            INITIAL_PTO_BACKOFF
        } else {
            backoff
        };

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-30.txt#6.2.1
        //# A sender SHOULD restart its PTO timer every time an ack-eliciting
        //# packet is sent or acknowledged,
        self.update(
            &path,
            pto_backoff,
            datagram.timestamp,
            self.space.is_application_data() && context.is_handshake_confirmed(),
        );

        Ok(loss_info)
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-31.txt#B.9
    //# When Initial or Handshake keys are discarded, packets from the
    //# space are discarded and loss detection state is updated.
    pub fn on_packet_number_space_discarded<CC: CongestionController>(
        &mut self,
        path: &mut Path<CC>,
    ) {
        debug_assert_ne!(self.space, PacketNumberSpace::ApplicationData);
        // Remove any unacknowledged packets from flight.
        for (_, unacked_sent_info) in self.sent_packets.iter() {
            path.congestion_controller
                .on_packet_discarded(unacked_sent_info.sent_bytes as usize);
        }
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-30.txt#A.10
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

        let mut persistent_congestion_period = Duration::from_secs(0);
        let mut prev_packet: Option<(&PacketNumber, Timestamp)> = None;

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

                loss_info.bytes_in_flight += unacked_sent_info.sent_bytes as usize;

                if unacked_sent_info.in_flight {
                    // TODO merge contiguous packet numbers
                    let range =
                        PacketNumberRange::new(*unacked_packet_number, *unacked_packet_number);
                    on_loss(range);
                }

                //= https://tools.ietf.org/id/draft-ietf-quic-recovery-31.txt#7.6.2
                //# A sender establishes persistent congestion on receiving an
                //# acknowledgement if at least two ack-eliciting packets are declared
                //# lost, and:
                //#
                //# *  all packets, across all packet number spaces, sent between these
                //#    two send times are declared lost;
                // Check if this lost packet is contiguous with the previous lost packet
                // in order to update the persistent congestion period.
                let is_contiguous = prev_packet.map_or(false, |(pn, _)| {
                    unacked_packet_number.checked_distance(*pn) == Some(1)
                });
                if is_contiguous {
                    // The previous lost packet was 1 less than this one, so it is contiguous.
                    // Add the difference in time to the current period.
                    persistent_congestion_period +=
                        unacked_sent_info.time_sent - prev_packet.expect("checked above").1;
                    loss_info.persistent_congestion_period = max(
                        loss_info.persistent_congestion_period,
                        persistent_congestion_period,
                    );
                } else {
                    // There was a gap in packet number or this is the beginning of the period.
                    // Reset the current period to zero.
                    persistent_congestion_period = Duration::from_secs(0);
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

            //= https://tools.ietf.org/id/draft-ietf-quic-recovery-31.txt#7.6.2
            //# The persistent congestion period SHOULD NOT start until there is at
            //# least one RTT sample.  Before the first RTT sample, a sender arms its
            //# PTO timer based on the initial RTT (Section 6.2.2), which could be
            //# substantially larger than the actual RTT.  Requiring a prior RTT
            //# sample prevents a sender from establishing persistent congestion with
            //# potentially too few probes.
            if self
                .first_rtt_sample
                .map_or(false, |ts| unacked_sent_info.time_sent > ts)
            {
                prev_packet = Some((unacked_packet_number, unacked_sent_info.time_sent));
            }
        }

        for packet_number in sent_packets_to_remove {
            self.sent_packets.remove(packet_number);
        }

        loss_info
    }

    fn update_time_threshold(&mut self, rtt_estimator: &RTTEstimator) {
        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-30.txt#6.1.2
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

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-31.txt#7.8
    //# When bytes in flight is smaller than the congestion window and
    //# sending is not pacing limited, the congestion window is under-
    //# utilized.  When this occurs, the congestion window SHOULD NOT be
    //# increased in either slow start or congestion avoidance.  This can
    //# happen due to insufficient application data or flow control limits.
    fn is_limited(&self) -> bool {
        //TODO: https://github.com/awslabs/s2n-quic/issues/137
        false
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

/// Manages the probe time out calculation and probe packet transmission
#[derive(Debug, Default)]
struct Pto {
    timer: VirtualTimer,
    state: PtoState,
    max_ack_delay: Duration,
}

#[derive(Debug, PartialEq)]
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
    /// Constructs a new `Pto` with the given `max_ack_delay`
    pub fn new(max_ack_delay: Duration) -> Self {
        Self {
            max_ack_delay,
            ..Self::default()
        }
    }

    /// Returns an iterator containing the probe timeout timestamp
    pub fn timers(&self) -> impl Iterator<Item = &Timestamp> {
        self.timer.iter()
    }

    /// Called when a timeout has occurred. Returns true if the PTO timer had expired.
    pub fn on_timeout(&mut self, packets_in_flight: bool, timestamp: Timestamp) -> bool {
        if self.timer.poll_expiration(timestamp).is_ready() {
            //= https://tools.ietf.org/id/draft-ietf-quic-recovery-30.txt#6.2.4
            //# When a PTO timer expires, a sender MUST send at least one ack-
            //# eliciting packet in the packet number space as a probe

            //= https://tools.ietf.org/id/draft-ietf-quic-recovery-30.txt#6.2.2.1
            //# Since the server could be blocked until more datagrams are received
            //# from the client, it is the client's responsibility to send packets to
            //# unblock the server until it is certain that the server has finished
            //# its address validation

            //= https://tools.ietf.org/id/draft-ietf-quic-recovery-30.txt#6.2.4
            //# An endpoint MAY send up to two full-
            //# sized datagrams containing ack-eliciting packets

            //= https://tools.ietf.org/id/draft-ietf-quic-recovery-30.txt#6.2.4
            //# Sending two packets on PTO
            //# expiration increases resilience to packet drops, thus reducing the
            //# probability of consecutive PTO events.
            let transmission_count = if packets_in_flight { 2 } else { 1 };

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
                //= https://tools.ietf.org/id/draft-ietf-quic-recovery-30.txt#6.2.4
                //# When there is no data to send, the sender SHOULD send
                //# a PING or other ack-eliciting frame in a single packet, re-arming the
                //# PTO timer.

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
            }
            _ => {}
        }
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-30.txt#6.2.1
    //# A sender SHOULD restart its PTO timer every time an ack-eliciting
    //# packet is sent or acknowledged, when the handshake is confirmed
    //# (Section 4.1.2 of [QUIC-TLS]), or when Initial or Handshake keys are
    //# discarded (Section 9 of [QUIC-TLS]).
    pub fn update<CC: CongestionController>(
        &mut self,
        path: &Path<CC>,
        backoff: u32,
        base_timestamp: Timestamp,
    ) {
        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-30.txt#6.2.1
        //# When an ack-eliciting packet is transmitted, the sender schedules a
        //# timer for the PTO period as follows:
        //#
        //# PTO = smoothed_rtt + max(4*rttvar, kGranularity) + max_ack_delay

        let mut pto_period = path.rtt_estimator.smoothed_rtt();

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-30.txt#6.2.1
        //# The PTO period MUST be at least kGranularity, to avoid the timer
        //# expiring immediately.
        pto_period += max(4 * path.rtt_estimator.rttvar(), K_GRANULARITY);

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-30.txt#6.2.1
        //# When the PTO is armed for Initial or Handshake packet number spaces,
        //# the max_ack_delay in the PTO period computation is set to 0, since
        //# the peer is expected to not delay these packets intentionally; see
        //# 13.2.1 of [QUIC-TRANSPORT].
        pto_period += self.max_ack_delay;

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-30.txt#6.2.1
        //# Even when there are ack-
        //# eliciting packets in-flight in multiple packet number spaces, the
        //# exponential increase in probe timeout occurs across all spaces to
        //# prevent excess load on the network.  For example, a timeout in the
        //# Initial packet number space doubles the length of the timeout in the
        //# Handshake packet number space.
        pto_period *= backoff;

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-30.txt#6.2.1
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
        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-30.txt#6.2.4
        //# If the sender wants to elicit a faster acknowledgement on PTO, it can
        //# skip a packet number to eliminate the acknowledgment delay.

        FrameExchangeInterests {
            delivery_notifications: false,
            transmission: matches!(self.state, PtoState::RequiresTransmission(_)),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{
        contexts::testing::{MockConnectionContext, MockWriteContext, OutgoingFrameBuffer},
        recovery,
        recovery::manager::PtoState::RequiresTransmission,
        space::rx_packet_numbers::ack_ranges::AckRanges,
    };
    use core::{ops::RangeInclusive, time::Duration};
    use s2n_quic_core::{
        connection, frame::ack_elicitation::AckElicitation, packet::number::PacketNumberSpace,
        recovery::congestion_controller::testing::Unlimited, varint::VarInt,
    };
    use std::collections::HashSet;

    #[compliance::tests("https://tools.ietf.org/id/draft-ietf-quic-recovery-30.txt#A.5")]
    #[test]
    fn on_packet_sent() {
        let space = PacketNumberSpace::ApplicationData;
        let mut manager = Manager::new(space, Duration::from_millis(100));
        let mut time_sent = s2n_quic_platform::time::now();
        let mut path = Path::new(
            Default::default(),
            connection::Id::EMPTY,
            RTTEstimator::new(Duration::from_millis(10)),
            Unlimited::default(),
            true,
        );

        for i in 1..=10 {
            let sent_packet = space.new_packet_number(VarInt::from_u8(i));
            let ack_elicitation = if i % 2 == 0 {
                AckElicitation::Eliciting
            } else {
                AckElicitation::NonEliciting
            };

            let outcome = transmission::Outcome {
                ack_elicitation,
                is_congestion_controlled: i % 3 == 0,
                bytes_sent: (2 * i) as usize,
            };

            manager.on_packet_sent(sent_packet, outcome, time_sent, &mut path);

            if ack_elicitation == AckElicitation::Eliciting {
                assert!(manager.sent_packets.get(sent_packet).is_some());
                let actual_sent_packet = manager.sent_packets.get(sent_packet).unwrap();
                assert_eq!(actual_sent_packet.sent_bytes as usize, outcome.bytes_sent);
                assert_eq!(
                    actual_sent_packet.in_flight,
                    outcome.is_congestion_controlled
                );
                assert_eq!(actual_sent_packet.time_sent, time_sent);
                if outcome.is_congestion_controlled {
                    assert_eq!(Some(time_sent), manager.time_of_last_ack_eliciting_packet);
                }
            } else {
                assert!(manager.sent_packets.get(sent_packet).is_none());
            }

            time_sent += Duration::from_millis(10);
        }

        assert_eq!(manager.sent_packets.iter().count(), 5);
    }

    #[compliance::tests("https://tools.ietf.org/id/draft-ietf-quic-recovery-30.txt#A.7")]
    #[test]
    fn on_ack_frame() {
        let space = PacketNumberSpace::ApplicationData;
        let rtt_estimator = RTTEstimator::new(Duration::from_millis(10));
        let mut manager = Manager::new(space, Duration::from_millis(100));
        let packet_bytes = 128;
        let mut path = Path::new(
            Default::default(),
            connection::Id::EMPTY,
            rtt_estimator,
            Unlimited::default(),
            true,
        );

        let time_sent = s2n_quic_platform::time::now() + Duration::from_secs(10);

        // Send packets 1 to 10
        for i in 1..=10 {
            manager.on_packet_sent(
                space.new_packet_number(VarInt::from_u8(i)),
                transmission::Outcome {
                    ack_elicitation: AckElicitation::Eliciting,
                    is_congestion_controlled: true,
                    bytes_sent: packet_bytes,
                },
                time_sent,
                &mut path,
            );
        }

        assert_eq!(manager.sent_packets.iter().count(), 10);

        // Ack packets 1 to 3
        let ack_receive_time = time_sent + Duration::from_millis(500);
        let (result, context) = ack_packets(1..=3, ack_receive_time, &mut path, &mut manager);

        assert_eq!(result.unwrap().bytes_in_flight, 0);
        assert!(result.unwrap().pto_reset);
        assert_eq!(manager.sent_packets.iter().count(), 7);
        assert_eq!(
            manager.largest_acked_packet,
            Some(space.new_packet_number(VarInt::from_u8(3)))
        );
        assert_eq!(context.on_packet_ack_count, 1);
        assert_eq!(context.on_new_packet_ack_count, 1);
        assert_eq!(context.validate_packet_ack_count, 1);
        assert_eq!(context.on_packet_loss_count, 0);
        assert_eq!(path.rtt_estimator.latest_rtt(), Duration::from_millis(500));

        // Acknowledging already acked packets
        let ack_receive_time = ack_receive_time + Duration::from_secs(1);
        let (result, context) = ack_packets(1..=3, ack_receive_time, &mut path, &mut manager);

        // Acknowledging already acked packets does not call on_new_packet_ack or change RTT
        assert_eq!(result.unwrap().bytes_in_flight, 0);
        assert!(!result.unwrap().pto_reset);
        assert_eq!(context.on_packet_ack_count, 1);
        assert_eq!(context.on_new_packet_ack_count, 0);
        assert_eq!(context.validate_packet_ack_count, 1);
        assert_eq!(context.on_packet_loss_count, 0);
        assert_eq!(path.rtt_estimator.latest_rtt(), Duration::from_millis(500));

        // Ack packets 7 to 9 (4 - 6 will be considered lost)
        let ack_receive_time = ack_receive_time + Duration::from_secs(1);
        let (result, context) = ack_packets(7..=9, ack_receive_time, &mut path, &mut manager);

        assert_eq!(result.unwrap().bytes_in_flight, (packet_bytes * 3) as usize);
        assert!(result.unwrap().pto_reset);
        assert_eq!(context.on_packet_ack_count, 1);
        assert_eq!(context.on_new_packet_ack_count, 1);
        assert_eq!(context.validate_packet_ack_count, 1);
        assert_eq!(context.on_packet_loss_count, 3);
        assert_eq!(path.rtt_estimator.latest_rtt(), Duration::from_millis(2500));

        // Ack packet 10, but with a path that is not peer validated
        let mut path = Path::new(
            Default::default(),
            connection::Id::EMPTY,
            rtt_estimator,
            Unlimited::default(),
            false,
        );
        let ack_receive_time = ack_receive_time + Duration::from_millis(500);
        let (result, context) = ack_packets(10..=10, ack_receive_time, &mut path, &mut manager);
        assert!(!result.unwrap().pto_reset);
        assert_eq!(context.on_packet_ack_count, 1);
        assert_eq!(context.on_new_packet_ack_count, 1);
        assert_eq!(context.validate_packet_ack_count, 1);
        assert_eq!(context.on_packet_loss_count, 0);
        assert_eq!(path.rtt_estimator.latest_rtt(), Duration::from_millis(3000));
    }

    #[compliance::tests("https://tools.ietf.org/id/draft-ietf-quic-recovery-30.txt#A.10")]
    #[test]
    fn detect_and_remove_lost_packets() {
        let space = PacketNumberSpace::ApplicationData;
        let mut rtt_estimator = RTTEstimator::new(Duration::from_millis(10));
        let mut manager = Manager::new(space, Duration::from_millis(100));
        let now = s2n_quic_platform::time::now();
        let mut path = Path::new(
            Default::default(),
            connection::Id::EMPTY,
            rtt_estimator,
            Unlimited::default(),
            true,
        );

        manager.largest_acked_packet = Some(space.new_packet_number(VarInt::from_u8(10)));

        let mut time_sent = s2n_quic_platform::time::now();
        let outcome = transmission::Outcome {
            ack_elicitation: AckElicitation::Eliciting,
            is_congestion_controlled: true,
            bytes_sent: 1,
        };

        // Send a packet that was sent too long ago (lost)
        let old_packet_time_sent = space.new_packet_number(VarInt::from_u8(8));
        manager.on_packet_sent(old_packet_time_sent, outcome, time_sent, &mut path);

        manager.time_threshold = Duration::from_secs(9);
        time_sent += Duration::from_secs(10);

        //Send a packet with a packet number K_PACKET_THRESHOLD away from the largest (lost)
        let old_packet_packet_number =
            space.new_packet_number(VarInt::new(10 - K_PACKET_THRESHOLD).unwrap());
        manager.on_packet_sent(old_packet_packet_number, outcome, time_sent, &mut path);

        // Send a packet that is less than the largest acked but not lost
        let not_lost = space.new_packet_number(VarInt::from_u8(9));
        manager.on_packet_sent(not_lost, outcome, time_sent, &mut path);

        // Send a packet larger than the largest acked (not lost)
        let larger_than_largest = manager.largest_acked_packet.unwrap().next().unwrap();
        manager.on_packet_sent(larger_than_largest, outcome, time_sent, &mut path);

        rtt_estimator.update_rtt(
            Duration::from_millis(10),
            Duration::from_millis(150),
            now,
            space,
        );

        // Four packets sent, each size 1 byte
        let bytes_in_flight: u16 = manager
            .sent_packets
            .iter()
            .map(|(_, info)| info.sent_bytes)
            .sum();
        assert_eq!(bytes_in_flight, 4);

        let now = time_sent;
        let mut lost_packets: HashSet<PacketNumber> = HashSet::default();

        let loss_info = manager.detect_and_remove_lost_packets(now, |packet_range| {
            lost_packets.insert(packet_range.start());
        });

        // Two packets lost, each size 1 byte
        assert_eq!(loss_info.bytes_in_flight, 2);
        // Two packets remaining
        let bytes_in_flight: u16 = manager
            .sent_packets
            .iter()
            .map(|(_, info)| info.sent_bytes)
            .sum();
        assert_eq!(bytes_in_flight, 2);

        let sent_packets = &manager.sent_packets;
        assert!(lost_packets.contains(&old_packet_time_sent));
        assert!(sent_packets.get(old_packet_time_sent).is_none());

        assert!(lost_packets.contains(&old_packet_packet_number));
        assert!(sent_packets.get(old_packet_packet_number).is_none());

        assert!(!lost_packets.contains(&larger_than_largest));
        assert!(sent_packets.get(larger_than_largest).is_some());

        assert!(!lost_packets.contains(&not_lost));
        assert!(sent_packets.get(not_lost).is_some());

        let expected_loss_time =
            sent_packets.get(not_lost).unwrap().time_sent + manager.time_threshold;
        assert!(manager.loss_timer.is_armed());
        assert_eq!(Some(&expected_loss_time), manager.loss_timer.iter().next());
    }

    #[compliance::tests("https://tools.ietf.org/id/draft-ietf-quic-recovery-31.txt#7.6.3")]
    #[test]
    fn persistent_congestion() {
        let space = PacketNumberSpace::ApplicationData;
        let rtt_estimator = RTTEstimator::new(Duration::from_millis(10));
        let mut manager = Manager::new(space, Duration::from_millis(100));
        let mut path = Path::new(
            Default::default(),
            connection::Id::EMPTY,
            rtt_estimator,
            Unlimited::default(),
            true,
        );
        let time_zero = s2n_quic_platform::time::now() + Duration::from_secs(10);
        manager.time_threshold = Duration::from_secs(3);
        // The RFC doesn't mention it, but it is implied that the first RTT sample has already
        // been received when this example begins, otherwise packet #2 would not be considered
        // part of the persistent congestion period.
        manager.first_rtt_sample = Some(s2n_quic_platform::time::now());

        let outcome = transmission::Outcome {
            ack_elicitation: AckElicitation::Eliciting,
            is_congestion_controlled: true,
            bytes_sent: 1,
        };

        // t=0: Send packet #1 (app data)
        manager.on_packet_sent(
            space.new_packet_number(VarInt::from_u8(1)),
            outcome,
            time_zero,
            &mut path,
        );

        // t=1: Send packet #2 (app data)
        manager.on_packet_sent(
            space.new_packet_number(VarInt::from_u8(2)),
            outcome,
            time_zero + Duration::from_secs(1),
            &mut path,
        );

        // t=1.2: Recv acknowledgement of #1
        let _ = ack_packets(
            1..=1,
            time_zero + Duration::from_millis(1200),
            &mut path,
            &mut manager,
        );

        // t=2-6: Send packets #3 - #7 (app data)
        for t in 2..=6 {
            manager.on_packet_sent(
                space.new_packet_number(VarInt::from_u8(t + 1)),
                outcome,
                time_zero + Duration::from_secs(t.into()),
                &mut path,
            );
        }

        // t=8: Send packet #8 (PTO 1)
        manager.on_packet_sent(
            space.new_packet_number(VarInt::from_u8(8)),
            outcome,
            time_zero + Duration::from_secs(8),
            &mut path,
        );

        // t=12: Send packet #9 (PTO 2)
        manager.on_packet_sent(
            space.new_packet_number(VarInt::from_u8(9)),
            outcome,
            time_zero + Duration::from_secs(12),
            &mut path,
        );

        // t=12.2: Recv acknowledgement of #9
        let (result, context) = ack_packets(
            9..=9,
            time_zero + Duration::from_millis(12200),
            &mut path,
            &mut manager,
        );

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-31.txt#7.6.3
        //# Packets 2 through 8 are declared lost when the acknowledgement for
        //# packet 9 is received at t = 12.2.
        assert_eq!(7, context.on_packet_loss_count);

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-31.txt#7.6.3
        //# The congestion period is calculated as the time between the oldest
        //# and newest lost packets: 8 - 1 = 7.
        assert_eq!(
            Duration::from_secs(7),
            result.unwrap().persistent_congestion_period
        );
    }

    #[compliance::tests("https://tools.ietf.org/id/draft-ietf-quic-recovery-31.txt#7.6")]
    #[test]
    fn persistent_congestion_multiple_periods() {
        let space = PacketNumberSpace::ApplicationData;
        let rtt_estimator = RTTEstimator::new(Duration::from_millis(10));
        let mut manager = Manager::new(space, Duration::from_millis(100));
        let mut path = Path::new(
            Default::default(),
            connection::Id::EMPTY,
            rtt_estimator,
            Unlimited::default(),
            true,
        );
        let time_zero = s2n_quic_platform::time::now() + Duration::from_secs(10);
        manager.time_threshold = Duration::from_secs(3);

        let outcome = transmission::Outcome {
            ack_elicitation: AckElicitation::Eliciting,
            is_congestion_controlled: true,
            bytes_sent: 1,
        };

        // t=0: Send packet #1 (app data)
        manager.on_packet_sent(
            space.new_packet_number(VarInt::from_u8(1)),
            outcome,
            time_zero,
            &mut path,
        );

        // t=1: Send packet #2 (app data)
        manager.on_packet_sent(
            space.new_packet_number(VarInt::from_u8(2)),
            outcome,
            time_zero + Duration::from_secs(1),
            &mut path,
        );

        // t=1.2: Recv acknowledgement of #1
        let _ = ack_packets(
            1..=1,
            time_zero + Duration::from_millis(1200),
            &mut path,
            &mut manager,
        );

        assert_eq!(
            manager.first_rtt_sample,
            Some(time_zero + Duration::from_millis(1200))
        );

        // t=2-6: Send packets #3 - #7 (app data)
        for t in 2..=6 {
            manager.on_packet_sent(
                space.new_packet_number(VarInt::from_u8(t + 1)),
                outcome,
                time_zero + Duration::from_secs(t.into()),
                &mut path,
            );
        }

        // Skip packet #8, which ends one persistent congestion period.

        // t=8: Send packet #9 (app data)
        manager.on_packet_sent(
            space.new_packet_number(VarInt::from_u8(9)),
            outcome,
            time_zero + Duration::from_secs(8),
            &mut path,
        );

        // t=16: Send packet #10 (app data)
        manager.on_packet_sent(
            space.new_packet_number(VarInt::from_u8(10)),
            outcome,
            time_zero + Duration::from_secs(16),
            &mut path,
        );

        // t=20: Send packet #11 (app data)
        manager.on_packet_sent(
            space.new_packet_number(VarInt::from_u8(11)),
            outcome,
            time_zero + Duration::from_secs(20),
            &mut path,
        );

        // t=20.2: Recv acknowledgement of #11
        let (result, context) = ack_packets(
            11..=11,
            time_zero + Duration::from_millis(20200),
            &mut path,
            &mut manager,
        );

        // Packets 2 though 7 and 9-10 should be lost
        assert_eq!(8, context.on_packet_loss_count);

        // The largest contiguous period of lost packets is #9 (sent at t8) to #10 (sent at t16)
        assert_eq!(
            Duration::from_secs(8),
            result.unwrap().persistent_congestion_period
        );
    }

    #[compliance::tests("https://tools.ietf.org/id/draft-ietf-quic-recovery-30.txt#6.2.1")]
    #[test]
    fn update() {
        let space = PacketNumberSpace::ApplicationData;
        let rtt_estimator = RTTEstimator::new(Duration::from_millis(10));
        let mut manager = Manager::new(space, Duration::from_millis(10));
        let now = s2n_quic_platform::time::now() + Duration::from_secs(10);
        let pto_backoff = 2;
        let is_handshake_confirmed = true;

        let mut path = Path::new(
            Default::default(),
            connection::Id::EMPTY,
            rtt_estimator,
            Unlimited::default(),
            false,
        );

        path.rtt_estimator.update_rtt(
            Duration::from_millis(0),
            Duration::from_millis(500),
            now,
            space,
        );
        path.rtt_estimator.update_rtt(
            Duration::from_millis(0),
            Duration::from_millis(1000),
            now,
            space,
        );
        // The path will be at the anti-amplification limit
        path.on_bytes_transmitted((1200 * 2) + 1);
        // Arm the PTO so we can verify it is cancelled
        manager.pto.timer.set(now + Duration::from_secs(10));
        manager.update(&path, pto_backoff, now, is_handshake_confirmed);

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-30.txt#6.1.2
        //# The time threshold is:
        //#
        //# max(kTimeThreshold * max(smoothed_rtt, latest_rtt), kGranularity)
        // time_threshold = max(9/8 * max(<1000, 1000), 1) = 1125
        assert_eq!(manager.time_threshold, Duration::from_millis(1125));
        // PTO is not armed because the path was at anti-amplification limit
        assert!(!manager.pto.timer.is_armed());

        // Arm the PTO so we can verify it is cancelled
        manager.pto.timer.set(now + Duration::from_secs(10));
        // Validate the path so it is not at the anti-amplification limit
        path.on_validated();
        path.on_peer_validated();
        manager.update(&path, pto_backoff, now, is_handshake_confirmed);

        // Since the path is peer validated and sent packets is empty, PTO is cancelled
        assert!(!manager.pto.timer.is_armed());

        // Reset the path back to not peer validated
        let mut path = Path::new(
            Default::default(),
            connection::Id::EMPTY,
            rtt_estimator,
            Unlimited::default(),
            false,
        );
        path.on_validated();
        let is_handshake_confirmed = false;
        manager.update(&path, pto_backoff, now, is_handshake_confirmed);

        // Since the packet space is Application and the handshake is not confirmed, PTO is cancelled
        assert!(!manager.pto.timer.is_armed());

        // Set is handshake confirmed back to true
        let is_handshake_confirmed = true;
        manager.update(&path, pto_backoff, now, is_handshake_confirmed);

        // Now the PTO is armed
        assert!(manager.pto.timer.is_armed());

        // Send a packet to validate behavior when sent_packets is not empty
        manager.on_packet_sent(
            space.new_packet_number(VarInt::from_u8(1)),
            transmission::Outcome {
                ack_elicitation: AckElicitation::Eliciting,
                is_congestion_controlled: true,
                bytes_sent: 1,
            },
            now,
            &mut path,
        );

        let expected_pto_base_timestamp = now - Duration::from_secs(5);
        manager.time_of_last_ack_eliciting_packet = Some(expected_pto_base_timestamp);
        // This will update the smoother_rtt to 2000, and rtt_var to 1000
        path.rtt_estimator.update_rtt(
            Duration::from_millis(0),
            Duration::from_millis(2000),
            now,
            space,
        );
        manager.update(&path, pto_backoff, now, is_handshake_confirmed);

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-30.txt#6.2.1
        //# When an ack-eliciting packet is transmitted, the sender schedules a
        //# timer for the PTO period as follows:
        //#
        //# PTO = smoothed_rtt + max(4*rttvar, kGranularity) + max_ack_delay
        // Including the pto backoff (2) =:
        // PTO = (2000 + max(4*1000, 1) + 10) * 2 = 12020
        assert!(manager.pto.timer.is_armed());
        assert_eq!(
            *manager.pto.timer.iter().next().unwrap(),
            expected_pto_base_timestamp + Duration::from_millis(12020)
        );
    }

    #[compliance::tests("https://tools.ietf.org/id/draft-ietf-quic-recovery-30.txt#6.2.1")]
    #[test]
    fn on_timeout() {
        let space = PacketNumberSpace::ApplicationData;
        let mut manager = Manager::new(space, Duration::from_millis(10));
        let now = s2n_quic_platform::time::now() + Duration::from_secs(10);
        manager.largest_acked_packet = Some(space.new_packet_number(VarInt::from_u8(10)));

        let mut context = MockContext::default();
        let mut path = Path::new(
            Default::default(),
            connection::Id::EMPTY,
            RTTEstimator::new(Duration::from_millis(10)),
            Unlimited::default(),
            false,
        );

        // Loss timer is armed but not expired yet, nothing happens
        manager.loss_timer.set(now + Duration::from_secs(10));
        let mut loss_info = manager.on_timeout(now, &mut context);
        assert_eq!(context.on_packet_loss_count, 0);
        assert!(!manager.pto.timer.is_armed());
        assert!(!loss_info.pto_expired);

        // Send a packet that will be considered lost
        manager.on_packet_sent(
            space.new_packet_number(VarInt::from_u8(1)),
            transmission::Outcome {
                ack_elicitation: AckElicitation::Eliciting,
                is_congestion_controlled: true,
                bytes_sent: 1,
            },
            now - Duration::from_secs(5),
            &mut path,
        );

        // Loss timer is armed and expired, on_packet_loss is called
        manager.loss_timer.set(now - Duration::from_secs(1));
        loss_info = manager.on_timeout(now, &mut context);
        assert_eq!(context.on_packet_loss_count, 1);
        assert!(!manager.pto.timer.is_armed());
        assert!(!loss_info.pto_expired);

        // Loss timer is not armed, pto timer is not armed
        manager.loss_timer.cancel();
        loss_info = manager.on_timeout(now, &mut context);
        assert!(!loss_info.pto_expired);

        // Loss timer is not armed, pto timer is armed but not expired
        manager.loss_timer.cancel();
        manager.pto.timer.set(now + Duration::from_secs(5));
        loss_info = manager.on_timeout(now, &mut context);
        assert!(!loss_info.pto_expired);

        // Loss timer is not armed, pto timer is expired without bytes in flight
        manager.pto.timer.set(now - Duration::from_secs(5));
        loss_info = manager.on_timeout(now, &mut context);
        assert!(loss_info.pto_expired);
        assert_eq!(manager.pto.state, RequiresTransmission(1));

        // Loss timer is not armed, pto timer is expired with bytes in flight
        manager.sent_packets.insert(
            space.new_packet_number(VarInt::from_u8(1)),
            SentPacketInfo {
                in_flight: true,
                sent_bytes: 1,
                time_sent: now,
            },
        );
        manager.pto.timer.set(now - Duration::from_secs(5));
        loss_info = manager.on_timeout(now, &mut context);
        assert!(loss_info.pto_expired);
        assert_eq!(manager.pto.state, RequiresTransmission(2));
    }

    #[test]
    fn timers() {
        let space = PacketNumberSpace::ApplicationData;
        let mut manager = Manager::new(space, Duration::from_millis(10));
        let loss_time = s2n_quic_platform::time::now() + Duration::from_secs(5);
        let pto_time = s2n_quic_platform::time::now() + Duration::from_secs(10);

        // No timer is set
        assert_eq!(manager.timers().count(), 0);

        // Loss timer is armed
        manager.loss_timer.set(loss_time);
        assert_eq!(manager.timers().count(), 1);
        assert_eq!(manager.timers().next(), Some(&loss_time));

        // PTO timer is armed
        manager.loss_timer.cancel();
        manager.pto.timer.set(pto_time);
        assert_eq!(manager.timers().count(), 1);
        assert_eq!(manager.timers().next(), Some(&pto_time));

        // Both timers are armed, only loss time is returned
        manager.loss_timer.set(loss_time);
        manager.pto.timer.set(pto_time);
        assert_eq!(manager.timers().count(), 1);
        assert_eq!(manager.timers().next(), Some(&loss_time));
    }

    #[test]
    fn on_transmit() {
        let space = PacketNumberSpace::ApplicationData;
        let mut manager = Manager::new(space, Duration::from_millis(10));
        let connection_context = MockConnectionContext::new(EndpointType::Client);
        let mut frame_buffer = OutgoingFrameBuffer::new();
        let mut context = MockWriteContext::new(
            &connection_context,
            s2n_quic_platform::time::now(),
            &mut frame_buffer,
        );

        // Already idle
        manager.pto.state = PtoState::Idle;
        manager.on_transmit(&mut context);
        assert_eq!(manager.pto.state, PtoState::Idle);

        // No transmissions required
        manager.pto.state = RequiresTransmission(0);
        manager.on_transmit(&mut context);
        assert_eq!(manager.pto.state, PtoState::Idle);

        // One transmission required, not ack eliciting
        manager.pto.state = RequiresTransmission(1);
        context.write_frame(&frame::Padding { length: 1 });
        manager.on_transmit(&mut context);
        assert_eq!(manager.pto.state, PtoState::Idle);

        // One transmission required, ack eliciting
        manager.pto.state = RequiresTransmission(1);
        context.write_frame(&frame::Ping);
        manager.on_transmit(&mut context);
        assert_eq!(manager.pto.state, PtoState::Idle);

        // Two transmissions required
        manager.pto.state = RequiresTransmission(2);
        manager.on_transmit(&mut context);
        assert_eq!(manager.pto.state, RequiresTransmission(1));
    }

    // Helper function that will call on_ack_frame with the given packet numbers
    fn ack_packets<CC: CongestionController>(
        range: RangeInclusive<u8>,
        ack_receive_time: Timestamp,
        path: &mut Path<CC>,
        manager: &mut Manager,
    ) -> (Result<LossInfo, TransportError>, MockContext) {
        let acked_packets = PacketNumberRange::new(
            manager
                .space
                .new_packet_number(VarInt::from_u8(*range.start())),
            manager
                .space
                .new_packet_number(VarInt::from_u8(*range.end())),
        );

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

        let mut context = MockContext::default();
        let result = manager.on_ack_frame(&datagram, frame, path, 1, &mut context);

        for packet in acked_packets {
            assert!(manager.sent_packets.get(packet).is_none());
        }

        (result, context)
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

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    contexts::WriteContext,
    path::{self, Path},
    recovery::{
        context::Context,
        pto::{Pto, PtoState},
        SentPacketInfo, SentPackets,
    },
    timer::VirtualTimer,
    transmission,
};
use core::{cmp::max, time::Duration};
use s2n_quic_core::{
    frame,
    inet::DatagramInfo,
    packet::number::{PacketNumber, PacketNumberRange, PacketNumberSpace},
    recovery::{CongestionController, RttEstimator, K_GRANULARITY},
    time::Timestamp,
    transport,
    varint::VarInt,
};
use smallvec::SmallVec;

#[derive(Debug)]
pub struct Manager {
    // The packet space for this recovery manager
    pub(crate) space: PacketNumberSpace,

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#A.3
    //# The maximum amount of time by which the receiver
    //# intends to delay acknowledgments for packets in the Application
    //# Data packet number space, as defined by the eponymous transport
    //# parameter (Section 18.2 of [QUIC-TRANSPORT]).  Note that the
    //# actual ack_delay in a received ACK frame may be larger due to late
    //# timers, reordering, or loss.
    max_ack_delay: Duration,

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#A.3
    //# The largest packet number acknowledged in the packet number space so far.
    pub(crate) largest_acked_packet: Option<PacketNumber>,

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#A.3
    //# An association of packet numbers in a packet number space to information about them.
    //  These are packets that are pending acknowledgement.
    pub(crate) sent_packets: SentPackets,

    // Timer set when packets may be declared lost at a time in the future
    loss_timer: VirtualTimer,

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.2
    //# A Probe Timeout (PTO) triggers sending one or two probe datagrams
    //# when ack-eliciting packets are not acknowledged within the expected
    //# period of time or the server may not have validated the client's
    //# address.  A PTO enables a connection to recover from loss of tail
    //# packets or acknowledgements.
    pub(crate) pto: Pto,

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#A.3
    //# The time the most recent ack-eliciting packet was sent.
    time_of_last_ack_eliciting_packet: Option<Timestamp>,

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#B.2
    //# The highest value reported for the ECN-CE counter in the packet
    //# number space by the peer in an ACK frame.  This value is used to
    //# detect increases in the reported ECN-CE counter.
    ecn_ce_counter: VarInt,
}

//= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.1.1
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
            time_of_last_ack_eliciting_packet: None,
            ecn_ce_counter: VarInt::default(),
        }
    }

    pub fn timers(&self) -> impl Iterator<Item = Timestamp> {
        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.2.1
        //# The PTO timer MUST NOT be set if a timer is set for time threshold
        //# loss detection; see Section 6.1.2.  A timer that is set for time
        //# threshold loss detection will expire earlier than the PTO timer in
        //# most cases and is less likely to spuriously retransmit data.

        let is_loss_timer_armed = self.loss_timer.is_armed();

        core::iter::empty()
            .chain(self.pto.timers())
            .filter(move |_| !is_loss_timer_armed)
            .chain(self.loss_timer.iter())
    }

    pub fn on_timeout<CC: CongestionController, Ctx: Context<CC>>(
        &mut self,
        timestamp: Timestamp,
        context: &mut Ctx,
    ) {
        if self.loss_timer.is_armed() {
            if self.loss_timer.poll_expiration(timestamp).is_ready() {
                self.detect_and_remove_lost_packets(timestamp, context)
            }
        } else {
            let pto_expired = self
                .pto
                .on_timeout(!self.sent_packets.is_empty(), timestamp);

            //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.2
            //# A PTO timer expiration event does not indicate packet loss and MUST
            //# NOT cause prior unacknowledged packets to be marked as lost.

            //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.2.1
            //# When a PTO timer expires, the PTO backoff MUST be increased,
            //# resulting in the PTO period being set to twice its current value.
            if pto_expired {
                // Note: the psuedocode updates the pto timer in OnLossDetectionTimeout
                // (see section A.9). We don't do that here since it will be rearmed in
                // `on_packet_sent`, which immediately follows a timeout.
                context.path_mut().pto_backoff *= 2;
            }
        }
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#A.5
    //# After a packet is sent, information about the packet is stored.
    pub fn on_packet_sent<CC: CongestionController, Ctx: Context<CC>>(
        &mut self,
        packet_number: PacketNumber,
        outcome: transmission::Outcome,
        time_sent: Timestamp,
        path_id: path::Id,
        context: &mut Ctx,
    ) {
        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#7
        //# Similar to TCP, packets containing only ACK frames do not count
        //# towards bytes in flight and are not congestion controlled.

        // Everything else (including probe packets) are counted, as specified below:
        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#7.5
        //# A sender MUST however count these packets as being additionally in
        //# flight, since these packets add network load without establishing
        //# packet loss.
        let congestion_controlled_bytes = if outcome.is_congestion_controlled {
            outcome.bytes_sent
        } else {
            0
        };

        self.sent_packets.insert(
            packet_number,
            SentPacketInfo::new(
                outcome.is_congestion_controlled,
                congestion_controlled_bytes,
                time_sent,
                outcome.ack_elicitation,
                path_id,
            ),
        );

        if outcome.is_congestion_controlled {
            if outcome.ack_elicitation.is_ack_eliciting() {
                self.time_of_last_ack_eliciting_packet = Some(time_sent);
            }
            //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.2.1
            //# A sender SHOULD restart its PTO timer every time an ack-eliciting
            //# packet is sent or acknowledged,
            let is_handshake_confirmed = context.is_handshake_confirmed();
            let path = context.path_mut();
            self.update_pto_timer(path, time_sent, is_handshake_confirmed);
            path.congestion_controller
                .on_packet_sent(time_sent, congestion_controlled_bytes);
        }
    }

    /// Updates the PTO timer
    pub fn update_pto_timer<CC: CongestionController>(
        &mut self,
        path: &Path<CC>,
        now: Timestamp,
        is_handshake_confirmed: bool,
    ) {
        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.2.2.1
        //# If no additional data can be sent, the server's PTO timer MUST NOT be
        //# armed until datagrams have been received from the client, because
        //# packets sent on PTO count against the anti-amplification limit.
        if path.at_amplification_limit() {
            // The server's timer is not set if nothing can be sent.
            self.pto.cancel();
            return;
        }

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.2.2.1
        //# it is the client's responsibility to send packets to unblock the server
        //# until it is certain that the server has finished its address validation
        if self.sent_packets.is_empty() && path.is_peer_validated() {
            // There is nothing to detect lost, so no timer is set.
            // However, the client needs to arm the timer if the
            // server might be blocked by the anti-amplification limit.
            self.pto.cancel();
            return;
        }

        let ack_eliciting_packets_in_flight = self.sent_packets.iter().any(|(_, sent_info)| {
            sent_info.congestion_controlled && sent_info.ack_elicitation.is_ack_eliciting()
        });

        let pto_base_timestamp = if ack_eliciting_packets_in_flight {
            self.time_of_last_ack_eliciting_packet
                .expect("there is at least one ack eliciting packet in flight")
        } else {
            // Arm PTO from now when there are no inflight packets.
            //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.2.2.1
            //# That is, the client MUST set the probe timer if the client has not received an
            //# acknowledgement for one of its Handshake packets and the handshake is
            //# not confirmed (see Section 4.1.2 of [QUIC-TLS]), even if there are no
            //# packets in flight.
            now
        };

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.2.1
        //# An endpoint MUST NOT set its PTO timer for the application data
        //# packet number space until the handshake is confirmed.
        if self.space.is_application_data() && !is_handshake_confirmed {
            self.pto.cancel();
        } else {
            self.pto
                .update(pto_base_timestamp, path.pto_period(self.space));
        }
    }

    /// Queries the component for any outgoing frames that need to get sent
    pub fn on_transmit<W: WriteContext>(&mut self, context: &mut W) {
        self.pto.on_transmit(context)
    }

    pub fn on_ack_frame<A: frame::ack::AckRanges, CC: CongestionController, Ctx: Context<CC>>(
        &mut self,
        datagram: &DatagramInfo,
        frame: frame::Ack<A>,
        context: &mut Ctx,
    ) -> Result<(), transport::Error> {
        let largest_acked_in_frame = self.space.new_packet_number(frame.largest_acknowledged());
        let mut newly_acked_packets =
            SmallVec::<[SentPacketInfo; ACKED_PACKETS_INITIAL_CAPACITY]>::new();

        // Update the largest acked packet if the largest packet acked in this frame is larger
        self.largest_acked_packet = Some(
            self.largest_acked_packet
                .map_or(largest_acked_in_frame, |pn| pn.max(largest_acked_in_frame)),
        );

        //# https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#5.1
        //# An endpoint generates an RTT sample on receiving an ACK frame that
        //# meets the following two conditions:
        //#
        //# *  the largest acknowledged packet number is newly acknowledged, and
        //#
        //# *  at least one of the newly acknowledged packets was ack-eliciting.
        let mut largest_newly_acked: Option<(PacketNumber, SentPacketInfo)> = None;
        let mut includes_ack_eliciting = false;

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
                    includes_ack_eliciting |= acked_packet_info.ack_elicitation.is_ack_eliciting();
                }
            }

            if new_packet_ack {
                // notify components of packets that are newly acked
                context.on_new_packet_ack(datagram, &acked_packets);
            }
        }

        if largest_newly_acked.is_none() {
            // Nothing to do if there are no newly acked packets.
            return Ok(());
        }

        let largest_newly_acked = largest_newly_acked.expect("There are newly acked packets");

        let mut should_update_rtt = true;
        let is_handshake_confirmed = context.is_handshake_confirmed();

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#5.1
        //# To avoid generating multiple RTT samples for a single packet, an ACK
        //# frame SHOULD NOT be used to update RTT estimates if it does not newly
        //# acknowledge the largest acknowledged packet.
        should_update_rtt &= largest_newly_acked.0 == largest_acked_in_frame;

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#5.1
        //# An RTT sample MUST NOT be generated on receiving an ACK frame that
        //# does not newly acknowledge at least one ack-eliciting packet.
        should_update_rtt &= includes_ack_eliciting;

        if should_update_rtt {
            let latest_rtt = datagram.timestamp - largest_newly_acked.1.time_sent;
            let path = context.path_mut();
            path.rtt_estimator.update_rtt(
                frame.ack_delay(),
                latest_rtt,
                datagram.timestamp,
                is_handshake_confirmed,
                largest_acked_in_frame.space(),
            );

            // Update the congestion controller with the latest RTT estimate
            path.congestion_controller
                .on_rtt_update(largest_newly_acked.1.time_sent, &path.rtt_estimator);

            // Notify components the RTT estimate was updated
            context.on_rtt_update();
        }

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#7.1
        //# If a path has been validated to support ECN ([RFC3168], [RFC8311]),
        //# QUIC treats a Congestion Experienced (CE) codepoint in the IP header
        //# as a signal of congestion.
        if let Some(ecn_counts) = frame.ecn_counts {
            if ecn_counts.ce_count > self.ecn_ce_counter {
                self.ecn_ce_counter = ecn_counts.ce_count;
                context
                    .path_mut()
                    .congestion_controller
                    .on_congestion_event(datagram.timestamp);
            }
        }

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.1.2
        //# Once a later packet within the same packet number space has been
        //# acknowledged, an endpoint SHOULD declare an earlier packet lost if it
        //# was sent a threshold amount of time in the past.
        self.detect_and_remove_lost_packets(datagram.timestamp, context);

        let path = context.path_mut();
        for acked_packet_info in newly_acked_packets {
            path.congestion_controller.on_packet_ack(
                largest_newly_acked.1.time_sent,
                acked_packet_info.sent_bytes as usize,
                &path.rtt_estimator,
                datagram.timestamp,
            );
        }

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.2.1
        //# The PTO backoff factor is reset when an acknowledgement is received,
        //# except in the following case.  A server might take longer to respond
        //# to packets during the handshake than otherwise.  To protect such a
        //# server from repeated client probes, the PTO backoff is not reset at a
        //# client that is not yet certain that the server has finished
        //# validating the client's address.  That is, a client does not reset
        //# the PTO backoff factor on receiving acknowledgements in Initial
        //# packets.
        if context.path().is_peer_validated() {
            context.path_mut().reset_pto_backoff();
        }

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.2.1
        //# A sender SHOULD restart its PTO timer every time an ack-eliciting
        //# packet is sent or acknowledged,
        let is_handshake_confirmed = context.is_handshake_confirmed();
        self.update_pto_timer(
            context.path_mut(),
            datagram.timestamp,
            self.space.is_application_data() && is_handshake_confirmed,
        );

        Ok(())
    }

    /// Returns `true` if the recovery manager requires a probe packet to be sent.
    pub fn requires_probe(&self) -> bool {
        matches!(self.pto.state, PtoState::RequiresTransmission(_))
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#B.9
    //# When Initial or Handshake keys are discarded, packets sent in that
    //# space no longer count toward bytes in flight.
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

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#A.10
    //# DetectAndRemoveLostPackets is called every time an ACK is received or the time threshold
    //# loss detection timer expires. This function operates on the sent_packets for that packet
    //# number space and returns a list of packets newly detected as lost.
    fn detect_and_remove_lost_packets<CC: CongestionController, Ctx: Context<CC>>(
        &mut self,
        now: Timestamp,
        context: &mut Ctx,
    ) {
        let path = context.path_mut();
        // Cancel the loss timer. It will be armed again if any unacknowledged packets are
        // older than the largest acked packet, but not old enough to be considered lost yet
        self.loss_timer.cancel();
        // Calculate how long we wait until a packet is declared lost
        let time_threshold = Self::calculate_loss_time_threshold(&path.rtt_estimator);
        // Packets sent before this time are deemed lost.
        let lost_send_time = now.checked_sub(time_threshold);

        // TODO: Investigate a more efficient mechanism for managing sent_packets
        //       See https://github.com/awslabs/s2n-quic/issues/69
        let mut sent_packets_to_remove = Vec::new();

        let largest_acked_packet = &self
            .largest_acked_packet
            .expect("This function is only called after an ack has been received");

        let mut lost_bytes = 0;
        let mut persistent_congestion_period = Duration::from_secs(0);
        let mut max_persistent_congestion_period = Duration::from_secs(0);
        let mut prev_packet: Option<(&PacketNumber, Timestamp)> = None;

        for (unacked_packet_number, unacked_sent_info) in self.sent_packets.iter() {
            if unacked_packet_number > largest_acked_packet {
                // sent_packets is ordered by packet number, so all remaining packets will be larger
                break;
            }

            let time_threshold_exceeded = lost_send_time.map_or(false, |lost_send_time| {
                unacked_sent_info.time_sent <= lost_send_time
            });

            let packet_number_threshold_exceeded = largest_acked_packet
                .checked_distance(*unacked_packet_number)
                .expect("largest_acked_packet >= unacked_packet_number")
                >= K_PACKET_THRESHOLD;

            //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.1
            //# A packet is declared lost if it meets all the following conditions:
            //#
            //#    *  The packet is unacknowledged, in-flight, and was sent prior to an
            //#       acknowledged packet.
            //#
            //#    *  The packet was sent kPacketThreshold packets before an
            //#       acknowledged packet (Section 6.1.1), or it was sent long enough in
            //#       the past (Section 6.1.2).
            if time_threshold_exceeded || packet_number_threshold_exceeded {
                sent_packets_to_remove.push(*unacked_packet_number);

                if unacked_sent_info.congestion_controlled {
                    // The packet is "in-flight", ie congestion controlled
                    lost_bytes += unacked_sent_info.sent_bytes as u32;
                    // TODO merge contiguous packet numbers
                    let range =
                        PacketNumberRange::new(*unacked_packet_number, *unacked_packet_number);
                    context.on_packet_loss(&range);
                }

                //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#7.6.2
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
                    max_persistent_congestion_period = max(
                        max_persistent_congestion_period,
                        persistent_congestion_period,
                    );
                } else {
                    // There was a gap in packet number or this is the beginning of the period.
                    // Reset the current period to zero.
                    persistent_congestion_period = Duration::from_secs(0);
                }
            } else {
                //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.1.2
                //# If packets sent prior to the largest acknowledged packet cannot yet
                //# be declared lost, then a timer SHOULD be set for the remaining time.
                self.loss_timer
                    .set(unacked_sent_info.time_sent + time_threshold);

                //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.2.1
                //# The PTO timer MUST NOT be set if a timer is set for time threshold
                //# loss detection; see Section 6.1.2.  A timer that is set for time
                //# threshold loss detection will expire earlier than the PTO timer in
                //# most cases and is less likely to spuriously retransmit data.
                self.pto.cancel();

                // assuming sent_packets is ordered by packet number and sent time, all remaining
                // packets will have a larger packet number and sent time, and are thus not lost.
                break;
            }

            //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#7.6.2
            //# The persistent congestion period SHOULD NOT start until there is at
            //# least one RTT sample.  Before the first RTT sample, a sender arms its
            //# PTO timer based on the initial RTT (Section 6.2.2), which could be
            //# substantially larger than the actual RTT.  Requiring a prior RTT
            //# sample prevents a sender from establishing persistent congestion with
            //# potentially too few probes.
            if context
                .path()
                .rtt_estimator
                .first_rtt_sample()
                .map_or(false, |ts| unacked_sent_info.time_sent > ts)
            {
                prev_packet = Some((unacked_packet_number, unacked_sent_info.time_sent));
            }
        }

        for packet_number in sent_packets_to_remove {
            self.sent_packets.remove(packet_number);
        }

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#7.6.2
        //# A sender that does not have state for all packet
        //# number spaces or an implementation that cannot compare send times
        //# across packet number spaces MAY use state for just the packet number
        //# space that was acknowledged.
        let persistent_congestion = max_persistent_congestion_period
            > context
                .path()
                .rtt_estimator
                .persistent_congestion_threshold();

        if lost_bytes > 0 {
            context.path_mut().congestion_controller.on_packets_lost(
                lost_bytes,
                persistent_congestion,
                now,
            );
        }

        if persistent_congestion {
            //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#5.2
            //# Endpoints SHOULD set the min_rtt to the newest RTT sample after
            //# persistent congestion is established.
            context.path_mut().rtt_estimator.on_persistent_congestion();
        }
    }

    fn calculate_loss_time_threshold(rtt_estimator: &RttEstimator) -> Duration {
        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.1.2
        //# The time threshold is:
        //#
        //# max(kTimeThreshold * max(smoothed_rtt, latest_rtt), kGranularity)
        let mut time_threshold = max(rtt_estimator.smoothed_rtt(), rtt_estimator.latest_rtt());

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.1.2
        //# The RECOMMENDED time threshold (kTimeThreshold), expressed as a
        //# round-trip time multiplier, is 9/8.
        time_threshold = (time_threshold * 9) / 8;

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.1.2
        //# To avoid declaring
        //# packets as lost too early, this time threshold MUST be set to at
        //# least the local timer granularity, as indicated by the kGranularity
        //# constant.
        max(time_threshold, K_GRANULARITY)
    }
}

impl transmission::interest::Provider for Manager {
    fn transmission_interest(&self) -> transmission::Interest {
        self.pto.transmission_interest()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{
        path::{self, Path},
        recovery::{
            context::mock::MockContext, manager::Manager, pto::PtoState::RequiresTransmission,
        },
        space::rx_packet_numbers::ack_ranges::AckRanges,
    };
    use core::{ops::RangeInclusive, time::Duration};
    use s2n_quic_core::{
        connection,
        frame::ack_elicitation::AckElicitation,
        packet::number::PacketNumberSpace,
        path::INITIAL_PTO_BACKOFF,
        recovery::{
            congestion_controller::testing::mock::CongestionController as MockCongestionController,
            DEFAULT_INITIAL_RTT,
        },
        varint::VarInt,
    };

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#A.5
    //= type=test
    #[test]
    fn on_packet_sent() {
        let space = PacketNumberSpace::ApplicationData;
        let max_ack_delay = Duration::from_millis(100);
        let mut manager = Manager::new(space, max_ack_delay);
        let now = s2n_quic_platform::time::now();
        let mut time_sent = now;
        let mut context = MockContext::new(max_ack_delay, true);
        // Call on validated so the path is not amplification limited so we can verify PTO arming
        context.path.on_validated();

        // PTO = smoothed_rtt + max(4*rttvar, kGranularity) + max_ack_delay
        // PTO = DEFAULT_INITIAL_RTT + 4*DEFAULT_INITIAL_RTT/2 + 10
        let expected_pto_duration = DEFAULT_INITIAL_RTT + 2 * DEFAULT_INITIAL_RTT + max_ack_delay;
        let mut expected_bytes_in_flight = 0;

        for i in 1..=10 {
            // Reset the timer so we can confirm it was set correctly
            manager.pto.timer.cancel();

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
                packet_number: space.new_packet_number(VarInt::from_u8(1)),
            };

            manager.on_packet_sent(
                sent_packet,
                outcome,
                time_sent,
                path::Id::new(0),
                &mut context,
            );

            assert!(manager.sent_packets.get(sent_packet).is_some());
            let actual_sent_packet = manager.sent_packets.get(sent_packet).unwrap();
            assert_eq!(
                actual_sent_packet.congestion_controlled,
                outcome.is_congestion_controlled
            );
            assert_eq!(actual_sent_packet.time_sent, time_sent);

            if outcome.is_congestion_controlled {
                assert_eq!(actual_sent_packet.sent_bytes as usize, outcome.bytes_sent);

                let expected_pto;
                if outcome.ack_elicitation.is_ack_eliciting() {
                    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.2.1
                    //= type=test
                    //# A sender SHOULD restart its PTO timer every time an ack-eliciting
                    //# packet is sent
                    expected_pto = time_sent + expected_pto_duration;
                } else if let Some(time_of_last_ack_eliciting_packet) =
                    manager.time_of_last_ack_eliciting_packet
                {
                    expected_pto = time_of_last_ack_eliciting_packet + expected_pto_duration;
                } else {
                    // No ack eliciting packets have been sent yet
                    expected_pto = time_sent + expected_pto_duration;
                }

                assert!(manager.pto.timer.is_armed());
                assert_eq!(Some(expected_pto), manager.pto.timer.iter().next());

                expected_bytes_in_flight += outcome.bytes_sent;
            } else {
                assert_eq!(actual_sent_packet.sent_bytes, 0);
            }

            time_sent += Duration::from_millis(10);
        }

        assert_eq!(manager.sent_packets.iter().count(), 10);
        assert_eq!(
            expected_bytes_in_flight as u32,
            context.path.congestion_controller.bytes_in_flight
        );
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#A.7
    //= type=test
    #[test]
    fn on_ack_frame() {
        let space = PacketNumberSpace::ApplicationData;
        let mut manager = Manager::new(space, Duration::from_millis(100));
        let packet_bytes = 128;
        let mut context = MockContext::default();

        // Start the pto backoff at 2 so we can tell if it was reset
        context.path.pto_backoff = 2;

        let time_sent = s2n_quic_platform::time::now() + Duration::from_secs(10);

        // Send packets 1 to 10
        for i in 1..=10 {
            manager.on_packet_sent(
                space.new_packet_number(VarInt::from_u8(i)),
                transmission::Outcome {
                    ack_elicitation: AckElicitation::Eliciting,
                    is_congestion_controlled: true,
                    bytes_sent: packet_bytes,
                    packet_number: space.new_packet_number(VarInt::from_u8(1)),
                },
                time_sent,
                path::Id::new(0),
                &mut context,
            );
        }

        assert_eq!(manager.sent_packets.iter().count(), 10);

        // Ack packets 1 to 3
        let ack_receive_time = time_sent + Duration::from_millis(500);
        ack_packets(1..=3, ack_receive_time, &mut context, &mut manager);

        assert_eq!(context.path.congestion_controller.lost_bytes, 0);
        assert_eq!(context.path.congestion_controller.on_rtt_update, 1);
        assert_eq!(context.path.pto_backoff, INITIAL_PTO_BACKOFF);
        assert_eq!(manager.sent_packets.iter().count(), 7);
        assert_eq!(
            manager.largest_acked_packet,
            Some(space.new_packet_number(VarInt::from_u8(3)))
        );
        assert_eq!(context.on_packet_ack_count, 1);
        assert_eq!(context.on_new_packet_ack_count, 1);
        assert_eq!(context.validate_packet_ack_count, 1);
        assert_eq!(context.on_packet_loss_count, 0);
        assert_eq!(
            context.path.rtt_estimator.latest_rtt(),
            Duration::from_millis(500)
        );
        assert_eq!(1, context.on_rtt_update_count);

        // Reset the pto backoff to 2 so we can tell if it was reset
        context.path.pto_backoff = 2;

        // Acknowledging already acked packets
        let ack_receive_time = ack_receive_time + Duration::from_secs(1);
        ack_packets(1..=3, ack_receive_time, &mut context, &mut manager);

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#5.1
        //= type=test
        //# An RTT sample MUST NOT be generated on receiving an ACK frame that
        //# does not newly acknowledge at least one ack-eliciting packet.

        // Acknowledging already acked packets does not call on_new_packet_ack or change RTT
        assert_eq!(context.path.congestion_controller.lost_bytes, 0);
        assert_eq!(context.path.congestion_controller.on_rtt_update, 1);
        assert_eq!(context.path.pto_backoff, 2);
        assert_eq!(context.on_packet_ack_count, 2);
        assert_eq!(context.on_new_packet_ack_count, 1);
        assert_eq!(context.validate_packet_ack_count, 2);
        assert_eq!(context.on_packet_loss_count, 0);
        assert_eq!(
            context.path.rtt_estimator.latest_rtt(),
            Duration::from_millis(500)
        );
        assert_eq!(1, context.on_rtt_update_count);

        // Ack packets 7 to 9 (4 - 6 will be considered lost)
        let ack_receive_time = ack_receive_time + Duration::from_secs(1);
        ack_packets(7..=9, ack_receive_time, &mut context, &mut manager);

        assert_eq!(
            context.path.congestion_controller.lost_bytes,
            (packet_bytes * 3) as u32
        );
        assert_eq!(context.path.pto_backoff, INITIAL_PTO_BACKOFF);
        assert_eq!(context.on_packet_ack_count, 3);
        assert_eq!(context.on_new_packet_ack_count, 2);
        assert_eq!(context.validate_packet_ack_count, 3);
        assert_eq!(context.on_packet_loss_count, 3);
        assert_eq!(
            context.path.rtt_estimator.latest_rtt(),
            Duration::from_millis(2500)
        );
        assert_eq!(2, context.on_rtt_update_count);

        // Ack packet 10, but with a path that is not peer validated
        context.path = Path::new(
            Default::default(),
            connection::PeerId::TEST_ID,
            context.path.rtt_estimator,
            MockCongestionController::default(),
            false,
        );
        context.path.pto_backoff = 2;
        let ack_receive_time = ack_receive_time + Duration::from_millis(500);
        ack_packets(10..=10, ack_receive_time, &mut context, &mut manager);
        assert_eq!(context.path.congestion_controller.on_rtt_update, 1);
        assert_eq!(context.path.pto_backoff, 2);
        assert_eq!(context.on_packet_ack_count, 4);
        assert_eq!(context.on_new_packet_ack_count, 3);
        assert_eq!(context.validate_packet_ack_count, 4);
        assert_eq!(context.on_packet_loss_count, 3);
        assert_eq!(
            context.path.rtt_estimator.latest_rtt(),
            Duration::from_millis(3000)
        );
        assert_eq!(3, context.on_rtt_update_count);

        // Send and ack a non ack eliciting packet
        manager.on_packet_sent(
            space.new_packet_number(VarInt::from_u8(11)),
            transmission::Outcome {
                ack_elicitation: AckElicitation::NonEliciting,
                is_congestion_controlled: true,
                bytes_sent: packet_bytes,
                packet_number: space.new_packet_number(VarInt::from_u8(1)),
            },
            time_sent,
            path::Id::new(0),
            &mut context,
        );
        ack_packets(11..=11, ack_receive_time, &mut context, &mut manager);

        assert_eq!(context.path.congestion_controller.lost_bytes, 0);
        assert_eq!(context.path.congestion_controller.on_rtt_update, 1);
        assert_eq!(context.on_packet_ack_count, 5);
        assert_eq!(context.on_new_packet_ack_count, 4);
        assert_eq!(context.validate_packet_ack_count, 5);
        assert_eq!(context.on_packet_loss_count, 3);
        // RTT remains unchanged
        assert_eq!(
            context.path.rtt_estimator.latest_rtt(),
            Duration::from_millis(3000)
        );
        assert_eq!(3, context.on_rtt_update_count);
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#A.10
    //= type=test
    #[test]
    fn detect_and_remove_lost_packets() {
        let space = PacketNumberSpace::ApplicationData;
        let mut manager = Manager::new(space, Duration::from_millis(100));
        let now = s2n_quic_platform::time::now();
        let mut context = MockContext::default();
        manager.largest_acked_packet = Some(space.new_packet_number(VarInt::from_u8(10)));

        let mut time_sent = s2n_quic_platform::time::now();
        let outcome = transmission::Outcome {
            ack_elicitation: AckElicitation::Eliciting,
            is_congestion_controlled: true,
            bytes_sent: 1,
            packet_number: space.new_packet_number(VarInt::from_u8(1)),
        };

        // Send a packet that was sent too long ago (lost)
        let old_packet_time_sent = space.new_packet_number(VarInt::from_u8(0));
        manager.on_packet_sent(
            old_packet_time_sent,
            outcome,
            time_sent,
            path::Id::new(0),
            &mut context,
        );

        // time threshold = max(kTimeThreshold * max(smoothed_rtt, latest_rtt), kGranularity)
        // time threshold = max(9/8 * 8) = 9
        context.path.rtt_estimator.update_rtt(
            Duration::from_secs(0),
            Duration::from_secs(8),
            now,
            true,
            space,
        );
        let expected_time_threshold = Duration::from_secs(9);
        assert_eq!(
            expected_time_threshold,
            Manager::calculate_loss_time_threshold(&context.path.rtt_estimator)
        );

        time_sent += Duration::from_secs(10);

        // Send a packet that was sent within the time threshold but is with a packet number
        // K_PACKET_THRESHOLD away from the largest (lost)
        let old_packet_packet_number =
            space.new_packet_number(VarInt::new(10 - K_PACKET_THRESHOLD).unwrap());
        manager.on_packet_sent(
            old_packet_packet_number,
            outcome,
            time_sent,
            path::Id::new(0),
            &mut context,
        );

        // Send a packet that is less than the largest acked but not lost
        let not_lost = space.new_packet_number(VarInt::from_u8(9));
        manager.on_packet_sent(not_lost, outcome, time_sent, path::Id::new(0), &mut context);

        // Send a packet larger than the largest acked (not lost)
        let larger_than_largest = manager.largest_acked_packet.unwrap().next().unwrap();
        manager.on_packet_sent(
            larger_than_largest,
            outcome,
            time_sent,
            path::Id::new(0),
            &mut context,
        );

        // Four packets sent, each size 1 byte
        let bytes_in_flight: u16 = manager
            .sent_packets
            .iter()
            .map(|(_, info)| info.sent_bytes)
            .sum();
        assert_eq!(bytes_in_flight, 4);

        let now = time_sent;

        manager.detect_and_remove_lost_packets(now, &mut context);

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.1.2
        //= type=test
        //# Once a later packet within the same packet number space has been
        //# acknowledged, an endpoint SHOULD declare an earlier packet lost if it
        //# was sent a threshold amount of time in the past.

        // Two packets lost, each size 1 byte
        assert_eq!(context.path.congestion_controller.lost_bytes, 2);
        // Two packets remaining
        let bytes_in_flight: u16 = manager
            .sent_packets
            .iter()
            .map(|(_, info)| info.sent_bytes)
            .sum();
        assert_eq!(bytes_in_flight, 2);

        let sent_packets = &manager.sent_packets;
        assert!(context.lost_packets.contains(&old_packet_time_sent));
        assert!(sent_packets.get(old_packet_time_sent).is_none());

        assert!(context.lost_packets.contains(&old_packet_packet_number));
        assert!(sent_packets.get(old_packet_packet_number).is_none());

        assert!(!context.lost_packets.contains(&larger_than_largest));
        assert!(sent_packets.get(larger_than_largest).is_some());

        assert!(!context.lost_packets.contains(&not_lost));
        assert!(sent_packets.get(not_lost).is_some());

        let expected_loss_time =
            sent_packets.get(not_lost).unwrap().time_sent + expected_time_threshold;
        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.1.2
        //= type=test
        //# If packets sent prior to the largest acknowledged packet cannot yet
        //# be declared lost, then a timer SHOULD be set for the remaining time.
        assert!(manager.loss_timer.is_armed());
        assert_eq!(Some(expected_loss_time), manager.loss_timer.iter().next());
    }

    #[test]
    fn detect_and_remove_lost_packets_nothing_lost() {
        let space = PacketNumberSpace::ApplicationData;
        let mut manager = Manager::new(space, Duration::from_millis(100));
        let mut context = MockContext::default();
        manager.largest_acked_packet = Some(space.new_packet_number(VarInt::from_u8(10)));

        let time_sent = s2n_quic_platform::time::now();
        let outcome = transmission::Outcome {
            ack_elicitation: AckElicitation::Eliciting,
            is_congestion_controlled: true,
            bytes_sent: 1,
            packet_number: space.new_packet_number(VarInt::from_u8(1)),
        };

        // Send a packet that is less than the largest acked but not lost
        let not_lost = space.new_packet_number(VarInt::from_u8(9));
        manager.on_packet_sent(not_lost, outcome, time_sent, path::Id::new(0), &mut context);

        manager.detect_and_remove_lost_packets(time_sent, &mut context);

        // Verify no lost bytes are sent to the congestion controller and
        // on_packets_lost is not called
        assert_eq!(context.lost_packets.len(), 0);
        assert_eq!(context.path.congestion_controller.lost_bytes, 0);
        assert_eq!(context.path.congestion_controller.on_packets_lost, 0);
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.2.1
    //= type=test
    #[test]
    fn update_pto_timer() {
        let space = PacketNumberSpace::ApplicationData;
        let mut manager = Manager::new(space, Duration::from_millis(10));
        let now = s2n_quic_platform::time::now() + Duration::from_secs(10);
        let is_handshake_confirmed = true;
        let mut context = MockContext::new(Duration::from_millis(10), false);

        context.path.rtt_estimator.update_rtt(
            Duration::from_millis(0),
            Duration::from_millis(500),
            now,
            true,
            space,
        );
        context.path.rtt_estimator.update_rtt(
            Duration::from_millis(0),
            Duration::from_millis(1000),
            now,
            true,
            space,
        );
        // The path will be at the anti-amplification limit
        context.path.on_bytes_received(1200);
        context.path.on_bytes_transmitted((1200 * 3) + 1);
        // Arm the PTO so we can verify it is cancelled
        manager.pto.timer.set(now + Duration::from_secs(10));
        manager.update_pto_timer(&context.path, now, is_handshake_confirmed);

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.2.2.1
        //= type=test
        //# If no additional data can be sent, the server's PTO timer MUST NOT be
        //# armed until datagrams have been received from the client, because
        //# packets sent on PTO count against the anti-amplification limit.
        assert!(!manager.pto.timer.is_armed());

        // Arm the PTO so we can verify it is cancelled
        manager.pto.timer.set(now + Duration::from_secs(10));
        // Validate the path so it is not at the anti-amplification limit
        context.path.on_validated();
        context.path.on_peer_validated();
        manager.update_pto_timer(&context.path, now, is_handshake_confirmed);

        // Since the path is peer validated and sent packets is empty, PTO is cancelled
        assert!(!manager.pto.timer.is_armed());

        // Reset the path back to not peer validated
        context.path = Path::new(
            Default::default(),
            connection::PeerId::TEST_ID,
            RttEstimator::new(manager.max_ack_delay),
            MockCongestionController::default(),
            false,
        );
        context.path.on_validated();
        context.path.pto_backoff = 2;
        let is_handshake_confirmed = false;
        manager.update_pto_timer(&context.path, now, is_handshake_confirmed);

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.2.1
        //= type=test
        //# An endpoint MUST NOT set its PTO timer for the application data
        //# packet number space until the handshake is confirmed.
        assert!(!manager.pto.timer.is_armed());

        // Set is handshake confirmed back to true
        let is_handshake_confirmed = true;
        manager.update_pto_timer(&context.path, now, is_handshake_confirmed);

        // Now the PTO is armed
        assert!(manager.pto.timer.is_armed());

        // Send a packet to validate behavior when sent_packets is not empty
        manager.on_packet_sent(
            space.new_packet_number(VarInt::from_u8(1)),
            transmission::Outcome {
                ack_elicitation: AckElicitation::Eliciting,
                is_congestion_controlled: true,
                bytes_sent: 1,
                packet_number: space.new_packet_number(VarInt::from_u8(1)),
            },
            now,
            path::Id::new(0),
            &mut context,
        );

        let expected_pto_base_timestamp = now - Duration::from_secs(5);
        manager.time_of_last_ack_eliciting_packet = Some(expected_pto_base_timestamp);
        // This will update the smoother_rtt to 2000, and rtt_var to 1000
        context.path.rtt_estimator.update_rtt(
            Duration::from_millis(0),
            Duration::from_millis(2000),
            now,
            true,
            space,
        );
        manager.update_pto_timer(&context.path, now, is_handshake_confirmed);

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.2.1
        //# When an ack-eliciting packet is transmitted, the sender schedules a
        //# timer for the PTO period as follows:
        //#
        //# PTO = smoothed_rtt + max(4*rttvar, kGranularity) + max_ack_delay
        // Including the pto backoff (2) =:
        // PTO = (2000 + max(4*1000, 1) + 10) * 2 = 12020
        assert!(manager.pto.timer.is_armed());
        assert_eq!(
            manager.pto.timer.iter().next().unwrap(),
            expected_pto_base_timestamp + Duration::from_millis(12020)
        );
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.2.1
    //= type=test
    #[test]
    fn on_timeout() {
        let space = PacketNumberSpace::ApplicationData;
        let mut manager = Manager::new(space, Duration::from_millis(10));
        let now = s2n_quic_platform::time::now() + Duration::from_secs(10);
        manager.largest_acked_packet = Some(space.new_packet_number(VarInt::from_u8(10)));
        let mut context = MockContext::new(Duration::from_millis(10), false);

        let mut expected_pto_backoff = context.path.pto_backoff;

        // Loss timer is armed but not expired yet, nothing happens
        manager.loss_timer.set(now + Duration::from_secs(10));
        manager.on_timeout(now, &mut context);
        assert_eq!(context.on_packet_loss_count, 0);
        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.2.1
        //= type=test
        //# The PTO timer MUST NOT be set if a timer is set for time threshold
        //# loss detection; see Section 6.1.2.
        assert!(!manager.pto.timer.is_armed());
        assert_eq!(expected_pto_backoff, context.path.pto_backoff);

        // Send a packet that will be considered lost
        manager.on_packet_sent(
            space.new_packet_number(VarInt::from_u8(1)),
            transmission::Outcome {
                ack_elicitation: AckElicitation::Eliciting,
                is_congestion_controlled: true,
                bytes_sent: 1,
                packet_number: space.new_packet_number(VarInt::from_u8(1)),
            },
            now - Duration::from_secs(5),
            path::Id::new(0),
            &mut context,
        );

        // Loss timer is armed and expired, on_packet_loss is called
        manager.loss_timer.set(now - Duration::from_secs(1));
        manager.on_timeout(now, &mut context);
        assert_eq!(context.on_packet_loss_count, 1);
        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.2.1
        //= type=test
        //# The PTO timer MUST NOT be set if a timer is set for time threshold
        //# loss detection; see Section 6.1.2.
        assert!(!manager.pto.timer.is_armed());
        assert_eq!(expected_pto_backoff, context.path.pto_backoff);

        // Loss timer is not armed, pto timer is not armed
        manager.loss_timer.cancel();
        manager.on_timeout(now, &mut context);
        assert_eq!(expected_pto_backoff, context.path.pto_backoff);

        // Loss timer is not armed, pto timer is armed but not expired
        manager.loss_timer.cancel();
        manager.pto.timer.set(now + Duration::from_secs(5));
        manager.on_timeout(now, &mut context);
        assert_eq!(expected_pto_backoff, context.path.pto_backoff);

        // Loss timer is not armed, pto timer is expired without bytes in flight
        expected_pto_backoff *= 2;
        manager.pto.timer.set(now - Duration::from_secs(5));
        manager.on_timeout(now, &mut context);
        assert_eq!(expected_pto_backoff, context.path.pto_backoff);
        assert_eq!(manager.pto.state, RequiresTransmission(1));

        // Loss timer is not armed, pto timer is expired with bytes in flight

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.2.1
        //= type=test
        //# When a PTO timer expires, the PTO backoff MUST be increased,
        //# resulting in the PTO period being set to twice its current value.
        expected_pto_backoff *= 2;
        manager.sent_packets.insert(
            space.new_packet_number(VarInt::from_u8(1)),
            SentPacketInfo {
                congestion_controlled: true,
                sent_bytes: 1,
                time_sent: now,
                ack_elicitation: AckElicitation::Eliciting,
                path_id: path::Id::new(0),
            },
        );
        manager.pto.timer.set(now - Duration::from_secs(5));
        manager.on_timeout(now, &mut context);
        assert_eq!(expected_pto_backoff, context.path.pto_backoff);

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.2.4
        //= type=test
        //# When a PTO timer expires, a sender MUST send at least one ack-
        //# eliciting packet in the packet number space as a probe.

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.2.4
        //= type=test
        //# An endpoint
        //# MAY send up to two full-sized datagrams containing ack-eliciting
        //# packets, to avoid an expensive consecutive PTO expiration due to a
        //# single lost datagram or transmit data from multiple packet number
        //# spaces.
        assert_eq!(manager.pto.state, RequiresTransmission(2));

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.2
        //= type=test
        //# A PTO timer expiration event does not indicate packet loss and MUST
        //# NOT cause prior unacknowledged packets to be marked as lost.
        assert!(manager
            .sent_packets
            .get(space.new_packet_number(VarInt::from_u8(1)))
            .is_some());
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
        assert_eq!(manager.timers().next(), Some(loss_time));

        // Both timers are armed, only loss time is returned
        manager.loss_timer.set(loss_time);
        manager.pto.timer.set(pto_time);
        assert_eq!(manager.timers().count(), 1);
        assert_eq!(manager.timers().next(), Some(loss_time));
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.1.1
    //= type=test
    //# The RECOMMENDED initial value for the packet reordering threshold
    //# (kPacketThreshold) is 3, based on best practices for TCP loss
    //# detection ([RFC5681], [RFC6675]).

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.1.1
    //= type=test
    //# In order to remain similar to TCP,
    //# implementations SHOULD NOT use a packet threshold less than 3; see
    //# [RFC5681].
    #[allow(clippy::assertions_on_constants)]
    #[test]
    fn packet_reorder_threshold_at_least_three() {
        assert!(K_PACKET_THRESHOLD >= 3);
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.1.2
    //= type=test
    //# The RECOMMENDED time threshold (kTimeThreshold), expressed as a
    //# round-trip time multiplier, is 9/8.
    #[test]
    fn time_threshold_multiplier_equals_nine_eighths() {
        let mut rtt_estimator = RttEstimator::new(Duration::from_millis(10));
        rtt_estimator.update_rtt(
            Duration::from_millis(10),
            Duration::from_secs(1),
            s2n_quic_platform::time::now(),
            true,
            PacketNumberSpace::Initial,
        );
        assert_eq!(
            Duration::from_millis(1125), // 9/8 seconds = 1.125 seconds
            Manager::calculate_loss_time_threshold(&rtt_estimator)
        );
    }

    #[test]
    fn requires_probe() {
        let space = PacketNumberSpace::ApplicationData;
        let mut manager = Manager::new(space, Duration::from_millis(10));

        manager.pto.state = PtoState::RequiresTransmission(2);
        assert!(manager.requires_probe());

        manager.pto.state = PtoState::Idle;
        assert!(!manager.requires_probe());
    }

    #[test]
    fn timer_granularity() {
        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.1.2
        //= type=test
        //# The RECOMMENDED value of the
        //# timer granularity (kGranularity) is 1ms.
        assert_eq!(Duration::from_millis(1), K_GRANULARITY);

        let mut rtt_estimator = RttEstimator::new(Duration::from_millis(0));
        rtt_estimator.update_rtt(
            Duration::from_millis(0),
            Duration::from_nanos(1),
            s2n_quic_platform::time::now(),
            true,
            PacketNumberSpace::Initial,
        );

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.1.2
        //= type=test
        //# To avoid declaring
        //# packets as lost too early, this time threshold MUST be set to at
        //# least the local timer granularity, as indicated by the kGranularity
        //# constant.
        assert!(Manager::calculate_loss_time_threshold(&rtt_estimator) >= K_GRANULARITY);
    }

    // Helper function that will call on_ack_frame with the given packet numbers
    fn ack_packets<CC: CongestionController, Ctx: Context<CC>>(
        range: RangeInclusive<u8>,
        ack_receive_time: Timestamp,
        context: &mut Ctx,
        manager: &mut Manager,
    ) {
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
            destination_connection_id: connection::LocalId::TEST_ID,
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

        let _ = manager.on_ack_frame(&datagram, frame, context);

        for packet in acked_packets {
            assert!(manager.sent_packets.get(packet).is_none());
        }
    }
}

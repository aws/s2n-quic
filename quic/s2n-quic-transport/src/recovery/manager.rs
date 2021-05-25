// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    contexts::WriteContext,
    path::{self, Path},
    recovery::{SentPacketInfo, SentPackets},
    timer::VirtualTimer,
    transmission,
};
use core::{cmp::max, time::Duration};
use s2n_quic_core::{
    endpoint, frame,
    inet::{DatagramInfo, SocketAddress},
    packet::number::{PacketNumber, PacketNumberRange, PacketNumberSpace},
    recovery::{CongestionController, RttEstimator, K_GRANULARITY},
    time::Timestamp,
    transport,
    varint::VarInt,
};
use smallvec::SmallVec;

type PacketDetails = (PacketNumber, SentPacketInfo, path::Id);

#[derive(Debug)]
pub struct Manager {
    // The packet space for this recovery manager
    space: PacketNumberSpace,

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
    largest_acked_packet: Option<PacketNumber>,

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#A.3
    //# An association of packet numbers in a packet number space to information about them.
    //  These are packets that are pending acknowledgement.
    sent_packets: SentPackets,

    // Timer set when packets may be declared lost at a time in the future
    loss_timer: VirtualTimer,

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.2
    //# A Probe Timeout (PTO) triggers sending one or two probe datagrams
    //# when ack-eliciting packets are not acknowledged within the expected
    //# period of time or the server may not have validated the client's
    //# address.  A PTO enables a connection to recover from loss of tail
    //# packets or acknowledgements.
    pto: Pto,

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
        path_id: path::Id,
        context: &mut Ctx,
    ) {
        if self.loss_timer.is_armed() {
            if self.loss_timer.poll_expiration(timestamp).is_ready() {
                self.detect_and_remove_lost_packets(timestamp, path_id, context)
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
            let path = context.path_mut_by_id(path_id);
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
            // TODO does the PTO need to be tracked by path?
            self.pto
                .update(pto_base_timestamp, path.pto_period(self.space));
        }
    }

    /// Queries the component for any outgoing frames that need to get sent
    pub fn on_transmit<W: WriteContext>(&mut self, context: &mut W) {
        self.pto.on_transmit(context)
    }

    /// Process ACK frame. Update congestion controler, timers and meta data around acked
    /// packet ranges.
    pub fn on_ack_frame<A: frame::ack::AckRanges, CC: CongestionController, Ctx: Context<CC>>(
        &mut self,
        datagram: &DatagramInfo,
        frame: frame::Ack<A>,
        context: &mut Ctx,
    ) -> Result<(), transport::Error> {
        let largest_acked_packet_number =
            self.space.new_packet_number(frame.largest_acknowledged());
        let mut newly_acked_packets =
            SmallVec::<[SentPacketInfo; ACKED_PACKETS_INITIAL_CAPACITY]>::new();

        // Update the largest acked packet if the largest packet acked in this frame is larger
        self.largest_acked_packet = Some(
            self.largest_acked_packet
                .map_or(largest_acked_packet_number, |pn| {
                    pn.max(largest_acked_packet_number)
                }),
        );

        let (largest_newly_acked, includes_ack_eliciting) =
            self.process_ack_range(&mut newly_acked_packets, datagram, &frame, context)?;

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#5.1
        //# An endpoint generates an RTT sample on receiving an ACK frame that
        //# meets the following two conditions:
        //#
        //# *  the largest acknowledged packet number is newly acknowledged, and
        //#
        //# *  at least one of the newly acknowledged packets was ack-eliciting.
        if let Some(largest_newly_acked) = largest_newly_acked {
            self.update_congestion_control(
                largest_newly_acked,
                largest_acked_packet_number,
                includes_ack_eliciting,
                datagram,
                &frame,
                context,
            );

            let (_, largest_newly_acked_info, _) = largest_newly_acked;
            self.process_new_acked_packets(
                &newly_acked_packets,
                largest_newly_acked_info.time_sent,
                datagram,
                context,
            );
        }

        Ok(())
    }

    // Process ack_range and return largest_newly_acked and if the packet is ack eliciting.
    fn process_ack_range<A: frame::ack::AckRanges, CC: CongestionController, Ctx: Context<CC>>(
        &mut self,
        newly_acked_packets: &mut SmallVec<[SentPacketInfo; ACKED_PACKETS_INITIAL_CAPACITY]>,
        datagram: &DatagramInfo,
        frame: &frame::Ack<A>,
        context: &mut Ctx,
    ) -> Result<(Option<PacketDetails>, bool), transport::Error> {
        let mut largest_newly_acked: Option<(PacketNumber, SentPacketInfo, path::Id)> = None;
        let mut includes_ack_eliciting = false;

        for ack_range in frame.ack_ranges() {
            let (start, end) = ack_range.into_inner();

            let acked_packets = PacketNumberRange::new(
                self.space.new_packet_number(start),
                self.space.new_packet_number(end),
            );

            context.validate_packet_ack(datagram, &acked_packets)?;
            // notify components of packets acked
            context.on_packet_ack(datagram, &acked_packets);

            let mut new_packet_ack = false;

            for packet_number in acked_packets {
                if let Some(acked_packet_info) = self.sent_packets.remove(packet_number) {
                    newly_acked_packets.push(acked_packet_info);

                    if largest_newly_acked.map_or(true, |(pn, _, _)| packet_number > pn) {
                        largest_newly_acked =
                            Some((packet_number, acked_packet_info, acked_packet_info.path_id));
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

        Ok((largest_newly_acked, includes_ack_eliciting))
    }

    fn update_congestion_control<
        A: frame::ack::AckRanges,
        CC: CongestionController,
        Ctx: Context<CC>,
    >(
        &mut self,
        largest_newly_acked: PacketDetails,
        largest_acked_packet_number: PacketNumber,
        includes_ack_eliciting: bool,
        datagram: &DatagramInfo,
        frame: &frame::Ack<A>,
        context: &mut Ctx,
    ) {
        let mut should_update_rtt = true;
        let is_handshake_confirmed = context.is_handshake_confirmed();
        let (largest_newly_acked_packet_number, largest_newly_acked_info, largest_acked_path_id) =
            largest_newly_acked;

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#9.4
        //# Packets sent on the old path MUST NOT contribute to
        //# congestion control or RTT estimation for the new path.
        should_update_rtt &= context
            .path_by_id(largest_acked_path_id)
            .peer_socket_address
            == datagram.remote_address;

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#5.1
        //# To avoid generating multiple RTT samples for a single packet, an ACK
        //# frame SHOULD NOT be used to update RTT estimates if it does not newly
        //# acknowledge the largest acknowledged packet.
        should_update_rtt &= largest_newly_acked_packet_number == largest_acked_packet_number;

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#5.1
        //# An RTT sample MUST NOT be generated on receiving an ACK frame that
        //# does not newly acknowledge at least one ack-eliciting packet.
        should_update_rtt &= includes_ack_eliciting;

        if should_update_rtt {
            let latest_rtt = datagram.timestamp - largest_newly_acked_info.time_sent;
            let path = context.path_mut_by_id(largest_acked_path_id);
            path.rtt_estimator.update_rtt(
                frame.ack_delay(),
                latest_rtt,
                datagram.timestamp,
                is_handshake_confirmed,
                largest_acked_packet_number.space(),
            );

            // Update the congestion controller with the latest RTT estimate
            path.congestion_controller
                .on_rtt_update(largest_newly_acked_info.time_sent, &path.rtt_estimator);

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
                    .path_mut_by_id(largest_acked_path_id)
                    .congestion_controller
                    .on_congestion_event(datagram.timestamp);
            }
        }
    }

    fn process_new_acked_packets<CC: CongestionController, Ctx: Context<CC>>(
        &mut self,
        newly_acked_packets: &SmallVec<[SentPacketInfo; ACKED_PACKETS_INITIAL_CAPACITY]>,
        largest_newly_acked_time_sent: Timestamp,
        datagram: &DatagramInfo,
        context: &mut Ctx,
    ) {
        // persistent_congestion is only updated for the path on which we receive the ack.
        // Managing state for multiple paths requires extra allocations but is only necessary
        // when also attempting connection_migration; which should not be very common.
        let acked_path_id = context
            .path_id_by_peer_addr(&datagram.remote_address)
            .expect("a path should exist for this acked packet");

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.1.2
        //# Once a later packet within the same packet number space has been
        //# acknowledged, an endpoint SHOULD declare an earlier packet lost if it
        //# was sent a threshold amount of time in the past.
        self.detect_and_remove_lost_packets(datagram.timestamp, acked_path_id, context);

        let is_handshake_confirmed = context.is_handshake_confirmed();
        for acked_packet_info in newly_acked_packets {
            let path = context.path_mut_by_id(acked_packet_info.path_id);
            path.congestion_controller.on_packet_ack(
                largest_newly_acked_time_sent,
                acked_packet_info.sent_bytes as usize,
                &path.rtt_estimator,
                datagram.timestamp,
            );

            //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.2.1
            //# The PTO backoff factor is reset when an acknowledgement is received,
            //# except in the following case.  A server might take longer to respond
            //# to packets during the handshake than otherwise.  To protect such a
            //# server from repeated client probes, the PTO backoff is not reset at a
            //# client that is not yet certain that the server has finished
            //# validating the client's address.  That is, a client does not reset
            //# the PTO backoff factor on receiving acknowledgements in Initial
            //# packets.
            if path.is_peer_validated() {
                path.reset_pto_backoff();
            }

            //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.2.1
            //# A sender SHOULD restart its PTO timer every time an ack-eliciting
            //# packet is sent or acknowledged,
            self.update_pto_timer(path, datagram.timestamp, is_handshake_confirmed);
        }
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
        path_id: path::Id,
        context: &mut Ctx,
    ) {
        // Cancel the loss timer. It will be armed again if any unacknowledged packets are
        // older than the largest acked packet, but not old enough to be considered lost yet
        self.loss_timer.cancel();

        // TODO: Investigate a more efficient mechanism for managing sent_packets
        //       See https://github.com/awslabs/s2n-quic/issues/69
        let (max_persistent_congestion_period, sent_packets_to_remove) =
            self.detect_lost_packets(now, path_id, context);

        self.remove_lost_packets(
            now,
            max_persistent_congestion_period,
            sent_packets_to_remove,
            path_id,
            context,
        );
    }

    fn detect_lost_packets<CC: CongestionController, Ctx: Context<CC>>(
        &mut self,
        now: Timestamp,
        path_id: path::Id,
        context: &mut Ctx,
    ) -> (Duration, Vec<PacketDetails>) {
        let largest_acked_packet = &self
            .largest_acked_packet
            .expect("This function is only called after an ack has been received");

        let mut sent_packets_to_remove = Vec::new();

        let mut max_persistent_congestion_period = Duration::from_secs(0);
        let mut persistent_congestion_period = Duration::from_secs(0);
        let mut prev_packet: Option<(&PacketNumber, Timestamp)> = None;

        for (unacked_packet_number, unacked_sent_info) in self.sent_packets.iter() {
            if unacked_packet_number > largest_acked_packet {
                // sent_packets is ordered by packet number, so all remaining packets will be larger
                break;
            }

            let unacked_path_id = unacked_sent_info.path_id;
            let path = &context.path_by_id(unacked_path_id);
            // Calculate how long we wait until a packet is declared lost
            let time_threshold = Self::calculate_loss_time_threshold(&path.rtt_estimator);
            // Packets sent before this time are deemed lost.
            let lost_send_time = now.checked_sub(time_threshold);

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
                sent_packets_to_remove.push((
                    *unacked_packet_number,
                    *unacked_sent_info,
                    unacked_path_id,
                ));

                if unacked_sent_info.congestion_controlled {
                    // The packet is "in-flight", ie congestion controlled
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
                if is_contiguous && unacked_path_id == path_id {
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
                .path_mut_by_id(unacked_path_id)
                .rtt_estimator
                .first_rtt_sample()
                .map_or(false, |ts| unacked_sent_info.time_sent > ts)
            {
                prev_packet = Some((unacked_packet_number, unacked_sent_info.time_sent));
            }
        }

        (max_persistent_congestion_period, sent_packets_to_remove)
    }

    fn remove_lost_packets<CC: CongestionController, Ctx: Context<CC>>(
        &mut self,
        now: Timestamp,
        max_persistent_congestion_period: Duration,
        sent_packets_to_remove: Vec<PacketDetails>,
        path_id: path::Id,
        context: &mut Ctx,
    ) {
        // Remove the lost packets and account for the bytes on the proper congestion controller
        for (packet_number, sent_info, remove_path_id) in sent_packets_to_remove {
            // TODO add test to verify multi path behavior
            // congestion_controller.on_packets_lost
            // rtt_estimator.on_persistent_congestion
            let path = context.path_mut_by_id(remove_path_id);

            self.sent_packets.remove(packet_number);

            //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#7.6.2
            //# A sender that does not have state for all packet
            //# number spaces or an implementation that cannot compare send times
            //# across packet number spaces MAY use state for just the packet number
            //# space that was acknowledged.
            let persistent_congestion = max_persistent_congestion_period
                > path.rtt_estimator.persistent_congestion_threshold();

            if sent_info.sent_bytes > 0 {
                path.congestion_controller.on_packets_lost(
                    sent_info.sent_bytes as u32,
                    persistent_congestion,
                    now,
                );
            }

            if persistent_congestion && remove_path_id == path_id {
                //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#5.2
                //# Endpoints SHOULD set the min_rtt to the newest RTT sample after
                //# persistent congestion is established.
                path.rtt_estimator.on_persistent_congestion();
            }
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

pub trait Context<CC: CongestionController> {
    const ENDPOINT_TYPE: endpoint::Type;

    fn is_handshake_confirmed(&self) -> bool;

    fn path(&self) -> &Path<CC>;

    fn path_mut(&mut self) -> &mut Path<CC>;

    fn path_by_id(&self, path_id: path::Id) -> &path::Path<CC>;

    fn path_mut_by_id(&mut self, path_id: path::Id) -> &mut path::Path<CC>;

    fn path_id_by_peer_addr(&self, addr: &SocketAddress) -> Option<path::Id>;

    fn validate_packet_ack(
        &mut self,
        datagram: &DatagramInfo,
        packet_number_range: &PacketNumberRange,
    ) -> Result<(), transport::Error>;

    fn on_new_packet_ack(
        &mut self,
        datagram: &DatagramInfo,
        packet_number_range: &PacketNumberRange,
    );
    fn on_packet_ack(&mut self, datagram: &DatagramInfo, packet_number_range: &PacketNumberRange);
    fn on_packet_loss(&mut self, packet_number_range: &PacketNumberRange);
    fn on_rtt_update(&mut self);
}

impl transmission::interest::Provider for Manager {
    fn transmission_interest(&self) -> transmission::Interest {
        self.pto.transmission_interest()
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
    pub fn timers(&self) -> impl Iterator<Item = Timestamp> {
        self.timer.iter()
    }

    /// Called when a timeout has occurred. Returns true if the PTO timer had expired.
    pub fn on_timeout(&mut self, packets_in_flight: bool, timestamp: Timestamp) -> bool {
        if self.timer.poll_expiration(timestamp).is_ready() {
            //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.2.4
            //# When a PTO timer expires, a sender MUST send at least one ack-
            //# eliciting packet in the packet number space as a probe.

            //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.2.2.1
            //# Since the server could be blocked until more datagrams are received
            //# from the client, it is the client's responsibility to send packets to
            //# unblock the server until it is certain that the server has finished
            //# its address validation

            //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.2.4
            //# An endpoint MAY send up to two full-sized datagrams containing
            //# ack-eliciting packets, to avoid an expensive consecutive PTO
            //# expiration due to a single lost datagram or transmit data from
            //# multiple packet number spaces.

            //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.2.4
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
                //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.2.4
                //# When there is no data to send, the sender SHOULD send
                //# a PING or other ack-eliciting frame in a single packet, re-arming the
                //# PTO timer.

                //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.2.2.1
                //# When the PTO fires, the client MUST send a Handshake packet if it
                //# has Handshake keys, otherwise it MUST send an Initial packet in a
                //# UDP datagram with a payload of at least 1200 bytes.

                //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#A.9
                //# // Client sends an anti-deadlock packet: Initial is padded
                //# // to earn more anti-amplification credit,
                //# // a Handshake packet proves address ownership.

                //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.2.4
                //# All probe packets sent on a PTO MUST be ack-eliciting.

                //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#7.5
                //# Probe packets MUST NOT be blocked by the congestion controller.

                // The early transmission will automatically ensure all initial packets sent by the
                // client are padded to 1200 bytes
                if context.ack_elicitation().is_ack_eliciting()
                    || context.write_frame_forced(&frame::Ping).is_some()
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

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.2.1
    //# A sender SHOULD restart its PTO timer every time an ack-eliciting
    //# packet is sent or acknowledged, when the handshake is confirmed
    //# (Section 4.1.2 of [QUIC-TLS]), or when Initial or Handshake keys are
    //# discarded (Section 4.9 of [QUIC-TLS]).
    pub fn update(&mut self, base_timestamp: Timestamp, pto_period: Duration) {
        self.timer.set(base_timestamp + pto_period);
    }

    /// Cancels the PTO timer
    pub fn cancel(&mut self) {
        self.timer.cancel();
    }
}

impl transmission::interest::Provider for Pto {
    fn transmission_interest(&self) -> transmission::Interest {
        if matches!(self.state, PtoState::RequiresTransmission(_)) {
            transmission::Interest::Forced
        } else {
            transmission::Interest::None
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{
        connection::{ConnectionIdMapper, InternalConnectionIdGenerator},
        contexts::testing::{MockWriteContext, OutgoingFrameBuffer},
        recovery,
        recovery::manager::PtoState::RequiresTransmission,
        space::rx_packet_numbers::ack_ranges::AckRanges,
    };
    use core::{ops::RangeInclusive, time::Duration};
    use s2n_quic_core::{
        connection, endpoint,
        frame::ack_elicitation::AckElicitation,
        inet::{DatagramInfo, ExplicitCongestionNotification, SocketAddress},
        packet::number::PacketNumberSpace,
        path::INITIAL_PTO_BACKOFF,
        random,
        recovery::{
            congestion_controller::testing::{
                mock::{CongestionController as MockCongestionController, Endpoint},
                unlimited::CongestionController as Unlimited,
            },
            DEFAULT_INITIAL_RTT,
        },
        time::{Clock, NoopClock},
        varint::VarInt,
    };
    use std::{collections::HashSet, net::SocketAddr};

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.2.2
    //= type=test
    //# When no previous RTT is available, the initial RTT
    //# SHOULD be set to 333ms, resulting in a 1 second initial timeout, as
    //# recommended in [RFC6298].
    #[test]
    fn one_second_pto_when_no_previous_rtt_available() {
        let space = PacketNumberSpace::Handshake;
        let max_ack_delay = Duration::from_millis(0);
        let mut manager = Manager::new(space, max_ack_delay);
        let now = s2n_quic_platform::time::now();

        let path = Path::new(
            Default::default(),
            connection::PeerId::TEST_ID,
            connection::LocalId::TEST_ID,
            RttEstimator::new(max_ack_delay),
            Unlimited::default(),
            false,
        );

        manager
            .pto
            .update(now, path.rtt_estimator.pto_period(path.pto_backoff, space));

        assert!(manager.pto.timer.is_armed());
        assert_eq!(
            manager.pto.timer.iter().next(),
            Some(now + Duration::from_millis(999))
        );
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#A.5
    //= type=test
    #[test]
    fn on_packet_sent() {
        let max_ack_delay = Duration::from_millis(100);
        let now = s2n_quic_platform::time::now();
        let mut time_sent = now;
        let space = PacketNumberSpace::ApplicationData;
        let (
            _first_addr,
            first_path_id,
            _second_addr,
            _second_path_id,
            mut manager,
            mut path_manager,
        ) = helper_generate_multi_path_manager(space);
        let mut context = MockContext::new(&mut path_manager);

        // Call on validated so the path is not amplification limited so we can verify PTO arming
        context.path_mut().on_validated();

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

            manager.on_packet_sent(sent_packet, outcome, time_sent, first_path_id, &mut context);

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
            context
                .path_by_id(first_path_id)
                .congestion_controller
                .bytes_in_flight
        );
    }

    #[test]
    // The pto timer is shared for all paths and should be armed if packet if received on any of
    // the paths.
    //
    // Setup 1:
    // - create path manager with two validated and not AmplificationLimited paths
    // - reset pto timer
    //
    // Trigger 1:
    // - send packet on path 1
    //   - packet: 1
    //   - ack_elicitation: Eliciting
    //   - is_congestion_controlled: true
    //
    // Expectation 1:
    // - bytes sent, congestion_controlled, time_sent match that of sent
    // - pto is armed
    //
    // Setup 2:
    // - reset pto timer
    //
    // Trigger 2:
    // - send packet on path 2
    //   - packet: 2
    //   - ack_elicitation: Eliciting
    //   - is_congestion_controlled: true
    //
    // Expectation 2:
    // - bytes sent, congestion_controlled, time_sent match that of sent
    // - pto is armed
    fn on_packet_sent_across_multiple_paths() {
        // let space = PacketNumberSpace::ApplicationData;
        let max_ack_delay = Duration::from_millis(100);
        // let mut manager = Manager::new(space, max_ack_delay);
        let now = s2n_quic_platform::time::now();
        let mut time_sent = now;
        // let mut path_manager = helper_generate_path_manager(Duration::from_millis(100));
        // let mut context = MockContext::new(&mut path_manager);
        // Call on validated so the path is not amplification limited so we can verify PTO arming
        let space = PacketNumberSpace::ApplicationData;
        let packet_bytes = 128;
        // Setup 1:
        let (
            _first_addr,
            first_path_id,
            _second_addr,
            second_path_id,
            mut manager,
            mut path_manager,
        ) = helper_generate_multi_path_manager(space);
        let mut context = MockContext::new(&mut path_manager);
        context.path_mut().on_validated();

        // PTO = smoothed_rtt + max(4*rttvar, kGranularity) + max_ack_delay
        // PTO = DEFAULT_INITIAL_RTT + 4*DEFAULT_INITIAL_RTT/2 + 10
        let expected_pto_duration = DEFAULT_INITIAL_RTT + 2 * DEFAULT_INITIAL_RTT + max_ack_delay;

        // Reset the timer so we can confirm it was set correctly
        manager.pto.timer.cancel();

        // Trigger 1:
        let sent_packet = space.new_packet_number(VarInt::from_u8(1));
        let ack_elicitation = AckElicitation::Eliciting;

        let outcome = transmission::Outcome {
            ack_elicitation,
            is_congestion_controlled: true,
            bytes_sent: packet_bytes,
            packet_number: space.new_packet_number(VarInt::from_u8(1)),
        };

        manager.on_packet_sent(sent_packet, outcome, time_sent, first_path_id, &mut context);

        // Expectation 1:
        assert!(manager.sent_packets.get(sent_packet).is_some());
        let actual_sent_packet = manager.sent_packets.get(sent_packet).unwrap();
        assert_eq!(
            actual_sent_packet.congestion_controlled,
            outcome.is_congestion_controlled
        );
        assert_eq!(actual_sent_packet.time_sent, time_sent);

        // checking outcome.is_congestion_controlled
        assert_eq!(actual_sent_packet.sent_bytes as usize, outcome.bytes_sent);

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.2.1
        //= type=test
        //# A sender SHOULD restart its PTO timer every time an ack-eliciting
        //# packet is sent
        let expected_pto = time_sent + expected_pto_duration;
        assert!(manager.pto.timer.is_armed());
        assert_eq!(Some(expected_pto), manager.pto.timer.iter().next());

        // Setup 2:
        // send 2nd packet on path 2nd path
        let sent_packet = space.new_packet_number(VarInt::from_u8(2));
        time_sent += Duration::from_millis(10);
        let outcome = transmission::Outcome {
            ack_elicitation,
            is_congestion_controlled: true,
            bytes_sent: packet_bytes,
            packet_number: space.new_packet_number(VarInt::from_u8(2)),
        };

        // Reset the timer so we can confirm it was set correctly
        manager.pto.timer.cancel();

        // Trigger 2:
        manager.on_packet_sent(
            sent_packet,
            outcome,
            time_sent,
            second_path_id,
            &mut context,
        );

        // Expectation 2:
        assert!(manager.sent_packets.get(sent_packet).is_some());
        let actual_sent_packet = manager.sent_packets.get(sent_packet).unwrap();
        assert_eq!(
            actual_sent_packet.congestion_controlled,
            outcome.is_congestion_controlled
        );
        assert_eq!(actual_sent_packet.time_sent, time_sent);

        // checking outcome.is_congestion_controlled
        assert_eq!(actual_sent_packet.sent_bytes as usize, outcome.bytes_sent);

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.2.1
        //= type=test
        //# A sender SHOULD restart its PTO timer every time an ack-eliciting
        //# packet is sent
        let expected_pto = time_sent + expected_pto_duration;
        assert!(manager.pto.timer.is_armed());
        assert_eq!(Some(expected_pto), manager.pto.timer.iter().next());
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#A.7
    //= type=test
    #[test]
    fn on_ack_frame() {
        let space = PacketNumberSpace::ApplicationData;
        let mut manager = Manager::new(space, Duration::from_millis(100));
        let packet_bytes = 128;
        let mut path_manager = helper_generate_path_manager(Duration::from_millis(10));
        let mut context = MockContext::new(&mut path_manager);

        // Start the pto backoff at 2 so we can tell if it was reset
        context.path_mut().pto_backoff = 2;

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

        assert_eq!(context.path().congestion_controller.lost_bytes, 0);
        assert_eq!(context.path().congestion_controller.on_rtt_update, 1);
        assert_eq!(context.path().pto_backoff, INITIAL_PTO_BACKOFF);
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
            context.path().rtt_estimator.latest_rtt(),
            Duration::from_millis(500)
        );
        assert_eq!(1, context.on_rtt_update_count);

        // Reset the pto backoff to 2 so we can tell if it was reset
        context.path_mut().pto_backoff = 2;

        // Acknowledging already acked packets
        let ack_receive_time = ack_receive_time + Duration::from_secs(1);
        ack_packets(1..=3, ack_receive_time, &mut context, &mut manager);

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#5.1
        //= type=test
        //# An RTT sample MUST NOT be generated on receiving an ACK frame that
        //# does not newly acknowledge at least one ack-eliciting packet.

        // Acknowledging already acked packets does not call on_new_packet_ack or change RTT
        assert_eq!(context.path().congestion_controller.lost_bytes, 0);
        assert_eq!(context.path().congestion_controller.on_rtt_update, 1);
        assert_eq!(context.path().pto_backoff, 2);
        assert_eq!(context.on_packet_ack_count, 2);
        assert_eq!(context.on_new_packet_ack_count, 1);
        assert_eq!(context.validate_packet_ack_count, 2);
        assert_eq!(context.on_packet_loss_count, 0);
        assert_eq!(
            context.path().rtt_estimator.latest_rtt(),
            Duration::from_millis(500)
        );
        assert_eq!(1, context.on_rtt_update_count);

        // Ack packets 7 to 9 (4 - 6 will be considered lost)
        let ack_receive_time = ack_receive_time + Duration::from_secs(1);
        ack_packets(7..=9, ack_receive_time, &mut context, &mut manager);

        assert_eq!(
            context.path().congestion_controller.lost_bytes,
            (packet_bytes * 3) as u32
        );
        assert_eq!(context.path().pto_backoff, INITIAL_PTO_BACKOFF);
        assert_eq!(context.on_packet_ack_count, 3);
        assert_eq!(context.on_new_packet_ack_count, 2);
        assert_eq!(context.validate_packet_ack_count, 3);
        assert_eq!(context.on_packet_loss_count, 3);
        assert_eq!(
            context.path().rtt_estimator.latest_rtt(),
            Duration::from_millis(2500)
        );
        assert_eq!(2, context.on_rtt_update_count);

        // Ack packet 10, but with a path that is not peer validated
        context.path_manager[path::Id::new(0)] = Path::new(
            Default::default(),
            connection::PeerId::TEST_ID,
            connection::LocalId::TEST_ID,
            context.path().rtt_estimator,
            MockCongestionController::default(),
            false,
        );
        context.path_mut().pto_backoff = 2;
        let ack_receive_time = ack_receive_time + Duration::from_millis(500);
        ack_packets(10..=10, ack_receive_time, &mut context, &mut manager);
        assert_eq!(context.path().congestion_controller.on_rtt_update, 1);
        assert_eq!(context.path().pto_backoff, 2);
        assert_eq!(context.on_packet_ack_count, 4);
        assert_eq!(context.on_new_packet_ack_count, 3);
        assert_eq!(context.validate_packet_ack_count, 4);
        assert_eq!(context.on_packet_loss_count, 3);
        assert_eq!(
            context.path().rtt_estimator.latest_rtt(),
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

        assert_eq!(context.path().congestion_controller.lost_bytes, 0);
        assert_eq!(context.path().congestion_controller.on_rtt_update, 1);
        assert_eq!(context.on_packet_ack_count, 5);
        assert_eq!(context.on_new_packet_ack_count, 4);
        assert_eq!(context.validate_packet_ack_count, 5);
        assert_eq!(context.on_packet_loss_count, 3);
        // RTT remains unchanged
        assert_eq!(
            context.path().rtt_estimator.latest_rtt(),
            Duration::from_millis(3000)
        );
        assert_eq!(3, context.on_rtt_update_count);
    }

    #[test]
    // pto_backoff reset should happen for the path the packet was sent on
    //
    // Setup 1:
    // - create path manager with two validated  paths
    // - send a packet on each path
    // - set pto_backoff to non-initial value
    //
    // Trigger 1:
    // - send ack for packet 1 on path 1
    //
    // Expectation 1:
    // - pto_backoff for first_path is reset
    // - pto_backoff for second_path_id path is not reset
    //
    // Trigger 2:
    // - send ack for packet 2 on path 1
    //
    // Expectation 2:
    // - pto_backoff for first_path path is not reset
    // - pto_backoff for second_path is reset
    fn process_new_acked_packets_update_pto_timer() {
        // Setup:
        let space = PacketNumberSpace::ApplicationData;
        let packet_bytes = 128;
        let (
            first_addr,
            first_path_id,
            _second_addr,
            second_path_id,
            mut manager,
            mut path_manager,
        ) = helper_generate_multi_path_manager(space);
        let mut context = MockContext::new(&mut path_manager);

        let time_sent = s2n_quic_platform::time::now() + Duration::from_secs(10);

        // Send packets 1 on first_path
        manager.on_packet_sent(
            space.new_packet_number(VarInt::from_u8(1)),
            transmission::Outcome {
                ack_elicitation: AckElicitation::Eliciting,
                is_congestion_controlled: true,
                bytes_sent: packet_bytes,
                packet_number: space.new_packet_number(VarInt::from_u8(1)),
            },
            time_sent,
            first_path_id,
            &mut context,
        );
        // Send packets 2 on second_path
        manager.on_packet_sent(
            space.new_packet_number(VarInt::from_u8(2)),
            transmission::Outcome {
                ack_elicitation: AckElicitation::Eliciting,
                is_congestion_controlled: true,
                bytes_sent: packet_bytes,
                packet_number: space.new_packet_number(VarInt::from_u8(1)),
            },
            time_sent,
            second_path_id,
            &mut context,
        );

        // Start the pto backoff at 2 so we can tell if it was reset
        context.path_mut_by_id(first_path_id).pto_backoff = 2;
        context.path_mut_by_id(second_path_id).pto_backoff = 2;

        // Trigger 1:
        // Ack packet first_path
        let ack_receive_time = time_sent + Duration::from_millis(500);
        ack_packets_on_path(
            1..=1,
            ack_receive_time,
            &mut context,
            &mut manager,
            first_addr,
        );

        // Expectation 1:
        assert_eq!(
            context.path_mut_by_id(first_path_id).pto_backoff,
            INITIAL_PTO_BACKOFF
        );
        assert_eq!(context.path_mut_by_id(second_path_id).pto_backoff, 2);

        // Reset the pto backoff to 2 so we can tell if it was reset
        context.path_mut_by_id(first_path_id).pto_backoff = 2;
        context.path_mut_by_id(second_path_id).pto_backoff = 2;

        // Trigger 2:
        // Ack packet second_path
        let ack_receive_time = ack_receive_time + Duration::from_secs(1);
        ack_packets_on_path(
            2..=2,
            ack_receive_time,
            &mut context,
            &mut manager,
            first_addr,
        );

        // Expectation 2:
        assert_eq!(context.path_mut_by_id(first_path_id).pto_backoff, 2);
        assert_eq!(
            context.path_mut_by_id(second_path_id).pto_backoff,
            INITIAL_PTO_BACKOFF
        );
    }

    #[test]
    // congestion_controller.on_packet_ack should be updated for the path the packet was sent on
    //
    // Setup 1:
    // - create path manager with two validated paths
    // - send a packet on each path
    //  - packet 1 on path 1
    //  - packet 2 on path 2
    //
    // Trigger 1:
    // - send ack for packet 1 on path 1
    //
    // Expectation 1:
    // - cc.on_packet_ack should be updated for first_path
    // - cc.on_packet_ack should not be updated for second_path
    //
    // Trigger 2:
    // - send ack for packet 2 on path 2
    //
    // Expectation 2:
    // - cc.on_packet_ack should not be updated for first_path
    // - cc.on_packet_ack should be updated for second_path
    fn process_new_acked_packets_congestion_controller() {
        // Setup:
        let space = PacketNumberSpace::ApplicationData;
        let packet_bytes = 128;
        let (
            first_addr,
            first_path_id,
            _second_addr,
            second_path_id,
            mut manager,
            mut path_manager,
        ) = helper_generate_multi_path_manager(space);
        let mut context = MockContext::new(&mut path_manager);

        let time_sent = s2n_quic_platform::time::now() + Duration::from_secs(10);

        // Send packets 1 on first_path
        manager.on_packet_sent(
            space.new_packet_number(VarInt::from_u8(1)),
            transmission::Outcome {
                ack_elicitation: AckElicitation::Eliciting,
                is_congestion_controlled: true,
                bytes_sent: packet_bytes,
                packet_number: space.new_packet_number(VarInt::from_u8(1)),
            },
            time_sent,
            first_path_id,
            &mut context,
        );
        // Send packets 2 on second_path
        manager.on_packet_sent(
            space.new_packet_number(VarInt::from_u8(2)),
            transmission::Outcome {
                ack_elicitation: AckElicitation::Eliciting,
                is_congestion_controlled: true,
                bytes_sent: packet_bytes,
                packet_number: space.new_packet_number(VarInt::from_u8(1)),
            },
            time_sent,
            second_path_id,
            &mut context,
        );

        // Trigger 1:
        // Ack packet 1 on path 1
        let ack_receive_time = time_sent + Duration::from_millis(500);
        ack_packets_on_path(
            1..=1,
            ack_receive_time,
            &mut context,
            &mut manager,
            first_addr,
        );

        // Expectation 1:
        assert_eq!(
            context
                .path_by_id(first_path_id)
                .congestion_controller
                .on_packet_ack,
            1
        );
        assert_eq!(
            context
                .path_by_id(second_path_id)
                .congestion_controller
                .on_packet_ack,
            0
        );

        // Trigger 2:
        // Ack packet 2 on path 1
        let ack_receive_time = time_sent + Duration::from_millis(500);
        ack_packets_on_path(
            2..=2,
            ack_receive_time,
            &mut context,
            &mut manager,
            first_addr,
        );

        // Expectation 2:
        assert_eq!(
            context
                .path_by_id(first_path_id)
                .congestion_controller
                .on_packet_ack,
            1
        );
        assert_eq!(
            context
                .path_by_id(second_path_id)
                .congestion_controller
                .on_packet_ack,
            1
        );
    }

    #[test]
    // since pto is shared across paths, acks on either paths should update pto timer
    //
    // Setup 1:
    // - create path manager with two peer_validated and not AmplificationLimited paths
    // -
    // - send a packet on each path (make sure sent_packets is non-empty)
    //  - packet 1 on path 1
    //  - packet 2 on path 2
    //
    // Trigger 1:
    // - send ack for packet 1 on path 1
    //
    // Expectation 1:
    // - pto.timer should be armed
    //
    // Setup 2:
    // - send a packet on path 1 (make sure sent_packets is non-empty)
    //
    // Trigger 2:
    // - send ack for packet 2 on path 2
    //
    // Expectation 2:
    // - pto.timer should be armed
    fn process_new_acked_packets_pto_timer() {
        // Setup:
        let space = PacketNumberSpace::ApplicationData;
        let packet_bytes = 128;
        let (first_addr, first_path_id, second_addr, second_path_id, mut manager, mut path_manager) =
            helper_generate_multi_path_manager(space);
        let mut context = MockContext::new(&mut path_manager);

        let time_sent = s2n_quic_platform::time::now() + Duration::from_secs(10);

        // Send packets 1 on first_path
        manager.on_packet_sent(
            space.new_packet_number(VarInt::from_u8(1)),
            transmission::Outcome {
                ack_elicitation: AckElicitation::Eliciting,
                is_congestion_controlled: true,
                bytes_sent: packet_bytes,
                packet_number: space.new_packet_number(VarInt::from_u8(1)),
            },
            time_sent,
            first_path_id,
            &mut context,
        );
        // Send packets 2 on second_path
        manager.on_packet_sent(
            space.new_packet_number(VarInt::from_u8(2)),
            transmission::Outcome {
                ack_elicitation: AckElicitation::Eliciting,
                is_congestion_controlled: true,
                bytes_sent: packet_bytes,
                packet_number: space.new_packet_number(VarInt::from_u8(1)),
            },
            time_sent,
            second_path_id,
            &mut context,
        );

        manager.pto.timer.cancel();

        // Trigger 1:
        // Ack packet 1 on path 1
        let ack_receive_time = time_sent + Duration::from_millis(500);
        ack_packets_on_path(
            1..=1,
            ack_receive_time,
            &mut context,
            &mut manager,
            first_addr,
        );

        // Expectation 1:
        assert!(manager.pto.timer.is_armed());

        // Setup 2:
        manager.pto.timer.cancel();
        // Send packets 3 on first path so that sent_packets is non-empty
        manager.on_packet_sent(
            space.new_packet_number(VarInt::from_u8(3)),
            transmission::Outcome {
                ack_elicitation: AckElicitation::Eliciting,
                is_congestion_controlled: true,
                bytes_sent: packet_bytes,
                packet_number: space.new_packet_number(VarInt::from_u8(1)),
            },
            time_sent,
            first_path_id,
            &mut context,
        );

        // Trigger 2:
        // Ack packet 2 on path 1
        let ack_receive_time = time_sent + Duration::from_millis(500);
        ack_packets_on_path(
            2..=2,
            ack_receive_time,
            &mut context,
            &mut manager,
            second_addr,
        );

        // Expectation 2:
        assert!(manager.pto.timer.is_armed());
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#5.1
    //= type=test
    //# To avoid generating multiple RTT samples for a single packet, an ACK
    //# frame SHOULD NOT be used to update RTT estimates if it does not newly
    //# acknowledge the largest acknowledged packet.
    #[test]
    fn no_rtt_update_when_not_acknowledging_the_largest_acknowledged_packet() {
        let space = PacketNumberSpace::ApplicationData;
        let mut manager = Manager::new(space, Duration::from_millis(100));
        let packet_bytes = 128;
        let mut path_manager = helper_generate_path_manager(Duration::from_millis(10));
        let mut context = MockContext::new(&mut path_manager);

        let time_sent = s2n_quic_platform::time::now() + Duration::from_secs(10);

        // Send 2 packets
        manager.on_packet_sent(
            space.new_packet_number(VarInt::from_u8(0)),
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
        manager.on_packet_sent(
            space.new_packet_number(VarInt::from_u8(1)),
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

        assert_eq!(manager.sent_packets.iter().count(), 2);

        // Ack packet 1
        let ack_receive_time = time_sent + Duration::from_millis(500);
        ack_packets(1..=1, ack_receive_time, &mut context, &mut manager);

        // New rtt estimate because the largest packet was newly acked
        assert_eq!(context.path().congestion_controller.on_rtt_update, 1);
        assert_eq!(
            manager.largest_acked_packet,
            Some(space.new_packet_number(VarInt::from_u8(1)))
        );
        assert_eq!(
            context.path().rtt_estimator.latest_rtt(),
            Duration::from_millis(500)
        );
        assert_eq!(1, context.on_rtt_update_count);

        // Ack packets 0 and 1
        let ack_receive_time = time_sent + Duration::from_millis(1500);
        ack_packets(0..=1, ack_receive_time, &mut context, &mut manager);

        // No new rtt estimate because the largest packet was not newly acked
        assert_eq!(context.path().congestion_controller.on_rtt_update, 1);
        assert_eq!(
            context.path().rtt_estimator.latest_rtt(),
            Duration::from_millis(500)
        );
        assert_eq!(1, context.on_rtt_update_count);
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#9.4
    //= type=test
    //# Packets sent on the old path MUST NOT contribute to
    //# congestion control or RTT estimation for the new path.
    #[test]
    fn no_rtt_update_when_receiving_packet_on_different_path() {
        let space = PacketNumberSpace::ApplicationData;
        let mut manager = Manager::new(space, Duration::from_millis(100));
        let packet_bytes = 128;
        let mut path_manager = helper_generate_path_manager(Duration::from_millis(10));
        let mut context = MockContext::new(&mut path_manager);

        let time_sent = s2n_quic_platform::time::now() + Duration::from_secs(10);

        // Send packet
        manager.on_packet_sent(
            space.new_packet_number(VarInt::from_u8(0)),
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
        manager.on_packet_sent(
            space.new_packet_number(VarInt::from_u8(1)),
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
        assert_eq!(manager.sent_packets.iter().count(), 2);

        // Ack packet 0 on diffent path as it was sent.. expect no rtt update
        let ack_receive_time = time_sent + Duration::from_millis(500);
        let ipv6_addr = SocketAddress::IpV6(Default::default());
        ack_packets_on_path(
            0..=0,
            ack_receive_time,
            &mut context,
            &mut manager,
            ipv6_addr,
        );

        // no rtt estimate because the packet was received on different path
        assert_eq!(context.path().congestion_controller.on_rtt_update, 0);
        assert_eq!(
            manager.largest_acked_packet,
            Some(space.new_packet_number(VarInt::from_u8(0)))
        );
        assert_eq!(
            context.path().rtt_estimator.latest_rtt(),
            Duration::from_millis(DEFAULT_INITIAL_RTT.as_millis() as u64)
        );
        assert_eq!(0, context.on_rtt_update_count);

        // Ack packet 1 on same path as it was sent.. expect rtt update
        let ack_receive_time = time_sent + Duration::from_millis(1500);
        let ipv4_addr = SocketAddress::IpV4(Default::default());
        ack_packets_on_path(
            1..=1,
            ack_receive_time,
            &mut context,
            &mut manager,
            ipv4_addr,
        );

        // rtt estimate because the packet was received on same path
        assert_eq!(context.path().congestion_controller.on_rtt_update, 1);
        assert_eq!(
            manager.largest_acked_packet,
            Some(space.new_packet_number(VarInt::from_u8(1)))
        );
        assert_eq!(
            context.path().rtt_estimator.latest_rtt(),
            Duration::from_millis(1500)
        );
        assert_eq!(1, context.on_rtt_update_count);
    }

    #[test]
    // It is possible to receive acks for packets that were sent on different paths. In this case
    // we still update rtt if the largest acked packet was sent/received on the same path.
    //
    // Setup:
    // - create path manager with two paths
    //
    // Trigger:
    // - send packet on each path
    //   - packet 1: 1st path: time 200
    //   - packet 0: 2nd path: time 500
    // - send ack for both packet on 1st path
    //
    // Expectation:
    // - update rtt for 1st path using packet 1 time
    //   - since packet 1 is largest acked and was sent/received on 1st path
    // - rtt for 2nd apth should be unchanged
    fn rtt_update_when_receiving_ack_from_multiple_paths() {
        // Setup:
        let space = PacketNumberSpace::ApplicationData;
        let packet_bytes = 128;
        let (
            first_addr,
            first_path_id,
            _second_addr,
            second_path_id,
            mut manager,
            mut path_manager,
        ) = helper_generate_multi_path_manager(space);
        let mut context = MockContext::new(&mut path_manager);

        // Trigger:
        let time_sent = s2n_quic_platform::time::now() + Duration::from_secs(10);
        let first_sent_time = time_sent + Duration::from_millis(200);
        let second_sent_time = time_sent + Duration::from_millis(500);
        let ack_receive_time = time_sent + Duration::from_millis(1000);

        // send packet 1 (largest) on first path. sent + 200
        manager.on_packet_sent(
            space.new_packet_number(VarInt::from_u8(1)),
            transmission::Outcome {
                ack_elicitation: AckElicitation::Eliciting,
                is_congestion_controlled: true,
                bytes_sent: packet_bytes,
                packet_number: space.new_packet_number(VarInt::from_u8(1)),
            },
            first_sent_time,
            first_path_id,
            &mut context,
        );
        // send packet 0 packet on second path. sent +500
        manager.on_packet_sent(
            space.new_packet_number(VarInt::from_u8(0)),
            transmission::Outcome {
                ack_elicitation: AckElicitation::Eliciting,
                is_congestion_controlled: true,
                bytes_sent: packet_bytes,
                packet_number: space.new_packet_number(VarInt::from_u8(1)),
            },
            second_sent_time,
            second_path_id,
            &mut context,
        );
        assert_eq!(manager.sent_packets.iter().count(), 2);

        // receive ack for both packets on address of first path (first packet is largest)
        ack_packets_on_path(
            0..=1,
            ack_receive_time,
            &mut context,
            &mut manager,
            first_addr,
        );

        let first_path = context.path_by_id(first_path_id);
        let second_path = context.path_by_id(second_path_id);

        // Expectation:
        // received/sent largest packet on first path so we expect an rtt update using sent time of first path
        // assert common component
        assert_eq!(
            manager.largest_acked_packet,
            Some(space.new_packet_number(VarInt::from_u8(1)))
        );
        assert_eq!(context.on_rtt_update_count, 1);

        // assert first path
        assert_eq!(
            first_path.rtt_estimator.latest_rtt(),
            ack_receive_time - first_sent_time
        );
        assert_eq!(first_path.congestion_controller.on_rtt_update, 1);

        // assert second path
        assert_eq!(
            second_path.rtt_estimator.latest_rtt(),
            Duration::from_millis(DEFAULT_INITIAL_RTT.as_millis() as u64)
        );
        assert_eq!(second_path.congestion_controller.on_rtt_update, 0);
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#A.10
    //= type=test
    #[test]
    fn detect_and_remove_lost_packets() {
        let space = PacketNumberSpace::ApplicationData;
        let mut manager = Manager::new(space, Duration::from_millis(100));
        let now = s2n_quic_platform::time::now();
        let mut path_manager = helper_generate_path_manager(Duration::from_millis(10));
        let mut context = MockContext::new(&mut path_manager);
        let active_path_id = path::Id::new(0);

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
            active_path_id,
            &mut context,
        );

        // time threshold = max(kTimeThreshold * max(smoothed_rtt, latest_rtt), kGranularity)
        // time threshold = max(9/8 * 8) = 9
        context.path_mut().rtt_estimator.update_rtt(
            Duration::from_secs(0),
            Duration::from_secs(8),
            now,
            true,
            space,
        );
        let expected_time_threshold = Duration::from_secs(9);
        assert_eq!(
            expected_time_threshold,
            Manager::calculate_loss_time_threshold(&context.path().rtt_estimator)
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
            active_path_id,
            &mut context,
        );

        // Send a packet that is less than the largest acked but not lost
        let not_lost = space.new_packet_number(VarInt::from_u8(9));
        manager.on_packet_sent(not_lost, outcome, time_sent, active_path_id, &mut context);

        // Send a packet larger than the largest acked (not lost)
        let larger_than_largest = manager.largest_acked_packet.unwrap().next().unwrap();
        manager.on_packet_sent(
            larger_than_largest,
            outcome,
            time_sent,
            active_path_id,
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

        manager.detect_and_remove_lost_packets(now, active_path_id, &mut context);

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.1.2
        //= type=test
        //# Once a later packet within the same packet number space has been
        //# acknowledged, an endpoint SHOULD declare an earlier packet lost if it
        //# was sent a threshold amount of time in the past.

        // Two packets lost, each size 1 byte
        assert_eq!(context.path().congestion_controller.lost_bytes, 2);
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
        let mut path_manager = helper_generate_path_manager(Duration::from_millis(10));
        let mut context = MockContext::new(&mut path_manager);
        manager.largest_acked_packet = Some(space.new_packet_number(VarInt::from_u8(10)));

        let active_path_id = path::Id::new(0);

        let time_sent = s2n_quic_platform::time::now();
        let outcome = transmission::Outcome {
            ack_elicitation: AckElicitation::Eliciting,
            is_congestion_controlled: true,
            bytes_sent: 1,
            packet_number: space.new_packet_number(VarInt::from_u8(1)),
        };

        // Send a packet that is less than the largest acked but not lost
        let not_lost = space.new_packet_number(VarInt::from_u8(9));
        manager.on_packet_sent(not_lost, outcome, time_sent, active_path_id, &mut context);

        manager.detect_and_remove_lost_packets(time_sent, active_path_id, &mut context);

        // Verify no lost bytes are sent to the congestion controller and
        // on_packets_lost is not called
        assert_eq!(context.lost_packets.len(), 0);
        assert_eq!(context.path().congestion_controller.lost_bytes, 0);
        assert_eq!(context.path().congestion_controller.on_packets_lost, 0);
    }

    #[test]
    fn persistent_congestion() {
        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#7.6.2
        //= type=test
        //# A sender that does not have state for all packet
        //# number spaces or an implementation that cannot compare send times
        //# across packet number spaces MAY use state for just the packet number
        //# space that was acknowledged.
        let space = PacketNumberSpace::ApplicationData;
        let mut manager = Manager::new(space, Duration::from_millis(100));
        let mut path_manager = helper_generate_path_manager(Duration::from_millis(10));
        let mut context = MockContext::new(&mut path_manager);
        let active_path_id = path::Id::new(0);

        let time_zero = s2n_quic_platform::time::now() + Duration::from_secs(10);
        // The RFC doesn't mention it, but it is implied that the first RTT sample has already
        // been received when this example begins, otherwise packet #2 would not be considered
        // part of the persistent congestion period.
        context.path_mut().rtt_estimator.update_rtt(
            Duration::from_millis(10),
            Duration::from_millis(700),
            s2n_quic_platform::time::now(),
            true,
            space,
        );

        let outcome = transmission::Outcome {
            ack_elicitation: AckElicitation::Eliciting,
            is_congestion_controlled: true,
            bytes_sent: 1,
            packet_number: space.new_packet_number(VarInt::from_u8(1)),
        };

        // t=0: Send packet #1 (app data)
        manager.on_packet_sent(
            space.new_packet_number(VarInt::from_u8(1)),
            outcome,
            time_zero,
            active_path_id,
            &mut context,
        );

        // t=1: Send packet #2 (app data)
        manager.on_packet_sent(
            space.new_packet_number(VarInt::from_u8(2)),
            outcome,
            time_zero + Duration::from_secs(1),
            active_path_id,
            &mut context,
        );

        // t=1.2: Recv acknowledgement of #1
        ack_packets(
            1..=1,
            time_zero + Duration::from_millis(1200),
            &mut context,
            &mut manager,
        );

        // t=2-6: Send packets #3 - #7 (app data)
        for t in 2..=6 {
            manager.on_packet_sent(
                space.new_packet_number(VarInt::from_u8(t + 1)),
                outcome,
                time_zero + Duration::from_secs(t.into()),
                active_path_id,
                &mut context,
            );
        }

        // t=8: Send packet #8 (PTO 1)
        manager.on_packet_sent(
            space.new_packet_number(VarInt::from_u8(8)),
            outcome,
            time_zero + Duration::from_secs(8),
            active_path_id,
            &mut context,
        );

        // t=12: Send packet #9 (PTO 2)
        manager.on_packet_sent(
            space.new_packet_number(VarInt::from_u8(9)),
            outcome,
            time_zero + Duration::from_secs(12),
            active_path_id,
            &mut context,
        );

        // t=12.2: Recv acknowledgement of #9
        ack_packets(
            9..=9,
            time_zero + Duration::from_millis(12200),
            &mut context,
            &mut manager,
        );

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#7.6.3
        //# Packets 2 through 8 are declared lost when the acknowledgement for
        //# packet 9 is received at t = 12.2.
        assert_eq!(7, context.on_packet_loss_count);

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#7.6.3
        //# The congestion period is calculated as the time between the oldest
        //# and newest lost packets: 8 - 1 = 7.
        assert!(
            context
                .path()
                .rtt_estimator
                .persistent_congestion_threshold()
                < Duration::from_secs(7)
        );
        assert_eq!(
            Some(true),
            context.path().congestion_controller.persistent_congestion
        );
        assert_eq!(context.path().rtt_estimator.first_rtt_sample(), None);

        // t=20: Send packet #10
        manager.on_packet_sent(
            space.new_packet_number(VarInt::from_u8(10)),
            outcome,
            time_zero + Duration::from_secs(20),
            active_path_id,
            &mut context,
        );

        // t=21: Recv acknowledgement of #10
        ack_packets(
            10..=10,
            time_zero + Duration::from_secs(21),
            &mut context,
            &mut manager,
        );

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#5.2
        //= type=test
        //# Endpoints SHOULD set the min_rtt to the newest RTT sample after
        //# persistent congestion is established.
        assert_eq!(
            context.path().rtt_estimator.min_rtt(),
            Duration::from_secs(1)
        );
        assert_eq!(
            context.path().rtt_estimator.smoothed_rtt(),
            Duration::from_secs(1)
        );
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#7.6
    //= type=test
    #[test]
    fn persistent_congestion_multiple_periods() {
        let space = PacketNumberSpace::ApplicationData;
        let mut manager = Manager::new(space, Duration::from_millis(100));
        let mut path_manager = helper_generate_path_manager(Duration::from_millis(10));
        let mut context = MockContext::new(&mut path_manager);
        let time_zero = s2n_quic_platform::time::now() + Duration::from_secs(10);

        let outcome = transmission::Outcome {
            ack_elicitation: AckElicitation::Eliciting,
            is_congestion_controlled: true,
            bytes_sent: 1,
            packet_number: space.new_packet_number(VarInt::from_u8(1)),
        };

        // t=0: Send packet #1 (app data)
        manager.on_packet_sent(
            space.new_packet_number(VarInt::from_u8(1)),
            outcome,
            time_zero,
            path::Id::new(0),
            &mut context,
        );

        // t=1: Send packet #2 (app data)
        manager.on_packet_sent(
            space.new_packet_number(VarInt::from_u8(2)),
            outcome,
            time_zero + Duration::from_secs(1),
            path::Id::new(0),
            &mut context,
        );

        // t=1.2: Recv acknowledgement of #1
        ack_packets(
            1..=1,
            time_zero + Duration::from_millis(1200),
            &mut context,
            &mut manager,
        );

        // t=2-6: Send packets #3 - #7 (app data)
        for t in 2..=6 {
            manager.on_packet_sent(
                space.new_packet_number(VarInt::from_u8(t + 1)),
                outcome,
                time_zero + Duration::from_secs(t.into()),
                path::Id::new(0),
                &mut context,
            );
        }

        // Skip packet #8, which ends one persistent congestion period.

        // t=8: Send packet #9 (app data)
        manager.on_packet_sent(
            space.new_packet_number(VarInt::from_u8(9)),
            outcome,
            time_zero + Duration::from_secs(8),
            path::Id::new(0),
            &mut context,
        );

        // t=20: Send packet #10 (app data)
        manager.on_packet_sent(
            space.new_packet_number(VarInt::from_u8(10)),
            outcome,
            time_zero + Duration::from_secs(20),
            path::Id::new(0),
            &mut context,
        );

        // t=30: Send packet #11 (app data)
        manager.on_packet_sent(
            space.new_packet_number(VarInt::from_u8(11)),
            outcome,
            time_zero + Duration::from_secs(30),
            path::Id::new(0),
            &mut context,
        );

        // t=30.2: Recv acknowledgement of #11
        ack_packets(
            11..=11,
            time_zero + Duration::from_millis(30200),
            &mut context,
            &mut manager,
        );

        // Packets 2 though 7 and 9-10 should be lost
        assert_eq!(8, context.on_packet_loss_count);

        // The largest contiguous period of lost packets is #9 (sent at t8) to #10 (sent at t20)
        assert!(
            context
                .path()
                .rtt_estimator
                .persistent_congestion_threshold()
                < Duration::from_secs(12)
        );
        assert_eq!(
            Some(true),
            context.path().congestion_controller.persistent_congestion
        );
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#7.6.2
    //= type=test
    //# The persistent congestion period SHOULD NOT start until there is at
    //# least one RTT sample.
    #[test]
    fn persistent_congestion_period_does_not_start_until_rtt_sample() {
        let space = PacketNumberSpace::ApplicationData;
        let mut manager = Manager::new(space, Duration::from_millis(100));
        let mut path_manager = helper_generate_path_manager(Duration::from_millis(10));
        let mut context = MockContext::new(&mut path_manager);
        let time_zero = s2n_quic_platform::time::now() + Duration::from_secs(10);

        let outcome = transmission::Outcome {
            ack_elicitation: AckElicitation::Eliciting,
            is_congestion_controlled: true,
            bytes_sent: 1,
            packet_number: space.new_packet_number(VarInt::from_u8(1)),
        };

        // t=0: Send packet #1 (app data)
        manager.on_packet_sent(
            space.new_packet_number(VarInt::from_u8(1)),
            outcome,
            time_zero,
            path::Id::new(0),
            &mut context,
        );

        // t=10: Send packet #2 (app data)
        manager.on_packet_sent(
            space.new_packet_number(VarInt::from_u8(2)),
            outcome,
            time_zero + Duration::from_secs(10),
            path::Id::new(0),
            &mut context,
        );

        // t=20: Send packet #3 (app data)
        manager.on_packet_sent(
            space.new_packet_number(VarInt::from_u8(3)),
            outcome,
            time_zero + Duration::from_secs(20),
            path::Id::new(0),
            &mut context,
        );

        // t=20.1: Recv acknowledgement of #3. The first RTT sample is collected
        //         now, at t=20.1
        ack_packets(
            3..=3,
            time_zero + Duration::from_millis(20_100),
            &mut context,
            &mut manager,
        );

        // There is no persistent congestion, because the lost packets were all
        // sent prior to the first RTT sample
        assert_eq!(context.path().congestion_controller.on_packets_lost, 2);
        assert_eq!(
            context.path().congestion_controller.persistent_congestion,
            Some(false)
        );
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.2.1
    //= type=test
    #[test]
    fn update_pto_timer() {
        let space = PacketNumberSpace::ApplicationData;
        let mut manager = Manager::new(space, Duration::from_millis(10));
        let now = s2n_quic_platform::time::now() + Duration::from_secs(10);
        let is_handshake_confirmed = true;
        let mut path_manager = helper_generate_path_manager(Duration::from_millis(10));
        let mut context = MockContext::new(&mut path_manager);

        context.path_mut().rtt_estimator.update_rtt(
            Duration::from_millis(0),
            Duration::from_millis(500),
            now,
            true,
            space,
        );
        context.path_mut().rtt_estimator.update_rtt(
            Duration::from_millis(0),
            Duration::from_millis(1000),
            now,
            true,
            space,
        );
        // The path will be at the anti-amplification limit
        context.path_mut().on_bytes_received(1200);
        context.path_mut().on_bytes_transmitted((1200 * 3) + 1);
        // Arm the PTO so we can verify it is cancelled
        manager.pto.timer.set(now + Duration::from_secs(10));
        manager.update_pto_timer(&context.path(), now, is_handshake_confirmed);

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.2.2.1
        //= type=test
        //# If no additional data can be sent, the server's PTO timer MUST NOT be
        //# armed until datagrams have been received from the client, because
        //# packets sent on PTO count against the anti-amplification limit.
        assert!(!manager.pto.timer.is_armed());

        // Arm the PTO so we can verify it is cancelled
        manager.pto.timer.set(now + Duration::from_secs(10));
        // Validate the path so it is not at the anti-amplification limit
        context.path_mut().on_validated();
        context.path_mut().on_peer_validated();
        manager.update_pto_timer(&context.path(), now, is_handshake_confirmed);

        // Since the path is peer validated and sent packets is empty, PTO is cancelled
        assert!(!manager.pto.timer.is_armed());

        // Reset the path back to not peer validated
        context.path_manager[path::Id::new(0)] = Path::new(
            Default::default(),
            connection::PeerId::TEST_ID,
            connection::LocalId::TEST_ID,
            RttEstimator::new(manager.max_ack_delay),
            MockCongestionController::default(),
            false,
        );
        context.path_mut().on_validated();
        context.path_mut().pto_backoff = 2;
        let is_handshake_confirmed = false;
        manager.update_pto_timer(&context.path(), now, is_handshake_confirmed);

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.2.1
        //= type=test
        //# An endpoint MUST NOT set its PTO timer for the application data
        //# packet number space until the handshake is confirmed.
        assert!(!manager.pto.timer.is_armed());

        // Set is handshake confirmed back to true
        let is_handshake_confirmed = true;
        manager.update_pto_timer(&context.path(), now, is_handshake_confirmed);

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
        context.path_mut().rtt_estimator.update_rtt(
            Duration::from_millis(0),
            Duration::from_millis(2000),
            now,
            true,
            space,
        );
        manager.update_pto_timer(&context.path(), now, is_handshake_confirmed);

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

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.2.2.1
    //= type=test
    //# That is,
    //# the client MUST set the probe timer if the client has not received an
    //# acknowledgement for one of its Handshake packets and the handshake is
    //# not confirmed (see Section 4.1.2 of [QUIC-TLS]), even if there are no
    //# packets in flight.
    #[test]
    fn pto_armed_if_handshake_not_confirmed() {
        let space = PacketNumberSpace::Handshake;
        let mut manager = Manager::new(space, Duration::from_millis(10));
        let now = s2n_quic_platform::time::now() + Duration::from_secs(10);
        let is_handshake_confirmed = false;

        let mut path = Path::new(
            Default::default(),
            connection::PeerId::TEST_ID,
            connection::LocalId::TEST_ID,
            RttEstimator::new(Duration::from_millis(10)),
            Unlimited::default(),
            false,
        );

        path.on_validated();

        manager.update_pto_timer(&path, now, is_handshake_confirmed);

        assert!(manager.pto.timer.is_armed());
    }
    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.2.1
    //= type=test
    //# The PTO period MUST be at least kGranularity, to avoid the timer
    //# expiring immediately.
    #[test]
    fn pto_must_be_at_least_k_granularity() {
        let space = PacketNumberSpace::Handshake;
        let max_ack_delay = Duration::from_millis(0);
        let mut manager = Manager::new(space, max_ack_delay);
        let now = s2n_quic_platform::time::now();

        let mut path = Path::new(
            Default::default(),
            connection::PeerId::TEST_ID,
            connection::LocalId::TEST_ID,
            RttEstimator::new(max_ack_delay),
            Unlimited::default(),
            false,
        );

        // Update RTT with the smallest possible sample
        path.rtt_estimator.update_rtt(
            Duration::from_millis(0),
            Duration::from_nanos(1),
            now,
            true,
            space,
        );

        manager
            .pto
            .update(now, path.rtt_estimator.pto_period(path.pto_backoff, space));

        assert!(manager.pto.timer.is_armed());
        assert!(manager.pto.timer.iter().next().unwrap() >= now + K_GRANULARITY);
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.2.1
    //= type=test
    #[test]
    fn on_timeout() {
        let space = PacketNumberSpace::ApplicationData;
        let mut manager = Manager::new(space, Duration::from_millis(10));
        let now = s2n_quic_platform::time::now() + Duration::from_secs(10);
        manager.largest_acked_packet = Some(space.new_packet_number(VarInt::from_u8(10)));
        let mut path_manager = helper_generate_path_manager(Duration::from_millis(10));
        let mut context = MockContext::new(&mut path_manager);
        let active_path_id = path::Id::new(0);

        let mut expected_pto_backoff = context.path().pto_backoff;

        // Loss timer is armed but not expired yet, nothing happens
        manager.loss_timer.set(now + Duration::from_secs(10));
        manager.on_timeout(now, active_path_id, &mut context);
        assert_eq!(context.on_packet_loss_count, 0);
        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.2.1
        //= type=test
        //# The PTO timer MUST NOT be set if a timer is set for time threshold
        //# loss detection; see Section 6.1.2.
        assert!(!manager.pto.timer.is_armed());
        assert_eq!(expected_pto_backoff, context.path().pto_backoff);

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
        manager.on_timeout(now, active_path_id, &mut context);
        assert_eq!(context.on_packet_loss_count, 1);
        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.2.1
        //= type=test
        //# The PTO timer MUST NOT be set if a timer is set for time threshold
        //# loss detection; see Section 6.1.2.
        assert!(!manager.pto.timer.is_armed());
        assert_eq!(expected_pto_backoff, context.path().pto_backoff);

        // Loss timer is not armed, pto timer is not armed
        manager.loss_timer.cancel();
        manager.on_timeout(now, active_path_id, &mut context);
        assert_eq!(expected_pto_backoff, context.path().pto_backoff);

        // Loss timer is not armed, pto timer is armed but not expired
        manager.loss_timer.cancel();
        manager.pto.timer.set(now + Duration::from_secs(5));
        manager.on_timeout(now, active_path_id, &mut context);
        assert_eq!(expected_pto_backoff, context.path().pto_backoff);

        // Loss timer is not armed, pto timer is expired without bytes in flight
        expected_pto_backoff *= 2;
        manager.pto.timer.set(now - Duration::from_secs(5));
        manager.on_timeout(now, active_path_id, &mut context);
        assert_eq!(expected_pto_backoff, context.path().pto_backoff);
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
        manager.on_timeout(now, active_path_id, &mut context);
        assert_eq!(expected_pto_backoff, context.path().pto_backoff);

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

        // PTO timer is armed
        manager.loss_timer.cancel();
        manager.pto.timer.set(pto_time);
        assert_eq!(manager.timers().count(), 1);
        assert_eq!(manager.timers().next(), Some(pto_time));

        // Both timers are armed, only loss time is returned
        manager.loss_timer.set(loss_time);
        manager.pto.timer.set(pto_time);
        assert_eq!(manager.timers().count(), 1);
        assert_eq!(manager.timers().next(), Some(loss_time));
    }

    #[test]
    fn on_transmit() {
        let space = PacketNumberSpace::ApplicationData;
        let mut manager = Manager::new(space, Duration::from_millis(10));
        let mut frame_buffer = OutgoingFrameBuffer::new();
        let mut context = MockWriteContext::new(
            s2n_quic_platform::time::now(),
            &mut frame_buffer,
            transmission::Constraint::CongestionLimited, // Recovery manager ignores constraints
            endpoint::Type::Client,
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
        context.write_frame_forced(&frame::Padding { length: 1 });
        assert!(!context.ack_elicitation().is_ack_eliciting());
        manager.on_transmit(&mut context);
        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.2.4
        //= type=test
        //# All probe packets sent on a PTO MUST be ack-eliciting.

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.2.4
        //= type=test
        //# When the PTO timer expires, an ack-eliciting packet MUST be sent.

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.2.4
        //= type=test
        //# When there is no data to send, the sender SHOULD send
        //# a PING or other ack-eliciting frame in a single packet, re-arming the
        //# PTO timer.
        assert!(context.ack_elicitation().is_ack_eliciting());
        assert_eq!(manager.pto.state, PtoState::Idle);

        // One transmission required, ack eliciting
        manager.pto.state = RequiresTransmission(1);
        context.write_frame_forced(&frame::Ping);
        manager.on_transmit(&mut context);
        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.2.4
        //= type=test
        //# All probe packets sent on a PTO MUST be ack-eliciting.

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.2.4
        //= type=test
        //# When the PTO timer expires, an ack-eliciting packet MUST be sent.
        assert!(context.ack_elicitation().is_ack_eliciting());
        assert_eq!(manager.pto.state, PtoState::Idle);

        // Two transmissions required
        manager.pto.state = RequiresTransmission(2);
        manager.on_transmit(&mut context);
        assert_eq!(manager.pto.state, RequiresTransmission(1));
    }

    // Helper function that will call on_ack_frame with the given packet numbers
    fn ack_packets_on_path<CC: CongestionController, Ctx: Context<CC>>(
        range: RangeInclusive<u8>,
        ack_receive_time: Timestamp,
        context: &mut Ctx,
        manager: &mut Manager,
        remote_address: SocketAddress,
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
            remote_address,
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

    // Helper function that will call on_ack_frame with the given packet numbers
    fn ack_packets<CC: CongestionController, Ctx: Context<CC>>(
        range: RangeInclusive<u8>,
        ack_receive_time: Timestamp,
        context: &mut Ctx,
        manager: &mut Manager,
    ) {
        ack_packets_on_path(
            range,
            ack_receive_time,
            context,
            manager,
            Default::default(),
        )
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

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#7.5
    //= type=test
    //# A sender MUST however count these packets as being additionally in
    //# flight, since these packets add network load without establishing
    //# packet loss.
    #[test]
    fn probe_packets_count_towards_bytes_in_flight() {
        let space = PacketNumberSpace::ApplicationData;
        let mut manager = Manager::new(space, Duration::from_millis(10));

        manager.pto.state = PtoState::RequiresTransmission(2);

        let mut path_manager = helper_generate_path_manager(Duration::from_millis(10));
        let mut context = MockContext::new(&mut path_manager);
        let outcome = transmission::Outcome {
            ack_elicitation: AckElicitation::Eliciting,
            is_congestion_controlled: true,
            bytes_sent: 100,
            packet_number: space.new_packet_number(VarInt::from_u8(1)),
        };
        manager.on_packet_sent(
            space.new_packet_number(VarInt::from_u8(1)),
            outcome,
            s2n_quic_platform::time::now(),
            path::Id::new(0),
            &mut context,
        );

        assert_eq!(context.path().congestion_controller.bytes_in_flight, 100);
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

    fn helper_generate_multi_path_manager(
        space: PacketNumberSpace,
    ) -> (
        SocketAddress,
        path::Id,
        SocketAddress,
        path::Id,
        Manager,
        path::Manager<Endpoint>,
    ) {
        let manager = Manager::new(space, Duration::from_millis(100));
        let clock = NoopClock {};

        let first_addr = SocketAddress::IpV4(Default::default());
        let second_addr: SocketAddr = "127.0.0.1:80".parse().unwrap();
        let second_addr = SocketAddress::from(second_addr);

        let first_path_id = path::Id::new(0);
        let second_path_id = path::Id::new(1);

        // confirm we have one path
        let mut path_manager = helper_generate_path_manager(Duration::from_millis(100));
        {
            assert!(path_manager.path(&first_addr).is_some());
            assert!(path_manager.path(&second_addr).is_none());
        }

        // insert and confirm we have two paths
        {
            let datagram = DatagramInfo {
                timestamp: clock.get_time(),
                remote_address: second_addr,
                payload_len: 0,
                ecn: ExplicitCongestionNotification::default(),
                destination_connection_id: connection::LocalId::TEST_ID,
            };
            let _ = path_manager
                .on_datagram_received(
                    &datagram,
                    &connection::Limits::default(),
                    true,
                    &mut Endpoint::default(),
                    &mut random::testing::Generator(123),
                )
                .unwrap();

            assert!(path_manager.path(&first_addr).is_some());
            assert!(path_manager.path(&second_addr).is_some());
            assert_eq!(path_manager.active_path_id(), first_path_id);
        }

        // start out with both paths validate and not AmplificationLimited
        path_manager.path_mut(&first_addr).unwrap().1.on_validated();
        path_manager
            .path_mut(&second_addr)
            .unwrap()
            .1
            .on_validated();
        let first_path = path_manager.path(&first_addr).unwrap().1;
        let second_path = path_manager.path(&second_addr).unwrap().1;
        assert_eq!(false, first_path.at_amplification_limit());
        assert_eq!(false, second_path.at_amplification_limit());
        assert!(first_path.is_peer_validated());
        assert!(second_path.is_peer_validated());

        (
            first_addr,
            first_path_id,
            second_addr,
            second_path_id,
            manager,
            path_manager,
        )
    }

    fn helper_generate_path_manager(max_ack_delay: Duration) -> path::Manager<Endpoint> {
        let mut random_generator = random::testing::Generator(123);

        let registry = ConnectionIdMapper::new(&mut random_generator, endpoint::Type::Server)
            .create_peer_id_registry(
                InternalConnectionIdGenerator::new().generate_id(),
                connection::PeerId::TEST_ID,
                None,
            );
        let path = Path::new(
            Default::default(),
            connection::PeerId::TEST_ID,
            connection::LocalId::TEST_ID,
            RttEstimator::new(max_ack_delay),
            MockCongestionController::default(),
            true,
        );

        path::Manager::new(path, registry)
    }

    struct MockContext<'a> {
        validate_packet_ack_count: u8,
        on_new_packet_ack_count: u8,
        on_packet_ack_count: u8,
        on_packet_loss_count: u8,
        on_rtt_update_count: u8,
        path_id: path::Id,
        lost_packets: HashSet<PacketNumber>,
        path_manager: &'a mut path::Manager<Endpoint>,
    }

    impl<'a> MockContext<'a> {
        pub fn new(path_manager: &'a mut path::Manager<Endpoint>) -> Self {
            Self {
                validate_packet_ack_count: 0,
                on_new_packet_ack_count: 0,
                on_packet_ack_count: 0,
                on_packet_loss_count: 0,
                on_rtt_update_count: 0,
                path_id: path_manager.active_path_id(),
                lost_packets: HashSet::default(),
                path_manager,
            }
        }
    }

    impl<'a> recovery::Context<MockCongestionController> for MockContext<'a> {
        const ENDPOINT_TYPE: endpoint::Type = endpoint::Type::Client;

        fn is_handshake_confirmed(&self) -> bool {
            true
        }

        fn path(&self) -> &Path<MockCongestionController> {
            &self.path_manager[self.path_id]
        }

        fn path_mut(&mut self) -> &mut Path<MockCongestionController> {
            &mut self.path_manager[self.path_id]
        }

        fn path_by_id(&self, path_id: path::Id) -> &Path<MockCongestionController> {
            &self.path_manager[path_id]
        }

        fn path_mut_by_id(&mut self, path_id: path::Id) -> &mut Path<MockCongestionController> {
            &mut self.path_manager[path_id]
        }

        fn path_id_by_peer_addr(&self, _addr: &SocketAddress) -> Option<path::Id> {
            Some(self.path_id)
        }

        fn validate_packet_ack(
            &mut self,
            _datagram: &DatagramInfo,
            _packet_number_range: &PacketNumberRange,
        ) -> Result<(), transport::Error> {
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

        fn on_packet_loss(&mut self, packet_number_range: &PacketNumberRange) {
            self.on_packet_loss_count += 1;
            self.lost_packets.insert(packet_number_range.start());
        }

        fn on_rtt_update(&mut self) {
            self.on_rtt_update_count += 1;
        }
    }
}

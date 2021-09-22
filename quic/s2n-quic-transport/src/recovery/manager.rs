// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    contexts::WriteContext,
    endpoint,
    path::{self, Path},
    recovery::{SentPacketInfo, SentPackets},
    transmission,
};
use core::{cmp::max, marker::PhantomData, time::Duration};
use s2n_quic_core::{
    event::{self, IntoEvent},
    frame,
    frame::ack::EcnCounts,
    inet::{DatagramInfo, ExplicitCongestionNotification},
    packet::number::{PacketNumber, PacketNumberRange, PacketNumberSpace},
    recovery::{CongestionController, RttEstimator, K_GRANULARITY},
    time::{timer, Timer, Timestamp},
    transport,
};
use smallvec::SmallVec;

type PacketDetails = (PacketNumber, SentPacketInfo);

#[derive(Debug)]
pub struct Manager<Config: endpoint::Config> {
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
    loss_timer: Timer,

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

    // The last processed ECN counts received in an ACK frame. Used to
    // validate new ECN counts and to detect increases in the reported ECN-CE counter.
    baseline_ecn_counts: EcnCounts,

    // The total ecn counts for outstanding (unacknowledged) packets
    sent_packet_ecn_counts: EcnCounts,

    config: PhantomData<Config>,
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
const ACKED_PACKETS_INITIAL_CAPACITY: usize = 32;

impl<Config: endpoint::Config> Manager<Config> {
    /// Constructs a new `recovery::Manager`
    pub fn new(space: PacketNumberSpace, max_ack_delay: Duration) -> Self {
        Self {
            space,
            max_ack_delay,
            largest_acked_packet: None,
            sent_packets: SentPackets::default(),
            loss_timer: Timer::default(),
            pto: Pto::new(max_ack_delay),
            time_of_last_ack_eliciting_packet: None,
            baseline_ecn_counts: EcnCounts::default(),
            sent_packet_ecn_counts: EcnCounts::default(),
            config: PhantomData,
        }
    }

    pub fn on_timeout<Ctx: Context<Config>, Pub: event::ConnectionPublisher>(
        &mut self,
        timestamp: Timestamp,
        context: &mut Ctx,
        publisher: &mut Pub,
    ) {
        if self.loss_timer.is_armed() {
            if self.loss_timer.poll_expiration(timestamp).is_ready() {
                self.detect_and_remove_lost_packets(timestamp, context, publisher);
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

        let path_id = context.path_id().as_u8();
        let path = context.path_mut();
        publisher.on_recovery_metrics(self.recovery_event(path_id, path));
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#A.5
    //# After a packet is sent, information about the packet is stored.
    pub fn on_packet_sent<Ctx: Context<Config>>(
        &mut self,
        packet_number: PacketNumber,
        outcome: transmission::Outcome,
        time_sent: Timestamp,
        ecn: ExplicitCongestionNotification,
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
                context.path_id(),
                ecn,
            ),
        );

        context.path_mut().ecn_controller.on_packet_sent(ecn);
        self.sent_packet_ecn_counts.increment(ecn);

        if outcome.is_congestion_controlled {
            if outcome.ack_elicitation.is_ack_eliciting() {
                self.time_of_last_ack_eliciting_packet = Some(time_sent);
            }
            //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.2.1
            //# A sender SHOULD restart its PTO timer every time an ack-eliciting
            //# packet is sent or acknowledged,
            let is_handshake_confirmed = context.is_handshake_confirmed();
            let path = context.path_mut_by_id(context.path_id());
            self.update_pto_timer(path, time_sent, is_handshake_confirmed);
            path.congestion_controller
                .on_packet_sent(time_sent, congestion_controlled_bytes);
        }
    }

    /// Updates the PTO timer
    pub fn update_pto_timer(
        &mut self,
        path: &Path<Config>,
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

    /// Process ACK frame.
    ///
    /// Update congestion controler, timers and meta data around acked packet ranges.
    pub fn on_ack_frame<
        A: frame::ack::AckRanges,
        Ctx: Context<Config>,
        Pub: event::ConnectionPublisher,
    >(
        &mut self,
        datagram: &DatagramInfo,
        frame: frame::Ack<A>,
        context: &mut Ctx,
        publisher: &mut Pub,
    ) -> Result<(), transport::Error> {
        let largest_acked_packet_number =
            self.space.new_packet_number(frame.largest_acknowledged());
        let mut newly_acked_packets =
            SmallVec::<[SentPacketInfo; ACKED_PACKETS_INITIAL_CAPACITY]>::new();

        // Update the largest acked packet if the largest packet acked in this frame is larger
        let new_largest_packet = if self
            .largest_acked_packet
            .map_or(true, |pn| pn < largest_acked_packet_number)
        {
            self.largest_acked_packet = Some(largest_acked_packet_number);
            true
        } else {
            false
        };

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

            let (_, largest_newly_acked_info) = largest_newly_acked;
            self.process_new_acked_packets(
                &newly_acked_packets,
                largest_newly_acked_info.time_sent,
                new_largest_packet,
                datagram,
                &frame,
                context,
                publisher,
            );
        }

        let path_id = context.path_id().as_u8();
        let path = context.path_mut();
        publisher.on_recovery_metrics(self.recovery_event(path_id, path));

        Ok(())
    }

    // Process ack_range and return largest_newly_acked and if the packet is ack eliciting.
    fn process_ack_range<A: frame::ack::AckRanges, Ctx: Context<Config>>(
        &mut self,
        newly_acked_packets: &mut SmallVec<[SentPacketInfo; ACKED_PACKETS_INITIAL_CAPACITY]>,
        datagram: &DatagramInfo,
        frame: &frame::Ack<A>,
        context: &mut Ctx,
    ) -> Result<(Option<PacketDetails>, bool), transport::Error> {
        let mut largest_newly_acked: Option<PacketDetails> = None;
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

            let mut newly_acked_range: Option<(PacketNumber, PacketNumber)> = None;

            for (packet_number, acked_packet_info) in self.sent_packets.remove_range(acked_packets)
            {
                newly_acked_packets.push(acked_packet_info);

                if largest_newly_acked.map_or(true, |(pn, _)| packet_number > pn) {
                    largest_newly_acked = Some((packet_number, acked_packet_info));
                }

                if let Some((start, end)) = newly_acked_range.as_mut() {
                    debug_assert!(
                        packet_number > *start && packet_number > *end,
                        "remove_range should return packet numbers in ascending order"
                    );
                    *end = packet_number;
                } else {
                    newly_acked_range = Some((packet_number, packet_number));
                };

                includes_ack_eliciting |= acked_packet_info.ack_elicitation.is_ack_eliciting();

                let path = context.path_mut_by_id(acked_packet_info.path_id);
                path.mtu_controller.on_packet_ack(
                    packet_number,
                    acked_packet_info.sent_bytes,
                    &mut path.congestion_controller,
                );
                path.ecn_controller
                    .on_packet_ack(acked_packet_info.time_sent, acked_packet_info.ecn);
            }

            if let Some((start, end)) = newly_acked_range {
                // notify components of packets that are newly acked
                context.on_new_packet_ack(datagram, &PacketNumberRange::new(start, end));
            }
        }

        Ok((largest_newly_acked, includes_ack_eliciting))
    }

    fn update_congestion_control<A: frame::ack::AckRanges, Ctx: Context<Config>>(
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
        let (largest_newly_acked_packet_number, largest_newly_acked_info) = largest_newly_acked;

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#9.4
        //# Packets sent on the old path MUST NOT contribute to
        //# congestion control or RTT estimation for the new path.
        should_update_rtt &= context.path_id() == largest_newly_acked_info.path_id;

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
            let path = context.path_mut_by_id(largest_newly_acked_info.path_id);
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
    }

    #[allow(clippy::too_many_arguments)]
    fn process_new_acked_packets<
        A: frame::ack::AckRanges,
        Ctx: Context<Config>,
        Pub: event::ConnectionPublisher,
    >(
        &mut self,
        newly_acked_packets: &SmallVec<[SentPacketInfo; ACKED_PACKETS_INITIAL_CAPACITY]>,
        largest_newly_acked_time_sent: Timestamp,
        new_largest_packet: bool,
        datagram: &DatagramInfo,
        frame: &frame::Ack<A>,
        context: &mut Ctx,
        publisher: &mut Pub,
    ) {
        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.1.2
        //# Once a later packet within the same packet number space has been
        //# acknowledged, an endpoint SHOULD declare an earlier packet lost if it
        //# was sent a threshold amount of time in the past.
        self.detect_and_remove_lost_packets(datagram.timestamp, context, publisher);

        let current_path_id = context.path_id();
        let is_handshake_confirmed = context.is_handshake_confirmed();
        let mut current_path_acked_bytes = 0;
        let mut newly_acked_ecn_counts = EcnCounts::default();

        for acked_packet_info in newly_acked_packets {
            let path = context.path_mut_by_id(acked_packet_info.path_id);

            let sent_bytes = acked_packet_info.sent_bytes as usize;
            newly_acked_ecn_counts.increment(acked_packet_info.ecn);

            if acked_packet_info.path_id == current_path_id {
                current_path_acked_bytes += sent_bytes;
            } else {
                path.congestion_controller.on_packet_ack(
                    largest_newly_acked_time_sent,
                    sent_bytes,
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
            if path.is_peer_validated() {
                path.reset_pto_backoff();
            }

            if acked_packet_info.path_id != current_path_id {
                self.update_pto_timer(path, datagram.timestamp, is_handshake_confirmed);
            }
        }

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.2.1
        //# A sender SHOULD restart its PTO timer every time an ack-eliciting
        //# packet is sent or acknowledged,
        debug_assert!(
            !newly_acked_packets.is_empty(),
            "this method assumes there was at least one newly-acked packet"
        );
        let path = context.path_mut();

        //= https://www.rfc-editor.org/rfc/rfc9000.txt#13.4.2.1
        //# Validating ECN counts from reordered ACK frames can result in failure.
        //# An endpoint MUST NOT fail ECN validation as a result of processing an
        //# ACK frame that does not increase the largest acknowledged packet number.
        if new_largest_packet {
            self.process_ecn(newly_acked_ecn_counts, frame.ecn_counts, datagram, path);
        }

        if current_path_acked_bytes > 0 {
            path.congestion_controller.on_packet_ack(
                largest_newly_acked_time_sent,
                current_path_acked_bytes,
                &path.rtt_estimator,
                datagram.timestamp,
            );

            self.update_pto_timer(path, datagram.timestamp, is_handshake_confirmed);
        }
    }

    fn process_ecn(
        &mut self,
        newly_acked_ecn_counts: EcnCounts,
        ack_frame_ecn_counts: Option<EcnCounts>,
        datagram: &DatagramInfo,
        path: &mut Path<Config>,
    ) {
        path.ecn_controller.validate(
            newly_acked_ecn_counts,
            self.sent_packet_ecn_counts,
            self.baseline_ecn_counts,
            ack_frame_ecn_counts,
            datagram.timestamp,
        );

        if let Some(ack_frame_ecn_counts) = ack_frame_ecn_counts {
            if path.ecn_controller.is_capable()
                && ack_frame_ecn_counts.ce_count > self.baseline_ecn_counts.ce_count
            {
                //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#7.1
                //# If a path has been validated to support ECN ([RFC3168], [RFC8311]),
                //# QUIC treats a Congestion Experienced (CE) codepoint in the IP header
                //# as a signal of congestion.
                path.congestion_controller
                    .on_congestion_event(datagram.timestamp);
            }
        }

        self.baseline_ecn_counts = ack_frame_ecn_counts.unwrap_or_default();
        self.sent_packet_ecn_counts -= newly_acked_ecn_counts;
    }

    /// Returns `true` if the recovery manager requires a probe packet to be sent.
    #[inline]
    pub fn requires_probe(&self) -> bool {
        matches!(self.pto.state, PtoState::RequiresTransmission(_))
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#B.9
    //# When Initial or Handshake keys are discarded, packets sent in that
    //# space no longer count toward bytes in flight.
    /// Clears bytes in flight for sent packets.
    pub fn on_packet_number_space_discarded<Pub: event::ConnectionPublisher>(
        &mut self,
        path: &mut Path<Config>,
        path_id: path::Id,
        publisher: &mut Pub,
    ) {
        debug_assert_ne!(self.space, PacketNumberSpace::ApplicationData);

        publisher.on_recovery_metrics(self.recovery_event(path_id.as_u8(), path));

        // Remove any unacknowledged packets from flight.
        let mut discarded_bytes = 0;
        for (_, unacked_sent_info) in self.sent_packets.iter() {
            debug_assert_eq!(
                unacked_sent_info.path_id,
                path_id,
                "this implementation assumes the connection has a single path when discarding packets"
            );
            discarded_bytes += unacked_sent_info.sent_bytes as usize;
        }
        path.congestion_controller
            .on_packet_discarded(discarded_bytes);
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#A.10
    //# DetectAndRemoveLostPackets is called every time an ACK is received or the time threshold
    //# loss detection timer expires. This function operates on the sent_packets for that packet
    //# number space and returns a list of packets newly detected as lost.
    fn detect_and_remove_lost_packets<Ctx: Context<Config>, Pub: event::ConnectionPublisher>(
        &mut self,
        now: Timestamp,
        context: &mut Ctx,
        publisher: &mut Pub,
    ) {
        // Cancel the loss timer. It will be armed again if any unacknowledged packets are
        // older than the largest acked packet, but not old enough to be considered lost yet
        self.loss_timer.cancel();

        // TODO: Investigate a more efficient mechanism for managing sent_packets
        //       See https://github.com/awslabs/s2n-quic/issues/69
        let (max_persistent_congestion_period, sent_packets_to_remove) =
            self.detect_lost_packets(now, context);

        self.remove_lost_packets(
            now,
            max_persistent_congestion_period,
            sent_packets_to_remove,
            context,
            publisher,
        );
    }

    fn detect_lost_packets<Ctx: Context<Config>>(
        &mut self,
        now: Timestamp,
        context: &mut Ctx,
    ) -> (Duration, Vec<PacketDetails>) {
        let largest_acked_packet = self
            .largest_acked_packet
            .expect("This function is only called after an ack has been received");

        let mut max_persistent_congestion_period = Duration::from_secs(0);
        let mut sent_packets_to_remove = Vec::new();
        let mut persistent_congestion_period = Duration::from_secs(0);
        let mut prev_packet: Option<(PacketNumber, path::Id, Timestamp)> = None;

        for (unacked_packet_number, unacked_sent_info) in self.sent_packets.iter() {
            if unacked_packet_number > largest_acked_packet {
                // sent_packets is ordered by packet number, so all remaining packets will be larger
                break;
            }

            let unacked_path_id = unacked_sent_info.path_id;
            let path = &context.path_by_id(unacked_path_id);
            // Calculate how long we wait until a packet is declared lost
            let time_threshold = Self::calculate_loss_time_threshold(&path.rtt_estimator);
            // Calculate at what time this particular packet is considered lost based on the
            // current path `time_threshold`
            let packet_lost_time = unacked_sent_info.time_sent + time_threshold;

            // If the `packet_lost_time` exceeds the current time, it's lost
            let time_threshold_exceeded = packet_lost_time.has_elapsed(now);

            let packet_number_threshold_exceeded = largest_acked_packet
                .checked_distance(unacked_packet_number)
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
                sent_packets_to_remove.push((unacked_packet_number, *unacked_sent_info));

                if unacked_sent_info.congestion_controlled {
                    // The packet is "in-flight", ie congestion controlled
                    // TODO merge contiguous packet numbers
                    let range =
                        PacketNumberRange::new(unacked_packet_number, unacked_packet_number);
                    context.on_packet_loss(&range);
                }

                //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#7.6.2
                //# A sender establishes persistent congestion on receiving an
                //# acknowledgement if at least two ack-eliciting packets are declared
                //# lost, and:
                //#
                //# *  all packets, across all packet number spaces, sent between these
                //#    two send times are declared lost;
                let is_contiguous = prev_packet.map_or(false, |(pn, prev_path_id, _)| {
                    // Check if this lost packet is contiguous with the previous lost packet.
                    let contiguous = unacked_packet_number.checked_distance(pn) == Some(1);
                    contiguous
                        // Check that this lost packet was sent on this path
                        //
                        // persistent congestion is only updated for the path on which we receive
                        // the ack. Managing state for multiple paths requires extra allocations
                        // but is only necessary when also attempting connection_migration; which
                        // should not be very common.
                        && unacked_path_id == context.path_id()
                        // Check that previous packet was sent on this path
                        && prev_path_id == context.path_id()
                });
                if is_contiguous {
                    // Add the difference in time to the current period.
                    persistent_congestion_period +=
                        unacked_sent_info.time_sent - prev_packet.expect("checked above").2;
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
                self.loss_timer.set(packet_lost_time);
                debug_assert!(
                    !self.loss_timer.is_expired(now),
                    "loss timer was not armed in the future; now: {}, threshold: {:?}\nmanager: {:#?}",
                    now,
                    time_threshold,
                    self
                );

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
                prev_packet = Some((
                    unacked_packet_number,
                    unacked_sent_info.path_id,
                    unacked_sent_info.time_sent,
                ));
            }
        }

        (max_persistent_congestion_period, sent_packets_to_remove)
    }

    fn remove_lost_packets<Ctx: Context<Config>, Pub: event::ConnectionPublisher>(
        &mut self,
        now: Timestamp,
        max_persistent_congestion_period: Duration,
        sent_packets_to_remove: Vec<PacketDetails>,
        context: &mut Ctx,
        publisher: &mut Pub,
    ) {
        let current_path_id = context.path_id();

        // Remove the lost packets and account for the bytes on the proper congestion controller
        for (packet_number, sent_info) in sent_packets_to_remove {
            let path = context.path_mut_by_id(sent_info.path_id);

            self.sent_packets.remove(packet_number);

            //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#7.6.2
            //# A sender that does not have state for all packet
            //# number spaces or an implementation that cannot compare send times
            //# across packet number spaces MAY use state for just the packet number
            //# space that was acknowledged.
            let persistent_congestion = max_persistent_congestion_period
                > path.rtt_estimator.persistent_congestion_threshold()
                // Check that the packet was sent on this path
                && sent_info.path_id == current_path_id;

            let mut is_mtu_probe = false;
            if sent_info.sent_bytes as usize > path.mtu_controller.mtu() {
                //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#14.4
                //# Loss of a QUIC packet that is carried in a PMTU probe is therefore not a
                //# reliable indication of congestion and SHOULD NOT trigger a congestion
                //# control reaction; see Section 3, Bullet 7 of [DPLPMTUD].

                //= https://tools.ietf.org/rfc/rfc8899.txt#3
                //# Loss of a probe packet SHOULD NOT be treated as an
                //# indication of congestion and SHOULD NOT trigger a congestion
                //# control reaction [RFC4821] because this could result in
                //# unnecessary reduction of the sending rate.
                path.congestion_controller
                    .on_packet_discarded(sent_info.sent_bytes as usize);
                is_mtu_probe = true;
            } else if sent_info.sent_bytes > 0 {
                path.congestion_controller.on_packets_lost(
                    sent_info.sent_bytes as u32,
                    persistent_congestion,
                    now,
                );
                is_mtu_probe = false;
            }

            publisher.on_packet_lost(event::builder::PacketLost {
                packet_header: event::builder::PacketHeader {
                    packet_type: packet_number.into_event(),
                    version: Some(publisher.quic_version()),
                },
                path: event::builder::Path {
                    local_addr: path.local_address().into_event(),
                    local_cid: path.local_connection_id.into_event(),
                    remote_addr: path.remote_address().into_event(),
                    remote_cid: path.peer_connection_id.into_event(),
                    id: current_path_id.into_event(),
                },
                bytes_lost: sent_info.sent_bytes,
                is_mtu_probe,
            });

            // Notify the MTU controller of packet loss even if it wasn't a probe since it uses
            // that information for blackhole detection.
            path.mtu_controller.on_packet_loss(
                packet_number,
                sent_info.sent_bytes,
                now,
                &mut path.congestion_controller,
            );

            // Notify the ECN controller of packet loss for blackhole detection.
            path.ecn_controller
                .on_packet_loss(sent_info.time_sent, sent_info.ecn, now);

            if persistent_congestion {
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

    fn recovery_event(&self, path_id: u8, path: &Path<Config>) -> event::builder::RecoveryMetrics {
        event::builder::RecoveryMetrics {
            path_id: path_id as u64,
            min_rtt: path.rtt_estimator.min_rtt(),
            smoothed_rtt: path.rtt_estimator.smoothed_rtt(),
            latest_rtt: path.rtt_estimator.latest_rtt(),
            rtt_variance: path.rtt_estimator.rttvar(),
            max_ack_delay: path.rtt_estimator.max_ack_delay(),
            pto_count: (path.pto_backoff as f32).log2() as u32,
            congestion_window: path.congestion_controller.congestion_window(),
            bytes_in_flight: path.congestion_controller.bytes_in_flight(),
        }
    }
}

impl<Config: endpoint::Config> timer::Provider for Manager<Config> {
    #[inline]
    fn timers<Q: timer::Query>(&self, query: &mut Q) -> timer::Result {
        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.2.1
        //# The PTO timer MUST NOT be set if a timer is set for time threshold
        //# loss detection; see Section 6.1.2.  A timer that is set for time
        //# threshold loss detection will expire earlier than the PTO timer in
        //# most cases and is less likely to spuriously retransmit data.

        if self.loss_timer.is_armed() {
            self.loss_timer.timers(query)?;
        } else {
            self.pto.timers(query)?;
        }

        Ok(())
    }
}

pub trait Context<Config: endpoint::Config> {
    const ENDPOINT_TYPE: endpoint::Type;

    fn is_handshake_confirmed(&self) -> bool;

    fn path(&self) -> &Path<Config>;

    fn path_mut(&mut self) -> &mut Path<Config>;

    fn path_by_id(&self, path_id: path::Id) -> &path::Path<Config>;

    fn path_mut_by_id(&mut self, path_id: path::Id) -> &mut path::Path<Config>;

    fn path_id(&self) -> path::Id;

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

impl<Config: endpoint::Config> transmission::interest::Provider for Manager<Config> {
    fn transmission_interest<Q: transmission::interest::Query>(
        &self,
        query: &mut Q,
    ) -> transmission::interest::Result {
        self.pto.transmission_interest(query)
    }
}

/// Manages the probe time out calculation and probe packet transmission
#[derive(Debug, Default)]
struct Pto {
    timer: Timer,
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
        if !context.transmission_mode().is_loss_recovery_probing() {
            // If we aren't currently in loss recovery probing mode, don't
            // send a probe. We could be in this state even if PtoState is
            // RequiresTransmission if we are just transmitting a ConnectionClose
            // frame.
            return;
        }

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

impl timer::Provider for Pto {
    #[inline]
    fn timers<Q: timer::Query>(&self, query: &mut Q) -> timer::Result {
        self.timer.timers(query)?;
        Ok(())
    }
}

impl transmission::interest::Provider for Pto {
    #[inline]
    fn transmission_interest<Q: transmission::interest::Query>(
        &self,
        query: &mut Q,
    ) -> transmission::interest::Result {
        if matches!(self.state, PtoState::RequiresTransmission(_)) {
            query.on_forced()?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests;

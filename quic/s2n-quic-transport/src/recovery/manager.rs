// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    contexts::WriteContext,
    endpoint,
    path::{self, ecn::ValidationOutcome, path_event, Path},
    recovery::{SentPacketInfo, SentPackets},
    transmission::{self, interest::Provider as _, Provider as _},
};
use core::time::Duration;
use s2n_quic_core::{
    event::{self, builder::CongestionSource, IntoEvent},
    frame,
    frame::ack::EcnCounts,
    inet::ExplicitCongestionNotification,
    packet::number::{PacketNumber, PacketNumberRange, PacketNumberSpace},
    recovery::{congestion_controller, persistent_congestion, CongestionController, Pto},
    time::{timer, timer::Provider, Timer, Timestamp},
    transport,
};
use smallvec::SmallVec;

#[cfg(test)]
mod tests;

type PacketDetails<PacketInfo> = (PacketNumber, SentPacketInfo<PacketInfo>);

#[derive(Debug)]
pub struct Manager<Config: endpoint::Config> {
    // The packet space for this recovery manager
    space: PacketNumberSpace,

    //= https://www.rfc-editor.org/rfc/rfc9002#appendix-A.3
    //# The largest packet number acknowledged in the packet number space so far.
    largest_acked_packet: Option<PacketNumber>,

    //= https://www.rfc-editor.org/rfc/rfc9002#appendix-A.3
    //# An association of packet numbers in a packet number space to information about them.
    //  These are packets that are pending acknowledgement.
    sent_packets: SentPackets<<<Config::CongestionControllerEndpoint as congestion_controller::Endpoint>::CongestionController as congestion_controller::CongestionController>::PacketInfo>,

    // Timer set when packets may be declared lost at a time in the future
    loss_timer: Timer,

    //= https://www.rfc-editor.org/rfc/rfc9002#section-6.2
    //# A Probe Timeout (PTO) triggers the sending of one or two probe
    //# datagrams when ack-eliciting packets are not acknowledged within the
    //# expected period of time or the server may not have validated the
    //# client's address.  A PTO enables a connection to recover from loss of
    //# tail packets or acknowledgments.
    pto: Pto,

    //= https://www.rfc-editor.org/rfc/rfc9002#appendix-A.3
    //# The time the most recent ack-eliciting packet was sent.
    time_of_last_ack_eliciting_packet: Option<Timestamp>,

    // The last processed ECN counts received in an ACK frame. Used to
    // validate new ECN counts and to detect increases in the reported ECN-CE counter.
    baseline_ecn_counts: EcnCounts,

    // The total ecn counts for outstanding (unacknowledged) packets
    sent_packet_ecn_counts: EcnCounts,

    // An update to the PTO timer is needed.
    //
    // Used for updating the PTO timer at the end of a transmission burst.
    pto_update_pending: bool,
}

/// Initial capacity of the SmallVec used for keeping track of packets
/// acked in an ack frame
// TODO: Determine if there is a more appropriate default
const ACKED_PACKETS_INITIAL_CAPACITY: usize = 32;

macro_rules! recovery_event {
    ($path_id:ident, $path:ident) => {
        event::builder::RecoveryMetrics {
            path: event::builder::Path {
                local_addr: $path.local_address().into_event(),
                local_cid: $path.local_connection_id.into_event(),
                remote_addr: $path.remote_address().into_event(),
                remote_cid: $path.peer_connection_id.into_event(),
                id: $path_id as u64,
                is_active: $path.is_active(),
            },
            min_rtt: $path.rtt_estimator.min_rtt(),
            smoothed_rtt: $path.rtt_estimator.smoothed_rtt(),
            latest_rtt: $path.rtt_estimator.latest_rtt(),
            rtt_variance: $path.rtt_estimator.rttvar(),
            max_ack_delay: $path.rtt_estimator.max_ack_delay(),
            pto_count: ($path.pto_backoff as f32).log2() as u32,
            congestion_window: $path.congestion_controller.congestion_window(),
            bytes_in_flight: $path.congestion_controller.bytes_in_flight(),
            congestion_limited: $path.transmission_constraint().is_congestion_limited(),
        }
    };
}

pub(crate) use recovery_event;
use s2n_quic_core::{path::mtu::MtuResult, recovery::loss};

// Since `SentPacketInfo` is generic over a type supplied by the Congestion Controller implementation,
// the type definition is particularly lengthy, especially since rust requires the fully-qualified
// syntax to eliminate ambiguity. This macro can be used where ever the Congestion Controller
// generic PacketInfo type is required to help with readability.
macro_rules! packet_info_type {
    () => {
        <<Config::CongestionControllerEndpoint as congestion_controller::Endpoint>::CongestionController as congestion_controller::CongestionController>::PacketInfo
    }
}

#[allow(clippy::type_complexity)]
impl<Config: endpoint::Config> Manager<Config> {
    /// Constructs a new `recovery::Manager`
    pub fn new(space: PacketNumberSpace) -> Self {
        Self {
            space,
            largest_acked_packet: None,
            sent_packets: SentPackets::default(),
            loss_timer: Timer::default(),
            pto: Pto::default(),
            time_of_last_ack_eliciting_packet: None,
            baseline_ecn_counts: EcnCounts::default(),
            sent_packet_ecn_counts: EcnCounts::default(),
            pto_update_pending: false,
        }
    }

    /// Invoked when the Client processes a Retry packet.
    ///
    /// Reset congestion controller state by discarding sent bytes and replacing recovery
    /// manager with a new instance of itself.
    pub fn on_retry_packet<Pub: event::ConnectionPublisher>(
        &mut self,
        path: &mut Path<Config>,
        path_id: path::Id,
        publisher: &mut Pub,
    ) {
        debug_assert!(
            Config::ENDPOINT_TYPE.is_client(),
            "only a Client should process a Retry packet"
        );

        let mut discarded_bytes = 0;
        for (_, unacked_sent_info) in self.sent_packets.iter() {
            discarded_bytes += unacked_sent_info.sent_bytes as usize;
        }
        path.congestion_controller.on_packet_discarded(
            discarded_bytes,
            &mut congestion_controller::PathPublisher::new(publisher, path_id),
        );

        *self = Self::new(self.space);
    }

    pub fn on_timeout<Ctx: Context<Config>, Pub: event::ConnectionPublisher>(
        &mut self,
        timestamp: Timestamp,
        random_generator: &mut Config::RandomGenerator,
        max_pto_backoff: u32,
        context: &mut Ctx,
        publisher: &mut Pub,
    ) {
        debug_assert!(!self.pto_update_pending);

        if self.loss_timer.is_armed() {
            if self.loss_timer.poll_expiration(timestamp).is_ready() {
                self.detect_and_remove_lost_packets(
                    timestamp,
                    random_generator,
                    context,
                    publisher,
                );
                self.update_pto_timer(
                    context.active_path(),
                    timestamp,
                    context.is_handshake_confirmed(),
                    random_generator,
                );
            }
        } else {
            let pto_expired = self
                .pto
                .on_timeout(!self.sent_packets.is_empty(), timestamp)
                .is_ready();

            //= https://www.rfc-editor.org/rfc/rfc9002#section-6.2
            //# A PTO timer expiration event does not indicate packet loss and MUST
            //# NOT cause prior unacknowledged packets to be marked as lost.

            //= https://www.rfc-editor.org/rfc/rfc9002#section-6.2.1
            //# When a PTO timer expires, the PTO backoff MUST be increased,
            //# resulting in the PTO period being set to twice its current value.
            if pto_expired {
                context.active_path_mut().pto_backoff =
                    (context.active_path().pto_backoff * 2).min(max_pto_backoff);
                self.update_pto_timer(
                    context.active_path(),
                    timestamp,
                    context.is_handshake_confirmed(),
                    random_generator,
                );
            }
        }

        self.check_consistency(context.active_path(), context.is_handshake_confirmed());

        let path_id = context.path_id().as_u8();
        let path = context.path_mut();
        publisher.on_recovery_metrics(recovery_event!(path_id, path));
    }

    //= https://www.rfc-editor.org/rfc/rfc9002#appendix-A.5
    //# After a packet is sent, information about the packet is stored.
    pub fn on_packet_sent<Ctx: Context<Config>, Pub: event::ConnectionPublisher>(
        &mut self,
        packet_number: PacketNumber,
        outcome: transmission::Outcome,
        time_sent: Timestamp,
        ecn: ExplicitCongestionNotification,
        transmission_mode: transmission::Mode,
        app_limited: Option<bool>,
        context: &mut Ctx,
        publisher: &mut Pub,
    ) {
        //= https://www.rfc-editor.org/rfc/rfc9002#section-7
        //# Similar to TCP, packets containing only ACK frames do not count
        //# towards bytes in flight and are not congestion controlled.

        // Everything else (including probe packets) are counted, as specified below:
        //= https://www.rfc-editor.org/rfc/rfc9002#section-7.5
        //# A sender MUST however count these packets as being additionally in
        //# flight, since these packets add network load without establishing
        //# packet loss.
        let congestion_controlled_bytes = if outcome.is_congestion_controlled {
            outcome.bytes_sent
        } else {
            0
        };

        let path_id = context.path_id();
        let path = context.path_mut();
        let cc_packet_info = path.congestion_controller.on_packet_sent(
            time_sent,
            congestion_controlled_bytes,
            app_limited,
            &path.rtt_estimator,
            &mut congestion_controller::PathPublisher::new(publisher, path_id),
        );

        self.sent_packets.insert(
            packet_number,
            SentPacketInfo::new(
                outcome.is_congestion_controlled,
                congestion_controlled_bytes,
                time_sent,
                outcome.ack_elicitation,
                path_id,
                ecn,
                transmission_mode,
                cc_packet_info,
            ),
        );
        path.ecn_controller
            .on_packet_sent(ecn, path_event!(path, path_id), publisher);
        self.sent_packet_ecn_counts.increment(ecn);

        if outcome.ack_elicitation.is_ack_eliciting() {
            self.time_of_last_ack_eliciting_packet = Some(time_sent);
            //= https://www.rfc-editor.org/rfc/rfc9002#section-6.2.1
            //# A sender SHOULD restart its PTO timer every time an ack-eliciting
            //# packet is sent or acknowledged,
            self.pto_update_pending = true;
        }
    }

    /// Invoked after a burst of packets has completed transmitting
    pub fn on_transmit_burst_complete(
        &mut self,
        active_path: &Path<Config>,
        now: Timestamp,
        is_handshake_confirmed: bool,
        random_generator: &mut Config::RandomGenerator,
    ) {
        debug_assert!(active_path.is_active());
        if self.pto_update_pending {
            // Update the PTO timer once per transmission burst to reduce CPU cost
            self.update_pto_timer(active_path, now, is_handshake_confirmed, random_generator);
            debug_assert!(!self.pto_update_pending);
        }
        self.check_consistency(active_path, is_handshake_confirmed);
    }

    /// Updates the PTO timer
    pub fn update_pto_timer(
        &mut self,
        active_path: &Path<Config>,
        now: Timestamp,
        is_handshake_confirmed: bool,
        random_generator: &mut Config::RandomGenerator,
    ) {
        self.pto_update_pending = false;

        debug_assert!(active_path.is_active());

        (|| {
            if self.loss_timer.is_armed() {
                //= https://www.rfc-editor.org/rfc/rfc9002#section-6.2.1
                //# The PTO timer MUST NOT be set if a timer is set for time threshold
                //# loss detection; see Section 6.1.2.  A timer that is set for time
                //# threshold loss detection will expire earlier than the PTO timer in
                //# most cases and is less likely to spuriously retransmit data.
                self.pto.cancel();
                return;
            }

            //= https://www.rfc-editor.org/rfc/rfc9002#section-6.2.2.1
            //# If no additional data can be sent, the server's PTO timer MUST NOT be
            //# armed until datagrams have been received from the client, because
            //# packets sent on PTO count against the anti-amplification limit.
            if active_path.at_amplification_limit() {
                // The server's timer is not set if nothing can be sent.
                self.pto.cancel();
                return;
            }

            //= https://www.rfc-editor.org/rfc/rfc9002#section-6.2.1
            //# An endpoint MUST NOT set its PTO timer for the Application Data
            //# packet number space until the handshake is confirmed.
            if self.space.is_application_data() && !is_handshake_confirmed {
                self.pto.cancel();
                return;
            }

            let ack_eliciting_packets_in_flight = self
                .sent_packets
                .iter()
                .any(|(_, sent_info)| sent_info.ack_elicitation.is_ack_eliciting());

            //= https://www.rfc-editor.org/rfc/rfc9002#section-6.2.2.1
            //# it is the client's responsibility to send packets to unblock the server
            //# until it is certain that the server has finished its address validation
            if !ack_eliciting_packets_in_flight && active_path.is_peer_validated() {
                // There is nothing to detect lost, so no timer is set.
                // However, the client needs to arm the timer if the
                // server might be blocked by the anti-amplification limit.
                self.pto.cancel();
                return;
            }

            let pto_base_timestamp = if ack_eliciting_packets_in_flight {
                self.time_of_last_ack_eliciting_packet
                    .expect("there is at least one ack eliciting packet in flight")
            } else {
                // Arm PTO from now when there are no inflight packets.
                //= https://www.rfc-editor.org/rfc/rfc9002#section-6.2.2.1
                //# That is,
                //# the client MUST set the PTO timer if the client has not received an
                //# acknowledgment for any of its Handshake packets and the handshake is
                //# not confirmed (see Section 4.1.2 of [QUIC-TLS]), even if there are no
                //# packets in flight.
                now
            };

            self.pto
                .update(pto_base_timestamp, active_path.pto_period_with_jitter(self.space, random_generator));
        })();

        self.check_consistency(active_path, is_handshake_confirmed);
    }

    /// Queries the component for any outgoing frames that need to get sent
    pub fn on_transmit<W: WriteContext>(&mut self, context: &mut W) {
        self.pto.on_transmit(context)
    }

    /// Process ACK frame.
    ///
    /// Update congestion controller, timers and meta data around acked packet ranges.
    pub fn on_ack_frame<
        A: frame::ack::AckRanges,
        Ctx: Context<Config>,
        Pub: event::ConnectionPublisher,
    >(
        &mut self,
        timestamp: Timestamp,
        frame: frame::Ack<A>,
        packet_number: PacketNumber,
        random_generator: &mut Config::RandomGenerator,
        context: &mut Ctx,
        publisher: &mut Pub,
    ) -> Result<(), transport::Error> {
        let space = self.space;
        let largest_acked_packet_number = space.new_packet_number(frame.largest_acknowledged());

        self.process_acks(
            timestamp,
            frame.ack_ranges().map(|ack_range| {
                let (start, end) = ack_range.into_inner();
                PacketNumberRange::new(space.new_packet_number(start), space.new_packet_number(end))
            }),
            largest_acked_packet_number,
            frame.ack_delay(),
            frame.ecn_counts,
            packet_number,
            random_generator,
            context,
            publisher,
        )?;

        self.check_consistency(context.active_path(), context.is_handshake_confirmed());

        Ok(())
    }

    /// Generic interface for processing ACK ranges.
    fn process_acks<Ctx: Context<Config>, Pub: event::ConnectionPublisher>(
        &mut self,
        timestamp: Timestamp,
        ranges: impl Iterator<Item = PacketNumberRange>,
        largest_acked_packet_number: PacketNumber,
        ack_delay: Duration,
        ecn_counts: Option<EcnCounts>,
        packet_number: PacketNumber,
        random_generator: &mut Config::RandomGenerator,
        context: &mut Ctx,
        publisher: &mut Pub,
    ) -> Result<(), transport::Error> {
        let mut newly_acked_packets =
            SmallVec::<[PacketDetails<packet_info_type!()>; ACKED_PACKETS_INITIAL_CAPACITY]>::new();
        let (largest_newly_acked, includes_ack_eliciting) = self.process_ack_range(
            &mut newly_acked_packets,
            timestamp,
            packet_number,
            ranges,
            context,
            publisher,
        )?;

        // Update the largest acked packet if the largest packet acked in this frame is larger
        let acked_new_largest_packet = match self.largest_acked_packet {
            Some(current_largest) if current_largest > largest_acked_packet_number => false,
            _ => {
                self.largest_acked_packet = Some(largest_acked_packet_number);
                true
            }
        };

        //= https://www.rfc-editor.org/rfc/rfc9002#section-5.1
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
                timestamp,
                ack_delay,
                context,
                publisher,
            );

            self.process_new_acked_packets(
                &newly_acked_packets,
                acked_new_largest_packet,
                timestamp,
                ecn_counts,
                random_generator,
                context,
                publisher,
            );
        }

        let path_id = context.path_id().as_u8();
        let path = context.path_mut();
        publisher.on_recovery_metrics(recovery_event!(path_id, path));

        Ok(())
    }

    // Process ack_range and return largest_newly_acked and if the packet is ack eliciting.
    fn process_ack_range<Ctx: Context<Config>, Pub: event::ConnectionPublisher>(
        &mut self,
        newly_acked_packets: &mut SmallVec<
            [PacketDetails<packet_info_type!()>; ACKED_PACKETS_INITIAL_CAPACITY],
        >,
        timestamp: Timestamp,
        packet_number: PacketNumber,
        ranges: impl Iterator<Item = PacketNumberRange>,
        context: &mut Ctx,
        publisher: &mut Pub,
    ) -> Result<(Option<PacketDetails<packet_info_type!()>>, bool), transport::Error> {
        let mut largest_newly_acked: Option<PacketDetails<packet_info_type!()>> = None;
        let mut includes_ack_eliciting = false;

        for pn_range in ranges {
            // The path the ack was received on
            let rx_path_id = context.path_id();
            let rx_path = context.path_mut();
            publisher.on_ack_range_received(event::builder::AckRangeReceived {
                packet_header: event::builder::PacketHeader::new(
                    packet_number,
                    publisher.quic_version(),
                ),
                path: path_event!(rx_path, rx_path_id),
                ack_range: pn_range.into_event(),
            });

            context.validate_packet_ack(
                timestamp,
                &pn_range,
                self.sent_packets.get_range().start(),
            )?;
            // notify components of packets acked
            context.on_packet_ack(timestamp, &pn_range);

            let mut newly_acked_range: Option<(PacketNumber, PacketNumber)> = None;

            for (packet_number, acked_packet_info) in self.sent_packets.remove_range(pn_range) {
                newly_acked_packets.push((packet_number, acked_packet_info));

                if largest_newly_acked.is_none_or(|(pn, _)| packet_number > pn) {
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
                path.ecn_controller
                    .on_packet_ack(acked_packet_info.time_sent, acked_packet_info.ecn);
                match path.mtu_controller.on_packet_ack(
                    packet_number,
                    acked_packet_info.sent_bytes,
                    &mut path.congestion_controller,
                    acked_packet_info.path_id,
                    publisher,
                ) {
                    MtuResult::MtuUpdated(max_datagram_size) => {
                        context.on_mtu_update(max_datagram_size)
                    }
                    MtuResult::NoChange => {}
                }
            }

            if let Some((start, end)) = newly_acked_range {
                // notify components of packets that are newly acked
                context.on_new_packet_ack(&PacketNumberRange::new(start, end), publisher);
            }
        }

        Ok((largest_newly_acked, includes_ack_eliciting))
    }

    fn update_congestion_control<Ctx: Context<Config>, Pub: event::ConnectionPublisher>(
        &mut self,
        largest_newly_acked: PacketDetails<packet_info_type!()>,
        largest_acked_packet_number: PacketNumber,
        includes_ack_eliciting: bool,
        timestamp: Timestamp,
        ack_delay: Duration,
        context: &mut Ctx,
        publisher: &mut Pub,
    ) {
        let mut should_update_rtt = true;
        let is_handshake_confirmed = context.is_handshake_confirmed();
        let (largest_newly_acked_packet_number, largest_newly_acked_info) = largest_newly_acked;

        //= https://www.rfc-editor.org/rfc/rfc9000#section-9.4
        //# Packets sent on the old path MUST NOT contribute to
        //# congestion control or RTT estimation for the new path.
        should_update_rtt &= context.path_id() == largest_newly_acked_info.path_id;

        //= https://www.rfc-editor.org/rfc/rfc9002#section-5.1
        //# To avoid generating multiple RTT samples for a single packet, an ACK
        //# frame SHOULD NOT be used to update RTT estimates if it does not newly
        //# acknowledge the largest acknowledged packet.
        should_update_rtt &= largest_newly_acked_packet_number == largest_acked_packet_number;

        //= https://www.rfc-editor.org/rfc/rfc9002#section-5.1
        //# An RTT sample MUST NOT be generated on receiving an ACK frame that
        //# does not newly acknowledge at least one ack-eliciting packet.
        should_update_rtt &= includes_ack_eliciting;

        if should_update_rtt {
            let latest_rtt = timestamp - largest_newly_acked_info.time_sent;
            let path = context.path_mut_by_id(largest_newly_acked_info.path_id);
            path.rtt_estimator.update_rtt(
                ack_delay,
                latest_rtt,
                timestamp,
                is_handshake_confirmed,
                largest_acked_packet_number.space(),
            );

            // Update the congestion controller with the latest RTT estimate
            path.congestion_controller.on_rtt_update(
                largest_newly_acked_info.time_sent,
                timestamp,
                &path.rtt_estimator,
                &mut congestion_controller::PathPublisher::new(
                    publisher,
                    largest_newly_acked_info.path_id,
                ),
            );

            // Notify components the RTT estimate was updated
            context.on_rtt_update(timestamp);
        }
    }

    fn process_new_acked_packets<Ctx: Context<Config>, Pub: event::ConnectionPublisher>(
        &mut self,
        newly_acked_packets: &SmallVec<
            [PacketDetails<packet_info_type!()>; ACKED_PACKETS_INITIAL_CAPACITY],
        >,
        new_largest_packet: bool,
        timestamp: Timestamp,
        ecn_counts: Option<EcnCounts>,
        random_generator: &mut Config::RandomGenerator,
        context: &mut Ctx,
        publisher: &mut Pub,
    ) {
        //= https://www.rfc-editor.org/rfc/rfc9002#section-6.1.2
        //# Once a later packet within the same packet number space has been
        //# acknowledged, an endpoint SHOULD declare an earlier packet lost if it
        //# was sent a threshold amount of time in the past.
        self.detect_and_remove_lost_packets(timestamp, random_generator, context, publisher);

        let current_path_id = context.path_id();
        let is_handshake_confirmed = context.is_handshake_confirmed();
        let mut current_path_acked_bytes = 0;
        let mut current_path_largest_newly_acked = None;
        let mut newly_acked_ecn_counts = EcnCounts::default();

        for (packet_number, acked_packet_info) in newly_acked_packets {
            let path = context.path_mut_by_id(acked_packet_info.path_id);

            let sent_bytes = acked_packet_info.sent_bytes as usize;
            newly_acked_ecn_counts.increment(acked_packet_info.ecn);

            if acked_packet_info.path_id == current_path_id {
                current_path_acked_bytes += sent_bytes;

                if current_path_largest_newly_acked.is_none_or(|(pn, _)| packet_number > pn) {
                    current_path_largest_newly_acked = Some((packet_number, acked_packet_info));
                }
            } else if sent_bytes > 0 {
                path.congestion_controller.on_ack(
                    acked_packet_info.time_sent,
                    sent_bytes,
                    acked_packet_info.cc_packet_info,
                    &path.rtt_estimator,
                    random_generator,
                    timestamp,
                    &mut congestion_controller::PathPublisher::new(
                        publisher,
                        acked_packet_info.path_id,
                    ),
                );
            }

            //= https://www.rfc-editor.org/rfc/rfc9002#section-6.2.1
            //# The PTO backoff factor is reset when an acknowledgment is received,
            //# except in the following case.  A server might take longer to respond
            //# to packets during the handshake than otherwise.  To protect such a
            //# server from repeated client probes, the PTO backoff is not reset at a
            //# client that is not yet certain that the server has finished
            //# validating the client's address.  That is, a client does not reset
            //# the PTO backoff factor on receiving acknowledgments in Initial
            //# packets.
            if path.is_peer_validated() {
                path.reset_pto_backoff();
            }
        }

        //= https://www.rfc-editor.org/rfc/rfc9002#section-6.2.1
        //# A sender SHOULD restart its PTO timer every time an ack-eliciting
        //# packet is sent or acknowledged,

        // The pseudocode in https://www.rfc-editor.org/rfc/rfc9002.html#section-a.7 does
        // not distinguish between ack-eliciting packets for determining if the PTO timer should
        // be restarted. This behavior is preferred, as detect_and_remove_lost_packets() will
        // cancel the loss timer, and there may still be ack eliciting packets pending that
        // require a PTO timer for recovery.
        self.update_pto_timer(context.active_path(), timestamp, is_handshake_confirmed, random_generator);

        debug_assert!(
            !newly_acked_packets.is_empty(),
            "this method assumes there was at least one newly-acked packet"
        );

        //= https://www.rfc-editor.org/rfc/rfc9000#section-13.4.2.1
        //# Validating ECN counts from reordered ACK frames can result in failure.
        //# An endpoint MUST NOT fail ECN validation as a result of processing an
        //# ACK frame that does not increase the largest acknowledged packet number.
        if new_largest_packet {
            self.process_ecn(
                newly_acked_ecn_counts,
                ecn_counts,
                timestamp,
                context,
                publisher,
            );
        }

        if current_path_acked_bytes > 0 {
            let (_, largest_newly_acked) = current_path_largest_newly_acked
                .expect("At least some bytes were acknowledged on the current path");
            let path = context.path_mut();
            path.congestion_controller.on_ack(
                largest_newly_acked.time_sent,
                current_path_acked_bytes,
                largest_newly_acked.cc_packet_info,
                &path.rtt_estimator,
                random_generator,
                timestamp,
                &mut congestion_controller::PathPublisher::new(publisher, current_path_id),
            );
        }
    }

    fn process_ecn<Ctx: Context<Config>, Pub: event::ConnectionPublisher>(
        &mut self,
        newly_acked_ecn_counts: EcnCounts,
        ack_frame_ecn_counts: Option<EcnCounts>,
        timestamp: Timestamp,
        context: &mut Ctx,
        publisher: &mut Pub,
    ) {
        let path_id = context.path_id();
        let path = context.path_mut();

        let outcome = path.ecn_controller.validate(
            newly_acked_ecn_counts,
            self.sent_packet_ecn_counts,
            self.baseline_ecn_counts,
            ack_frame_ecn_counts,
            timestamp,
            path.rtt_estimator.smoothed_rtt(),
            path_event!(path, path_id),
            publisher,
        );

        if let ValidationOutcome::CongestionExperienced(ce_count) = outcome {
            //= https://www.rfc-editor.org/rfc/rfc9002#section-7.1
            //# If a path has been validated to support Explicit Congestion
            //# Notification (ECN) [RFC3168] [RFC8311], QUIC treats a Congestion
            //# Experienced (CE) codepoint in the IP header as a signal of
            //# congestion.
            context
                .path_mut()
                .congestion_controller
                .on_explicit_congestion(
                    ce_count.as_u64(),
                    timestamp,
                    &mut congestion_controller::PathPublisher::new(publisher, path_id),
                );
            let path = context.path();
            publisher.on_congestion(event::builder::Congestion {
                path: path_event!(path, path_id),
                source: CongestionSource::Ecn,
            })
        }

        self.baseline_ecn_counts = ack_frame_ecn_counts.unwrap_or_default();
        self.sent_packet_ecn_counts -= newly_acked_ecn_counts;
    }

    /// Returns `true` if the recovery manager requires a probe packet to be sent.
    #[inline]
    pub fn requires_probe(&self) -> bool {
        self.pto.has_transmission_interest()
    }

    //= https://www.rfc-editor.org/rfc/rfc9002#appendix-B.9
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

        let path_id_idx = path_id.as_u8();
        publisher.on_recovery_metrics(recovery_event!(path_id_idx, path));

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
        path.congestion_controller.on_packet_discarded(
            discarded_bytes,
            &mut congestion_controller::PathPublisher::new(publisher, path_id),
        );
    }

    //= https://www.rfc-editor.org/rfc/rfc9002#appendix-A.10
    //# DetectAndRemoveLostPackets is called every time an ACK is received or the time threshold
    //# loss detection timer expires. This function operates on the sent_packets for that packet
    //# number space and returns a list of packets newly detected as lost.
    fn detect_and_remove_lost_packets<Ctx: Context<Config>, Pub: event::ConnectionPublisher>(
        &mut self,
        now: Timestamp,
        random_generator: &mut Config::RandomGenerator,
        context: &mut Ctx,
        publisher: &mut Pub,
    ) {
        // Cancel the loss timer. It will be armed again if any unacknowledged packets are
        // older than the largest acked packet, but not old enough to be considered lost yet
        self.loss_timer.cancel();

        let (persistent_congestion_duration, lost_packets) =
            self.detect_lost_packets(now, context, publisher);

        if let Some(lost_packets) = lost_packets {
            self.remove_lost_packets(
                now,
                persistent_congestion_duration,
                lost_packets,
                random_generator,
                context,
                publisher,
            );
        }
    }

    fn detect_lost_packets<Ctx: Context<Config>, Pub: event::ConnectionPublisher>(
        &mut self,
        now: Timestamp,
        context: &mut Ctx,
        publisher: &mut Pub,
    ) -> (Duration, Option<PacketNumberRange>) {
        let largest_acked_packet = self
            .largest_acked_packet
            .expect("This function is only called after an ack has been received");

        let mut persistent_congestion_calculator = persistent_congestion::Calculator::new(
            context.path().rtt_estimator.first_rtt_sample(),
            context.path_id(),
        );

        let mut smallest_lost_packet = None;
        let mut largest_lost_packet = None;
        for (unacked_packet_number, unacked_sent_info) in self.sent_packets.iter() {
            if unacked_packet_number > largest_acked_packet {
                // sent_packets is ordered by packet number, so all remaining packets will be larger
                break;
            }

            let unacked_path_id = unacked_sent_info.path_id;
            let path = &context.path_by_id(unacked_path_id);
            // Calculate how long we wait until a packet is declared lost
            let time_threshold = path.rtt_estimator.loss_time_threshold();

            let loss_outcome = loss::detect(
                time_threshold,
                unacked_sent_info.time_sent,
                loss::K_PACKET_THRESHOLD,
                unacked_packet_number,
                largest_acked_packet,
                now,
            );

            match loss_outcome {
                loss::Outcome::Lost => {
                    if smallest_lost_packet.is_none() {
                        smallest_lost_packet = Some(unacked_packet_number);
                    }
                    largest_lost_packet = Some(unacked_packet_number);

                    // TODO merge contiguous packet numbers
                    let range =
                        PacketNumberRange::new(unacked_packet_number, unacked_packet_number);
                    context.on_packet_loss(&range, publisher);

                    persistent_congestion_calculator
                        .on_lost_packet(unacked_packet_number, unacked_sent_info);
                }
                loss::Outcome::NotLostYet { lost_time } => {
                    //= https://www.rfc-editor.org/rfc/rfc9002#section-6.1.2
                    //# If packets sent prior to the largest acknowledged packet cannot yet
                    //# be declared lost, then a timer SHOULD be set for the remaining time.
                    self.loss_timer.set(lost_time);
                    debug_assert!(
                        !self.loss_timer.is_expired(now),
                        "loss timer was not armed in the future; now: {now}, threshold: {time_threshold:?}\nmanager: {self:#?}"
                    );

                    //= https://www.rfc-editor.org/rfc/rfc9002#section-6.2.1
                    //# The PTO timer MUST NOT be set if a timer is set for time threshold
                    //# loss detection; see Section 6.1.2.  A timer that is set for time
                    //# threshold loss detection will expire earlier than the PTO timer in
                    //# most cases and is less likely to spuriously retransmit data.
                    self.pto.cancel();

                    // assuming sent_packets is ordered by packet number and sent time, all remaining
                    // packets will have a larger packet number and sent time, and are thus not lost.
                    break;
                }
            }
        }

        let sent_packets_to_remove = {
            if let (Some(start), Some(end)) = (smallest_lost_packet, largest_lost_packet) {
                Some(PacketNumberRange::new(start, end))
            } else {
                None
            }
        };

        (
            persistent_congestion_calculator.persistent_congestion_duration(),
            sent_packets_to_remove,
        )
    }

    fn remove_lost_packets<Ctx: Context<Config>, Pub: event::ConnectionPublisher>(
        &mut self,
        now: Timestamp,
        persistent_congestion_duration: Duration,
        lost_packets: PacketNumberRange,
        random_generator: &mut Config::RandomGenerator,
        context: &mut Ctx,
        publisher: &mut Pub,
    ) {
        let current_path_id = context.path_id();
        let mut is_congestion_event = false;
        let mut prev_lost_packet_number = None;

        // Remove the lost packets and account for the bytes on the proper congestion controller
        for (packet_number, sent_info) in self.sent_packets.remove_range(lost_packets) {
            let path = context.path_mut_by_id(sent_info.path_id);

            //= https://www.rfc-editor.org/rfc/rfc9002#section-7.6.2
            //# A sender that does not have state for all packet
            //# number spaces or an implementation that cannot compare send times
            //# across packet number spaces MAY use state for just the packet number
            //# space that was acknowledged.
            let persistent_congestion = persistent_congestion_duration
                > path.rtt_estimator.persistent_congestion_threshold()
                // Check that the packet was sent on this path
                && sent_info.path_id == current_path_id;

            let new_loss_burst = prev_lost_packet_number
                .is_none_or(|prev: PacketNumber| packet_number.checked_distance(prev) != Some(1));

            if sent_info.transmission_mode.is_mtu_probing() {
                //= https://www.rfc-editor.org/rfc/rfc9000#section-14.4
                //# Loss of a QUIC packet that is carried in a PMTU probe is therefore not a
                //# reliable indication of congestion and SHOULD NOT trigger a congestion
                //# control reaction; see Item 7 in Section 3 of [DPLPMTUD].

                //= https://www.rfc-editor.org/rfc/rfc8899#section-3
                //# Loss of a probe packet SHOULD NOT be treated as an
                //# indication of congestion and SHOULD NOT trigger a congestion
                //# control reaction [RFC4821] because this could result in
                //# unnecessary reduction of the sending rate.
                path.congestion_controller.on_packet_discarded(
                    sent_info.sent_bytes as usize,
                    &mut congestion_controller::PathPublisher::new(publisher, sent_info.path_id),
                );
            } else if sent_info.sent_bytes > 0 {
                path.congestion_controller.on_packet_lost(
                    sent_info.sent_bytes as u32,
                    sent_info.cc_packet_info,
                    persistent_congestion,
                    new_loss_burst,
                    random_generator,
                    now,
                    &mut congestion_controller::PathPublisher::new(publisher, sent_info.path_id),
                );
                is_congestion_event = true;
            }

            publisher.on_packet_lost(event::builder::PacketLost {
                packet_header: event::builder::PacketHeader::new(
                    packet_number,
                    publisher.quic_version(),
                ),
                path: path_event!(path, current_path_id),
                bytes_lost: sent_info.sent_bytes,
                is_mtu_probe: sent_info.transmission_mode.is_mtu_probing(),
            });

            let path_id = sent_info.path_id;

            // Notify the ECN controller of packet loss for blackhole detection.
            path.ecn_controller.on_packet_loss(
                sent_info.time_sent,
                sent_info.ecn,
                now,
                path_event!(path, path_id),
                publisher,
            );

            if persistent_congestion {
                //= https://www.rfc-editor.org/rfc/rfc9002#section-5.2
                //# Endpoints SHOULD set the min_rtt to the newest RTT sample after
                //# persistent congestion is established.
                path.rtt_estimator.on_persistent_congestion();
            }

            // Notify the MTU controller of packet loss even if it wasn't a probe since it uses
            // that information for blackhole detection.
            match path.mtu_controller.on_packet_loss(
                packet_number,
                sent_info.sent_bytes,
                new_loss_burst,
                now,
                &mut path.congestion_controller,
                sent_info.path_id,
                publisher,
            ) {
                MtuResult::MtuUpdated(max_datagram_size) => {
                    context.on_mtu_update(max_datagram_size)
                }
                MtuResult::NoChange => {}
            }

            prev_lost_packet_number = Some(packet_number);
        }

        if is_congestion_event {
            let path = context.path();
            publisher.on_congestion(event::builder::Congestion {
                path: path_event!(path, current_path_id),
                source: CongestionSource::PacketLoss,
            })
        }
    }

    #[inline]
    fn check_consistency(&self, active_path: &Path<Config>, is_handshake_confirmed: bool) {
        if cfg!(debug_assertions) {
            assert!(active_path.is_active());

            let ack_eliciting_packets_in_flight = self
                .sent_packets
                .iter()
                .any(|(_, sent_info)| sent_info.ack_elicitation.is_ack_eliciting());

            let mut timer_required = ack_eliciting_packets_in_flight;

            //= https://www.rfc-editor.org/rfc/rfc9002#section-6.2.2.1
            //# it is the client's responsibility to send packets to unblock the server
            //# until it is certain that the server has finished its address validation
            timer_required |= !active_path.is_peer_validated();

            //= https://www.rfc-editor.org/rfc/rfc9002#section-6.2.2.1
            //# If no additional data can be sent, the server's PTO timer MUST NOT be
            //# armed until datagrams have been received from the client, because
            //# packets sent on PTO count against the anti-amplification limit.
            timer_required &= !active_path.at_amplification_limit();

            //= https://www.rfc-editor.org/rfc/rfc9002#section-6.2.1
            //# An endpoint MUST NOT set its PTO timer for the Application Data
            //# packet number space until the handshake is confirmed.
            timer_required &= !self.space.is_application_data() || is_handshake_confirmed;

            // We haven't transmitted anything in this packet space yet so the
            // PTO timer would not be armed yet
            timer_required &= self.time_of_last_ack_eliciting_packet.is_some();

            if timer_required {
                assert_ne!(self.armed_timer_count(), 0);
            }
        }
    }
}

impl<Config: endpoint::Config> timer::Provider for Manager<Config> {
    #[inline]
    fn timers<Q: timer::Query>(&self, query: &mut Q) -> timer::Result {
        //= https://www.rfc-editor.org/rfc/rfc9002#section-6.2.1
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

    fn active_path(&self) -> &Path<Config>;

    fn active_path_mut(&mut self) -> &mut Path<Config>;

    fn path(&self) -> &Path<Config>;

    fn path_mut(&mut self) -> &mut Path<Config>;

    fn path_by_id(&self, path_id: path::Id) -> &path::Path<Config>;

    fn path_mut_by_id(&mut self, path_id: path::Id) -> &mut path::Path<Config>;

    fn path_id(&self) -> path::Id;

    fn validate_packet_ack(
        &mut self,
        timestamp: Timestamp,
        packet_number_range: &PacketNumberRange,
        lowest_tracking_packet_number: PacketNumber,
    ) -> Result<(), transport::Error>;

    fn on_new_packet_ack<Pub: event::ConnectionPublisher>(
        &mut self,
        packet_number_range: &PacketNumberRange,
        publisher: &mut Pub,
    );
    fn on_packet_ack(&mut self, timestamp: Timestamp, packet_number_range: &PacketNumberRange);
    fn on_packet_loss<Pub: event::ConnectionPublisher>(
        &mut self,
        packet_number_range: &PacketNumberRange,
        publisher: &mut Pub,
    );
    fn on_rtt_update(&mut self, now: Timestamp);

    fn on_mtu_update(&mut self, max_datagram_size: u16);
}

impl<Config: endpoint::Config> transmission::interest::Provider for Manager<Config> {
    fn transmission_interest<Q: transmission::interest::Query>(
        &self,
        query: &mut Q,
    ) -> transmission::interest::Result {
        self.pto.transmission_interest(query)
    }
}

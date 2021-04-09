// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    contexts::WriteContext,
    processed_packet::ProcessedPacket,
    space::rx_packet_numbers::{
        ack_eliciting_transmission::{AckElicitingTransmission, AckElicitingTransmissionSet},
        ack_ranges::AckRanges,
        ack_transmission_state::AckTransmissionState,
    },
    timer::VirtualTimer,
    transmission,
};
use s2n_quic_core::{
    ack,
    counter::{Counter, Saturating},
    frame::{Ack, Ping},
    inet::DatagramInfo,
    packet::number::{PacketNumber, PacketNumberSpace},
    time::Timestamp,
    varint::VarInt,
};
use s2n_quic_core::time::TimerIterator;

//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#13.2
//# Endpoints acknowledge all packets they receive and process.  However,
//# only ack-eliciting packets cause an ACK frame to be sent within the
//# maximum ack delay.  Packets that are not ack-eliciting are only
//# acknowledged when an ACK frame is sent for other reasons.
//#
//# When sending a packet for any reason, an endpoint SHOULD attempt to
//# include an ACK frame if one has not been sent recently.  Doing so
//# helps with timely loss detection at the peer.
//#
//# In general, frequent feedback from a receiver improves loss and
//# congestion response, but this has to be balanced against excessive
//# load generated by a receiver that sends an ACK frame in response to
//# every ack-eliciting packet.  The guidance offered below seeks to
//# strike this balance.

#[derive(Clone, Debug)]
pub struct AckManager {
    /// Time at which the AckManager will wake and transmit an ACK
    ack_delay_timer: VirtualTimer,

    /// Used to track the ACK-eliciting transmissions sent from the AckManager
    ack_eliciting_transmissions: AckElicitingTransmissionSet,

    /// All of the processed AckRanges that need to be ACKed
    pub(super) ack_ranges: AckRanges,

    /// Peer's AckSettings from the transport parameters
    pub ack_settings: ack::Settings,

    /// The largest packet number that we've acked - used for pn decoding
    largest_received_packet_number_acked: PacketNumber,

    /// The time at which we received the largest pn
    largest_received_packet_number_at: Option<Timestamp>,

    /// The number of processed packets since transmission
    processed_packets_since_transmission: Counter<u8, Saturating>,

    /// The number of transmissions since the last ACK-eliciting packet was sent
    transmissions_since_elicitation: Counter<u8, Saturating>,

    /// Used to transition through transmission/retransmission states
    transmission_state: AckTransmissionState,
}

impl AckManager {
    pub fn new(packet_space: PacketNumberSpace, ack_settings: ack::Settings) -> Self {
        Self {
            ack_delay_timer: VirtualTimer::default(),
            ack_eliciting_transmissions: AckElicitingTransmissionSet::default(),
            ack_settings,
            ack_ranges: AckRanges::new(ack_settings.ack_ranges_limit as usize),
            largest_received_packet_number_acked: packet_space
                .new_packet_number(VarInt::from_u8(0)),
            largest_received_packet_number_at: None,
            processed_packets_since_transmission: Counter::new(0),
            transmissions_since_elicitation: Counter::new(0),
            transmission_state: AckTransmissionState::default(),
        }
    }

    /// Called when an outgoing packet is being assembled
    pub fn on_transmit<W: WriteContext>(&mut self, context: &mut W) -> bool {
        if !self.transmission_state.should_transmit() {
            return false;
        }

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#7
        //# packets containing only ACK frames do not count
        //# towards bytes in flight and are not congestion controlled.
        let _ = context.transmission_constraint(); // ignored

        let ack_delay = self.ack_delay(context.current_time());
        // TODO retrieve ECN counts from current path
        let ecn_counts = Default::default();

        context
            .write_frame(&Ack {
                ack_delay,
                ack_ranges: &self.ack_ranges,
                ecn_counts,
            })
            .is_some()
    }

    /// Called after an outgoing packet is assembled and `on_transmit` returned `true`
    pub fn on_transmit_complete<W: WriteContext>(&mut self, context: &mut W) {
        debug_assert!(
            self.transmission_state.should_transmit(),
            "`on_transmit_complete` was called when `should_transmit` is false"
        );

        let mut is_ack_eliciting = context.ack_elicitation().is_ack_eliciting();

        if !is_ack_eliciting {
            // check the timer and make sure we can still write a Ping frame before removing it
            // We send a ping even when constrained to retransmissions only, as a fast
            // retransmission that is not ack eliciting will not help us recover faster.
            if (context.transmission_constraint().can_transmit()
                || context.transmission_constraint().can_retransmit())
                && self.transmissions_since_elicitation
                    >= self.ack_settings.ack_elicitation_interval
                && context.write_frame(&Ping).is_some()
            {
                is_ack_eliciting = true;
            } else {
                self.transmissions_since_elicitation += 1;
            }
        }

        self.largest_received_packet_number_acked = self
            .ack_ranges
            .max_value()
            .expect("transmission_state should be Disabled while ack_ranges is empty");

        if is_ack_eliciting {
            // reset the counter
            self.transmissions_since_elicitation = Counter::new(0);

            //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#13.2.4
            //# When a packet containing an ACK frame is sent, the largest
            //# acknowledged in that frame may be saved.
            self.ack_eliciting_transmissions
                .on_transmit(AckElicitingTransmission {
                    sent_in_packet: context.packet_number(),
                    largest_received_packet_number_acked: self.largest_received_packet_number_acked,
                });
        }

        // record a transmission
        self.transmission_state.on_transmit();

        // reset the number of packets since transmission
        self.processed_packets_since_transmission = Counter::new(0);
    }

    /// Called when a set of packets was acknowledged
    pub fn on_packet_ack<A: ack::Set>(&mut self, _datagram: &DatagramInfo, ack_set: &A) {
        if let Some(ack_range) = self.ack_eliciting_transmissions.on_update(ack_set) {
            self.ack_ranges
                .remove(ack_range)
                .expect("The range should always shrink the interval length");

            // `self.transmission_state` will be automatically notified in `on_processed_packet`
            // so wait for that instead
        }
    }

    /// Called when a set of packets was reported lost
    pub fn on_packet_loss<A: ack::Set>(&mut self, ack_set: &A) {
        if self
            .ack_eliciting_transmissions
            .on_update(ack_set)
            .is_some()
        {
            // transition to active mode when packet is lost
            self.transmission_state.on_update(&self.ack_ranges);
            self.transmission_state.activate();
        }
    }

    /// Called after an RX packet has been processed
    pub fn on_processed_packet(&mut self, processed_packet: &ProcessedPacket) {
        let packet_number = processed_packet.packet_number;
        let now = processed_packet.datagram.timestamp;

        // perform some checks before inserting into the ack_ranges
        let (is_ordered, is_largest) = self
            .ack_ranges
            .max_value()
            .and_then(|max_value| {
                // check to see if the packet number is the next one in the sequence
                let is_ordered = packet_number == max_value.next()?;

                // check to see if the packet number is the largest we've seen
                let is_largest = packet_number > max_value;

                Some((is_ordered, is_largest))
            })
            .unwrap_or((true, true));

        // This will fail if `packet_number` is less than `ack_ranges.min_value()`
        // and `ack_ranges` is at capacity.
        //
        // Most likely, this packet is very old and the contents have already
        // been retransmitted by the peer.
        if !self.ack_ranges.insert_packet_number(packet_number) {
            return;
        }

        // Notify the state that the ack_ranges have changed
        self.transmission_state.on_update(&self.ack_ranges);
        self.processed_packets_since_transmission += 1;

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#13.2.5
        //# An endpoint measures the delays intentionally introduced between the
        //# time the packet with the largest packet number is received and the
        //# time an acknowledgment is sent.  The endpoint encodes this
        //# acknowledgement delay in the ACK Delay field of an ACK frame; see
        //# Section 19.3.  This allows the receiver of the ACK frame to adjust
        //# for any intentional delays, which is important for getting a better
        //# estimate of the path RTT when acknowledgments are delayed.
        if is_largest {
            self.largest_received_packet_number_at = Some(now);
        }

        if processed_packet.is_ack_eliciting() {
            let mut should_activate = false;

            //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#13.2.1
            //# In order to assist loss detection at the sender, an endpoint SHOULD
            //# generate and send an ACK frame without delay when it receives an ack-
            //# eliciting packet either:
            //#
            //# *  when the received packet has a packet number less than another
            //#    ack-eliciting packet that has been received, or

            should_activate |= !is_largest;

            //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#13.2.1
            //# *  when the packet has a packet number larger than the highest-
            //#    numbered ack-eliciting packet that has been received and there are
            //#    missing packets between that packet and this packet.

            should_activate |= !is_ordered;

            //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#13.2.1
            //# Similarly, packets marked with the ECN Congestion Experienced (CE)
            //# codepoint in the IP header SHOULD be acknowledged immediately, to
            //# reduce the peer's response time to congestion events.
            should_activate |= processed_packet.datagram.ecn.congestion_experienced();

            // TODO update to draft link after published
            // https://github.com/quicwg/base-drafts/pull/3623
            // An ACK frame SHOULD be generated for at least every 10th ack-eliciting packet

            // TODO support delayed ack proposal
            // https://tools.ietf.org/html/draft-iyengar-quic-delayed-ack-00
            let packet_tolerance = 10;

            should_activate |= self.processed_packets_since_transmission >= packet_tolerance;

            if should_activate {
                self.transmission_state.activate();
            } else if !self.ack_delay_timer.is_armed() {
                //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#13.2
                //# Endpoints acknowledge all packets they receive and process.  However,
                //# only ack-eliciting packets cause an ACK frame to be sent within the
                //# maximum ack delay.  Packets that are not ack-eliciting are only
                //# acknowledged when an ACK frame is sent for other reasons.
                self.ack_delay_timer
                    .set(now + self.ack_settings.max_ack_delay)
            }
        }

        // To save on timer churn, check to see if we've already expired since the
        // last time we sent an ACK frame
        if self.ack_delay_timer.poll_expiration(now).is_ready() {
            self.transmission_state.activate();
        }
    }

    /// Returns all of the component timers
    pub fn timers(&self) -> TimerIterator {
        // NOTE: ack_elicitation_timer is not actively polled

        self.ack_delay_timer.iter()
    }

    /// Called when the connection timer expired
    pub fn on_timeout(&mut self, timestamp: Timestamp) {
        // NOTE: ack_elicitation_timer is not actively polled

        if self.ack_delay_timer.poll_expiration(timestamp).is_ready() {
            // transition to active transmission when we exceed the ack_delay
            self.transmission_state.activate();
        }
    }

    /// Returns the largest received packet number that has been ACKed at least once
    pub fn largest_received_packet_number_acked(&self) -> PacketNumber {
        self.largest_received_packet_number_acked
    }

    /// Computes the ack_delay field for the current state
    fn ack_delay(&self, now: Timestamp) -> VarInt {
        let ack_delay = self
            .largest_received_packet_number_at
            .map(|prev| now.saturating_duration_since(prev))
            .unwrap_or_default();
        self.ack_settings.encode_ack_delay(ack_delay)
    }
}

impl transmission::interest::Provider for AckManager {
    fn transmission_interest(&self) -> transmission::Interest {
        self.transmission_state.transmission_interest()
    }
}

#[cfg(test)]
mod tests {
    use super::{super::tests::*, *};
    use crate::contexts::testing::{MockWriteContext, OutgoingFrameBuffer};
    use core::{
        iter::{empty, once},
        mem::size_of,
        time::Duration,
    };
    use insta::assert_debug_snapshot;
    use s2n_quic_core::{
        ack, endpoint,
        frame::{ping, Frame},
    };

    #[test]
    fn on_transmit_complete_transmission_constrained() {
        let mut manager =
            AckManager::new(PacketNumberSpace::ApplicationData, ack::Settings::default());
        let mut frame_buffer = OutgoingFrameBuffer::new();
        let mut write_context = MockWriteContext::new(
            s2n_quic_platform::time::now(),
            &mut frame_buffer,
            transmission::Constraint::None,
            endpoint::Type::Server,
        );

        manager.ack_ranges = AckRanges::default();
        manager.ack_ranges.insert_packet_number(
            PacketNumberSpace::ApplicationData.new_packet_number(VarInt::from_u8(1)),
        );
        manager.transmission_state = AckTransmissionState::Active { retransmissions: 0 };
        manager.transmissions_since_elicitation =
            Counter::new(ack::Settings::EARLY.ack_elicitation_interval);

        manager.on_transmit_complete(&mut write_context);

        assert_eq!(
            write_context
                .frame_buffer
                .pop_front()
                .expect("Frame is written")
                .as_frame(),
            Frame::Ping(ping::Ping),
            "Ping should be written when transmission is not constrained"
        );

        manager.transmission_state = AckTransmissionState::Active { retransmissions: 0 };
        manager.transmissions_since_elicitation =
            Counter::new(ack::Settings::EARLY.ack_elicitation_interval);
        write_context.frame_buffer.clear();
        write_context.transmission_constraint = transmission::Constraint::CongestionLimited;

        manager.on_transmit_complete(&mut write_context);
        assert!(
            write_context.frame_buffer.is_empty(),
            "Ping should not be written when CongestionLimited"
        );

        manager.transmission_state = AckTransmissionState::Active { retransmissions: 0 };
        manager.transmissions_since_elicitation =
            Counter::new(ack::Settings::EARLY.ack_elicitation_interval);
        write_context.frame_buffer.clear();
        write_context.transmission_constraint = transmission::Constraint::RetransmissionOnly;

        manager.on_transmit_complete(&mut write_context);
        assert_eq!(
            write_context
                .frame_buffer
                .pop_front()
                .expect("Frame is written")
                .as_frame(),
            Frame::Ping(ping::Ping),
            "Ping should be written when transmission is retransmission only"
        );
    }

    #[test]
    fn on_transmit_complete_many_transmissions_since_elicitation() {
        let mut manager =
            AckManager::new(PacketNumberSpace::ApplicationData, ack::Settings::default());
        let mut frame_buffer = OutgoingFrameBuffer::new();
        let mut write_context = MockWriteContext::new(
            s2n_quic_platform::time::now(),
            &mut frame_buffer,
            transmission::Constraint::None,
            endpoint::Type::Server,
        );
        write_context.transmission_constraint = transmission::Constraint::CongestionLimited;

        manager.ack_ranges = AckRanges::default();
        manager.ack_ranges.insert_packet_number(
            PacketNumberSpace::ApplicationData.new_packet_number(VarInt::from_u8(1)),
        );
        manager.transmission_state = AckTransmissionState::Active { retransmissions: 0 };
        manager.transmissions_since_elicitation = Counter::new(u8::max_value());

        manager.on_transmit_complete(&mut write_context);

        assert_eq!(
            manager.transmissions_since_elicitation,
            Counter::new(u8::max_value())
        );
    }

    #[test]
    fn size_of_snapshots() {
        assert_debug_snapshot!("AckManager", size_of::<AckManager>());
    }

    #[test]
    fn client_sending_test() {
        assert_debug_snapshot!(
            "client_sending_test",
            Simulation {
                network: Network {
                    client: Application::new(
                        Endpoint::new(ack::Settings {
                            max_ack_delay: Duration::from_millis(25),
                            ack_delay_exponent: 1,
                            ..Default::default()
                        }),
                        [Duration::from_millis(5)].iter().cycle().take(100).cloned(),
                    )
                    .into(),
                    server: Application::new(
                        Endpoint::new(ack::Settings {
                            max_ack_delay: Duration::from_millis(25),
                            ack_delay_exponent: 1,
                            ..Default::default()
                        }),
                        empty(),
                    )
                    .into(),
                },
                // pass all packets unchanged
                events: empty().collect(),
                delay: Duration::from_millis(0),
            }
            .run()
        );
    }

    #[test]
    fn delayed_client_sending_test() {
        assert_debug_snapshot!(
            "delayed_client_sending_test",
            Simulation {
                network: Network {
                    client: Application::new(
                        Endpoint::new(ack::Settings {
                            max_ack_delay: Duration::from_millis(25),
                            ack_delay_exponent: 1,
                            ..Default::default()
                        }),
                        [Duration::from_millis(5)].iter().cycle().take(100).cloned(),
                    )
                    .into(),
                    server: Application::new(
                        Endpoint::new(ack::Settings {
                            max_ack_delay: Duration::from_millis(25),
                            ack_delay_exponent: 1,
                            ..Default::default()
                        }),
                        empty(),
                    )
                    .into(),
                },
                // pass all packets unchanged
                events: empty().collect(),
                // delay sending each packet by 100ms
                delay: Duration::from_millis(100),
            }
            .run()
        );
    }

    #[test]
    fn high_latency_test() {
        assert_debug_snapshot!(
            "high_latency_test",
            Simulation {
                network: Network {
                    client: Application::new(
                        Endpoint::new(ack::Settings {
                            max_ack_delay: Duration::from_millis(25),
                            ack_delay_exponent: 1,
                            ..Default::default()
                        }),
                        [Duration::from_millis(5)].iter().cycle().take(100).cloned(),
                    )
                    .into(),
                    server: Application::new(
                        Endpoint::new(ack::Settings {
                            max_ack_delay: Duration::from_millis(100),
                            ack_delay_exponent: 1,
                            ..Default::default()
                        }),
                        [Duration::from_millis(5)].iter().cycle().take(100).cloned(),
                    )
                    .into(),
                },
                // pass all packets unchanged
                events: empty().collect(),
                // delay sending each packet by 1s
                delay: Duration::from_millis(1000),
            }
            .run()
        );
    }

    #[test]
    fn lossy_network_test() {
        assert_debug_snapshot!(
            "lossy_network_test",
            Simulation {
                network: Network {
                    client: Application::new(
                        Endpoint::new(ack::Settings {
                            max_ack_delay: Duration::from_millis(25),
                            ack_delay_exponent: 1,
                            ..Default::default()
                        }),
                        [Duration::from_millis(5)].iter().cycle().take(100).cloned(),
                    )
                    .into(),
                    server: Application::new(
                        Endpoint::new(ack::Settings {
                            max_ack_delay: Duration::from_millis(100),
                            ack_delay_exponent: 1,
                            ..Default::default()
                        }),
                        [Duration::from_millis(5)].iter().cycle().take(100).cloned(),
                    )
                    .into(),
                },
                // drop every 5th packet
                events: once(NetworkEvent::Pass)
                    .cycle()
                    .take(4)
                    .chain(once(NetworkEvent::Drop))
                    .collect(),
                // delay sending each packet by 100ms
                delay: Duration::from_millis(0),
            }
            .run()
        );
    }
}

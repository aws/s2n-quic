// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Sends a "flag" frame towards the peer
//!
//! This can be used by frames, like PING and HANDSHAKE_DONE, that don't have any
//! content other than the frame tag itself. At the cost of a single byte per packet, it will passively
//! transmit the flag in any outgoing packets until the peer ACKs the frame. This is to increase
//! the likelihood the peer receives the flag, even in a high-loss environment. It may also be used
//! by frames that do have content, such as DC_STATELESS_RESET_TOKENS, that similarly require aggressive
//! transmission to increase the likelihood the peer receives the frame.

use crate::{
    contexts::{OnTransmitError, WriteContext},
    transmission,
};
use s2n_quic_core::{ack, packet::number::PacketNumber};

#[derive(Debug, Default)]
pub struct Flag<W: Writer> {
    delivery: DeliveryState,
    writer: W,
}

pub trait Writer: Default {
    fn write_frame<W: WriteContext>(&mut self, context: &mut W) -> Option<PacketNumber>;
}

#[derive(Debug, PartialEq, Default)]
enum DeliveryState {
    /// The flag has not been requested
    #[default]
    Idle,

    /// The flag needs to be transmitted
    RequiresTransmission,

    /// The flag was lost and needs to be retransmitted
    RequiresRetransmission,

    /// The flag has been transmitted and is pending acknowledgement.
    ///
    /// Note that in this state, flags are being passively transmitted to ensure
    /// the peer can make progress.
    InFlight {
        /// A stable flag transmission
        ///
        /// In this case, "stable" means the oldest transmission that
        /// hasn't been acked by the peer.
        ///
        /// This packet number is stored to ensure the transmission is either confirmed or declared
        /// lost. Without it, the latest packet number would be a moving target and never
        /// transition to the `Delivered` state
        stable: PacketNumber,

        /// The latest flag transmission
        latest: PacketNumber,
    },

    /// The flag has been delivered
    Delivered,
}

impl<W: Writer> Flag<W> {
    /// Constructs a flag with the given `writer`
    pub fn new(writer: W) -> Self {
        Self {
            writer,
            ..Default::default()
        }
    }

    /// Returns `true` if the flag hasn't been sent
    pub fn is_idle(&self) -> bool {
        matches!(self.delivery, DeliveryState::Idle)
    }

    /// Returns `true` if the flag has been delivered
    pub fn is_delivered(&self) -> bool {
        matches!(self.delivery, DeliveryState::Delivered)
    }

    /// Stars sending the flag to the peer
    pub fn send(&mut self) {
        if self.is_idle() || self.is_delivered() {
            self.delivery = DeliveryState::RequiresTransmission;
        }
    }

    /// Mark the flag as delivered
    pub fn finish(&mut self) {
        self.delivery = DeliveryState::Delivered
    }

    /// This method gets called when a packet delivery got acknowledged
    pub fn on_packet_ack<A: ack::Set>(&mut self, ack_set: &A) -> bool {
        if let DeliveryState::InFlight { stable, latest } = &self.delivery {
            if ack_set.contains(*stable) || ack_set.contains(*latest) {
                self.finish();
                return true;
            }
        }

        false
    }

    /// This method gets called when a packet loss is reported
    pub fn on_packet_loss<A: ack::Set>(&mut self, ack_set: &A) -> bool {
        let mut lost = false;
        if let DeliveryState::InFlight { stable, latest } = &mut self.delivery {
            // If stable is lost, fall back on latest
            if ack_set.contains(*stable) {
                lost = true;
                *stable = *latest;
            }

            // Force retransmission
            if ack_set.contains(*latest) {
                lost = true;
                self.delivery = DeliveryState::RequiresRetransmission;
            }
        }
        lost
    }

    /// Queries the component for any outgoing frames that need to get sent
    pub fn on_transmit<C: WriteContext>(&mut self, context: &mut C) -> Result<(), OnTransmitError> {
        let constraint = context.transmission_constraint();
        match &mut self.delivery {
            DeliveryState::RequiresTransmission if constraint.can_transmit() => {
                if let Some(packet_number) = self.writer.write_frame(context) {
                    self.delivery = DeliveryState::InFlight {
                        stable: packet_number,
                        latest: packet_number,
                    }
                }
            }
            DeliveryState::RequiresRetransmission if constraint.can_retransmit() => {
                if let Some(packet_number) = self.writer.write_frame(context) {
                    self.delivery = DeliveryState::InFlight {
                        stable: packet_number,
                        latest: packet_number,
                    }
                }
            }
            DeliveryState::InFlight { latest, .. } if constraint.can_transmit() => {
                if let Some(packet_number) = self.writer.write_frame(context) {
                    *latest = packet_number;
                }
            }
            _ => {}
        }

        Ok(())
    }
}

impl<W: Writer> transmission::interest::Provider for Flag<W> {
    #[inline]
    fn transmission_interest<Q: transmission::interest::Query>(
        &self,
        query: &mut Q,
    ) -> transmission::interest::Result {
        match &self.delivery {
            DeliveryState::RequiresTransmission => query.on_new_data(),
            DeliveryState::RequiresRetransmission => query.on_lost_data(),
            _ => Ok(()),
        }
    }
}

pub type Ping = Flag<PingWriter>;

#[derive(Debug, Default)]
pub struct PingWriter;

impl Writer for PingWriter {
    fn write_frame<W: WriteContext>(&mut self, context: &mut W) -> Option<PacketNumber> {
        if context.ack_elicitation().is_ack_eliciting() {
            // we don't need to write a PING frame but we'll store the PacketNumber since it'll be
            // ACKed as if we did
            Some(context.packet_number())
        } else {
            context.write_frame(&s2n_quic_core::frame::Ping)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{contexts::testing::*, transmission::interest::Provider};
    use s2n_quic_core::{endpoint, time::clock::testing as time};

    #[test]
    fn ping_test() {
        let mut frame_buffer = OutgoingFrameBuffer::new();
        let mut context = MockWriteContext::new(
            time::now(),
            &mut frame_buffer,
            transmission::Constraint::None,
            transmission::Mode::Normal,
            endpoint::Type::Server,
        );

        let mut pinger = Ping::default();

        assert!(pinger.is_idle());
        assert!(
            !pinger.has_transmission_interest(),
            "status should not express interest in default state"
        );

        pinger.on_transmit(&mut context).unwrap();

        assert!(
            context.frame_buffer.is_empty(),
            "status should not transmit in default state"
        );

        pinger.send();
        assert!(!pinger.is_idle());

        assert_eq!(
            pinger.get_transmission_interest(),
            transmission::Interest::NewData,
            "status should express interest in deliver after calling send"
        );

        pinger.send();
        assert_eq!(
            pinger.get_transmission_interest(),
            transmission::Interest::NewData,
            "status should accept duplicate calls to send"
        );

        context.transmission_constraint = transmission::Constraint::CongestionLimited;
        pinger.on_transmit(&mut context).unwrap();
        assert!(!pinger.is_idle());

        assert_eq!(
            pinger.delivery,
            DeliveryState::RequiresTransmission,
            "status should not transmit when congestion limited"
        );
        assert!(
            context.frame_buffer.is_empty(),
            "status should not transmit in congestion limited state"
        );

        context.transmission_constraint = transmission::Constraint::None;

        pinger.on_transmit(&mut context).unwrap();
        assert!(!pinger.is_idle());

        let stable_packet_number = context
            .frame_buffer
            .pop_front()
            .expect("status should write PING frames")
            .packet_nr;
        context.frame_buffer.clear();

        assert_eq!(
            pinger.delivery,
            DeliveryState::InFlight {
                stable: stable_packet_number,
                latest: stable_packet_number
            }
        );

        context.transmission_constraint = transmission::Constraint::RetransmissionOnly;

        pinger.on_transmit(&mut context).unwrap();
        assert!(!pinger.is_idle());

        assert!(
            context.frame_buffer.is_empty(),
            "status should not passively write frames when transmission constrained"
        );

        context.transmission_constraint = transmission::Constraint::None;

        pinger.on_transmit(&mut context).unwrap();
        assert!(!pinger.is_idle());

        let latest_packet_number = context
            .frame_buffer
            .pop_front()
            .expect("status should passively write PING frames")
            .packet_nr;
        context.frame_buffer.clear();

        assert_eq!(
            pinger.delivery,
            DeliveryState::InFlight {
                stable: stable_packet_number,
                latest: latest_packet_number,
            }
        );

        pinger.on_packet_loss(&stable_packet_number);

        assert_eq!(
            pinger.delivery,
            DeliveryState::InFlight {
                stable: latest_packet_number,
                latest: latest_packet_number,
            },
            "status should transition to latest on stable packet loss"
        );

        pinger.on_packet_loss(&latest_packet_number);

        assert_eq!(
            pinger.get_transmission_interest(),
            transmission::Interest::LostData,
            "transmission should be active on latest packet loss"
        );
        assert_eq!(
            pinger.delivery,
            DeliveryState::RequiresRetransmission,
            "status should force retransmission on loss"
        );

        context.transmission_constraint = transmission::Constraint::CongestionLimited;
        pinger.on_transmit(&mut context).unwrap();
        assert!(!pinger.is_idle());

        assert_eq!(
            pinger.delivery,
            DeliveryState::RequiresRetransmission,
            "status should not transmit when congestion limited"
        );

        context.transmission_constraint = transmission::Constraint::RetransmissionOnly;

        pinger.on_transmit(&mut context).unwrap();

        let latest_packet_number = context
            .frame_buffer
            .pop_front()
            .expect("status should passively write PING frames")
            .packet_nr;
        context.frame_buffer.clear();

        pinger.on_packet_ack(&latest_packet_number);

        assert_eq!(pinger.delivery, DeliveryState::Delivered);

        assert!(
            !pinger.has_transmission_interest(),
            "status should not express interest after complete",
        );

        pinger.on_transmit(&mut context).unwrap();
        assert!(
            context.frame_buffer.is_empty(),
            "status should not transmit after complete"
        );
    }
}

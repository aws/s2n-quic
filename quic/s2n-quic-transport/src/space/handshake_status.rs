use crate::{
    contexts::{OnTransmitError, WriteContext},
    transmission,
};
use s2n_quic_core::{ack_set::AckSet, frame::HandshakeDone, packet::number::PacketNumber};

/// Tracks handshake status for a connection
#[derive(Debug, PartialEq)]
pub enum HandshakeStatus {
    /// The handshake has started and is in progress.
    InProgress,

    /// The HANDSHAKE_DONE frame has been requested to be delivered. This state is only possible
    /// on the server.
    RequiresTransmission,

    /// A previous HANDSHAKE_DONE frame had been sent and lost and we need to send another. This
    /// state is only possible on the server.
    RequiresRetransmission,

    /// The HANDSHAKE_DONE frame has been transmitted and is pending acknowledgement.
    ///
    /// Note that in this state, HANDSHAKE_DONE frames are being passively transmitted to ensure
    /// the peer can make progress.
    ///
    /// This state is only possible on the server.
    InFlight {
        /// A stable HANDSHAKE_DONE transmission
        ///
        /// In this case, "stable" means the oldest transmission that
        /// hasn't been acked by the peer.
        ///
        /// This packet number is stored to ensure the transmission is either confirmed or declared
        /// lost. Without it, the latest packet number would be a moving target and never
        /// transition to the `Confirmed` state
        stable: PacketNumber,

        /// The latest HANDSHAKE_DONE transmission
        latest: PacketNumber,
    },

    //= https://tools.ietf.org/id/draft-ietf-quic-tls-29#4.1.2
    //# the TLS handshake is considered confirmed at the
    //# server when the handshake completes.  At the client, the handshake is
    //# considered confirmed when a HANDSHAKE_DONE frame is received.
    /// The handshake has been confirmed
    Confirmed,
}

impl Default for HandshakeStatus {
    fn default() -> Self {
        Self::InProgress
    }
}

impl HandshakeStatus {
    /// This method is called on the server after the handshake has been completed
    pub fn on_handshake_done(&mut self) {
        if matches!(self, Self::InProgress) {
            //= https://tools.ietf.org/id/draft-ietf-quic-tls-29#4.11.2
            //# The server MUST send a HANDSHAKE_DONE
            //# frame as soon as it completes the handshake.

            //= https://tools.ietf.org/id/draft-ietf-quic-tls-29#4.1.2
            //# the TLS handshake is considered confirmed at the
            //# server when the handshake completes.
            *self = Self::RequiresTransmission;
        }
    }

    /// This method is called on the client when the HANDSHAKE_DONE frame has been received
    pub fn on_handshake_done_received(&mut self) {
        if matches!(self, Self::InProgress) {
            //= https://tools.ietf.org/id/draft-ietf-quic-tls-29#4.1.2
            //# At the client, the handshake is
            //# considered confirmed when a HANDSHAKE_DONE frame is received.
            *self = Self::Confirmed;
        }
    }

    /// This method gets called when a packet delivery got acknowledged
    pub fn on_packet_ack<A: AckSet>(&mut self, ack_set: &A) {
        if let Self::InFlight { stable, latest } = self {
            if ack_set.contains(*stable) || ack_set.contains(*latest) {
                *self = Self::Confirmed;
            }
        }
    }

    /// This method gets called when a packet loss is reported
    pub fn on_packet_loss<A: AckSet>(&mut self, ack_set: &A) {
        if let Self::InFlight { stable, latest } = self {
            // If stable is lost, fall back on latest
            if ack_set.contains(*stable) {
                *stable = *latest;
            }

            // Force retransmission
            if ack_set.contains(*latest) {
                *self = Self::RequiresRetransmission;
            }
        }
    }

    /// Queries the component for any outgoing frames that need to get sent
    pub fn on_transmit<W: WriteContext>(&mut self, context: &mut W) -> Result<(), OnTransmitError> {
        let constraint = context.transmission_constraint();
        match self {
            Self::RequiresTransmission if constraint.can_transmit() => {
                if let Some(packet_number) = context.write_frame(&HandshakeDone) {
                    *self = Self::InFlight {
                        stable: packet_number,
                        latest: packet_number,
                    }
                }
            }
            Self::RequiresRetransmission if constraint.can_retransmit() => {
                if let Some(packet_number) = context.write_frame(&HandshakeDone) {
                    *self = Self::InFlight {
                        stable: packet_number,
                        latest: packet_number,
                    }
                }
            }
            // passively write HANDSHAKE_DONE frames while waiting for an ACK
            Self::InFlight { latest, .. } if constraint.can_transmit() => {
                if let Some(packet_number) = context.write_frame(&HandshakeDone) {
                    *latest = packet_number;
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Returns `true` if the handshake has been confirmed
    pub fn is_confirmed(&self) -> bool {
        // As long as it's not in progress it should be considered confirmed
        !matches!(self, Self::InProgress)
    }
}

impl transmission::interest::Provider for HandshakeStatus {
    fn transmission_interest(&self) -> transmission::Interest {
        match self {
            Self::RequiresTransmission => transmission::Interest::NewData,
            Self::RequiresRetransmission => transmission::Interest::LostData,
            _ => transmission::Interest::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contexts::testing::*;
    use s2n_quic_core::endpoint::EndpointType;
    use s2n_quic_platform::time;

    #[test]
    fn server_test() {
        let connection_context = MockConnectionContext::new(EndpointType::Server);
        let mut frame_buffer = OutgoingFrameBuffer::new();
        let mut context =
            MockWriteContext::new(&connection_context, time::now(), &mut frame_buffer);

        let mut status = HandshakeStatus::default();

        assert!(!status.is_confirmed());
        assert_eq!(
            status.frame_exchange_interests(),
            transmission::Interest::default(),
            "status should not express interest in default state"
        );

        status.on_transmit(&mut context).unwrap();

        assert!(
            context.frame_buffer.is_empty(),
            "status should not transmit in default state"
        );

        status.on_handshake_done();
        assert!(status.is_confirmed());

        assert_eq!(
            status.transmission_interest(),
            transmission::Interest::NewData,
            "status should express interest in deliver after handshake done"
        );

        status.on_handshake_done();
        assert_eq!(
            status.frame_exchange_interests(),
            FrameExchangeInterests {
                delivery_notifications: false,
                transmission: true,
            },
            "status should accept duplicate calls to handshake_done"
        );

        status.on_transmit(&mut context).unwrap();
        assert!(status.is_confirmed());

        let stable_packet_number = context
            .frame_buffer
            .pop_front()
            .expect("status should write HANDSHAKE_DONE frames")
            .packet_nr;

        assert_eq!(
            status,
            HandshakeStatus::InFlight {
                stable: stable_packet_number,
                latest: stable_packet_number
            }
        );

        status.on_transmit(&mut context).unwrap();
        assert!(status.is_confirmed());

        let latest_packet_number = context
            .frame_buffer
            .pop_front()
            .expect("status should passively write HANDSHAKE_DONE frames")
            .packet_nr;

        assert_eq!(
            status,
            HandshakeStatus::InFlight {
                stable: stable_packet_number,
                latest: latest_packet_number,
            }
        );

        status.on_packet_loss(&stable_packet_number);

        assert_eq!(
            status,
            HandshakeStatus::InFlight {
                stable: latest_packet_number,
                latest: latest_packet_number,
            },
            "status should transition to latest on stable packet loss"
        );

        status.on_packet_loss(&latest_packet_number);

        assert!(
            status.frame_exchange_interests().transmission,
            "transmission should be active on latest packet loss"
        );

        status.on_transmit(&mut context).unwrap();

        let latest_packet_number = context
            .frame_buffer
            .pop_front()
            .expect("status should passively write HANDSHAKE_DONE frames")
            .packet_nr;

        status.on_packet_ack(&latest_packet_number);

        assert_eq!(status, HandshakeStatus::Confirmed);

        assert_eq!(
            status.frame_exchange_interests(),
            FrameExchangeInterests::default(),
            "status should not express interest after complete",
        );

        status.on_transmit(&mut context).unwrap();
        assert!(
            context.frame_buffer.is_empty(),
            "status should not transmit after complete"
        );
    }

    #[test]
    fn client_test() {
        let connection_context = MockConnectionContext::new(EndpointType::Client);
        let mut frame_buffer = OutgoingFrameBuffer::new();
        let mut context =
            MockWriteContext::new(&connection_context, time::now(), &mut frame_buffer);

        let mut status = HandshakeStatus::default();

        assert!(!status.is_confirmed());

        assert_eq!(
            status.frame_exchange_interests(),
            FrameExchangeInterests::default(),
            "status should not express interest in default state"
        );

        status.on_transmit(&mut context).unwrap();
        assert!(
            context.frame_buffer.is_empty(),
            "status should not transmit in default state"
        );

        status.on_handshake_done_received();
        assert!(status.is_confirmed());

        assert_eq!(status, HandshakeStatus::Confirmed);

        assert_eq!(
            status.frame_exchange_interests(),
            FrameExchangeInterests::default(),
            "status should not express interest after complete",
        );

        status.on_transmit(&mut context).unwrap();
        assert!(
            context.frame_buffer.is_empty(),
            "status should not transmit after complete"
        );

        // try calling it multiple times
        status.on_handshake_done_received();
        assert_eq!(status, HandshakeStatus::Confirmed);
    }
}

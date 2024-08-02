// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{contexts::WriteContext, endpoint, sync::flag, transmission};
use s2n_quic_core::{
    ack,
    event::{self, ConnectionPublisher},
    frame::HandshakeDone,
    packet::number::PacketNumber,
};

pub type Flag = flag::Flag<HandshakeDoneWriter>;

/// The handshake status is used to track the handshake progress.
///
/// As the handshake proceeds, it unlocks certain progress on the connection i.e.
/// discarding keys, sending and receiving 1-rtt packets.
///
/// The handshake status transitions from Pending -> Complete -> Confirmed. However
/// the progress differs between the Client and Server. The chart below captures
/// these different requirements.
///
/// |        | Complete         | Confirmed                  |
/// |--------|------------------|----------------------------|
/// | server | TLS-completes    | TLS-completes              |
/// |        |                  |                            |
/// | client | TLS-completes    | HANDSHAKE_DONE received    |
/// |        |                  | or 1-rtt acked             |
///
/// TLS-completes: TLS stack reports the handshake as complete. This happens when
///                the TLS stack has sent the Finished message and verified peer's
///                Finished message.
///
/// The major differences between the Client and Server include:
/// - the handshake is complete and confirmed once the TLS-completes on the Server.
/// - the Server is required to send a HANDSHAKE_DONE frame once the handshake completes.
/// - the Client must wait for a HANDSHAKE_DONE (or an acked 1-rtt packet) to 'Confirm'
///   the handshake.
///
/// Note: s2n-quic does not implement the optional 1-rtt acked requirement.
#[derive(Debug, Default)]
pub enum HandshakeStatus {
    /// Awaiting handshake completion
    #[default]
    InProgress,

    /// Client handshake Complete
    ///
    /// Transient state while client awaits HANDSHAKE_DONE
    ClientComplete,

    /// Server handshake Complete
    ///
    /// Transient state where server awaits sending a HANDSHAKE_DONE
    ServerCompleteConfirmed(Flag),

    /// Terminal state requiring no further action
    Confirmed,
}

impl HandshakeStatus {
    /// Returns `true` if the handshake has been completed
    #[inline]
    pub fn is_complete(&self) -> bool {
        // The handshake is complete once its not Pending
        !matches!(self, HandshakeStatus::InProgress)
    }

    /// Returns `true` if the handshake has been confirmed
    pub fn is_confirmed(&self) -> bool {
        match self {
            HandshakeStatus::InProgress | HandshakeStatus::ClientComplete => false,
            HandshakeStatus::ServerCompleteConfirmed(_) => {
                //= https://www.rfc-editor.org/rfc/rfc9001#section-4.1.2
                //# the TLS handshake is considered confirmed at the
                //# server when the handshake completes
                true
            }
            HandshakeStatus::Confirmed => true,
        }
    }

    /// This method is called on the client when the HANDSHAKE_DONE
    /// frame has been received
    pub fn on_handshake_done_received<Pub: ConnectionPublisher>(&mut self, publisher: &mut Pub) {
        if let HandshakeStatus::ClientComplete = self {
            publisher.on_handshake_status_updated(event::builder::HandshakeStatusUpdated {
                status: event::builder::HandshakeStatus::HandshakeDoneAcked,
            });
            publisher.on_handshake_status_updated(event::builder::HandshakeStatusUpdated {
                status: event::builder::HandshakeStatus::Confirmed,
            });
            //= https://www.rfc-editor.org/rfc/rfc9001#section-4.1.2
            //# At the client, the handshake is
            //# considered confirmed when a HANDSHAKE_DONE frame is received.
            *self = HandshakeStatus::Confirmed;
        }
    }

    /// This method is called after the TLS handshake has been completed
    pub fn on_handshake_complete<Pub: ConnectionPublisher>(
        &mut self,
        endpoint_type: endpoint::Type,
        publisher: &mut Pub,
    ) {
        debug_assert!(
            matches!(self, Self::InProgress),
            "on_handshake_complete should only be called once."
        );
        publisher.on_handshake_status_updated(event::builder::HandshakeStatusUpdated {
            status: event::builder::HandshakeStatus::Complete,
        });

        if endpoint_type.is_server() {
            publisher.on_handshake_status_updated(event::builder::HandshakeStatusUpdated {
                status: event::builder::HandshakeStatus::Confirmed,
            });
            //= https://www.rfc-editor.org/rfc/rfc9001#section-4.1.2
            //# The server MUST send a HANDSHAKE_DONE
            //# frame as soon as the handshake is complete.
            let mut flag = Flag::default();
            flag.send();
            *self = HandshakeStatus::ServerCompleteConfirmed(flag);
        } else {
            *self = HandshakeStatus::ClientComplete;
        }
    }

    /// Used for tracking when the HANDSHAKE_DONE frame has been delivered
    /// to the peer.
    pub fn on_packet_ack<A: ack::Set, Pub: event::ConnectionPublisher>(
        &mut self,
        ack_set: &A,
        publisher: &mut Pub,
    ) {
        if let HandshakeStatus::ServerCompleteConfirmed(flag) = self {
            // The server is required to re-transmit the frame until it is
            // acknowledged by the peer. Once it is delivered, the state
            // can transition to Confirmed.
            if flag.on_packet_ack(ack_set) {
                publisher.on_handshake_status_updated(event::builder::HandshakeStatusUpdated {
                    status: event::builder::HandshakeStatus::HandshakeDoneAcked,
                });
                *self = HandshakeStatus::Confirmed;
            }
        }
    }

    /// Used for tracking when the HANDSHAKE_DONE frame needs to be
    /// re-transmitted.
    pub fn on_packet_loss<A: ack::Set, Pub: event::ConnectionPublisher>(
        &mut self,
        ack_set: &A,
        publisher: &mut Pub,
    ) {
        if let HandshakeStatus::ServerCompleteConfirmed(flag) = self {
            //= https://www.rfc-editor.org/rfc/rfc9000#section-13.3
            //# The HANDSHAKE_DONE frame MUST be retransmitted until it is
            //# acknowledged.
            if flag.on_packet_loss(ack_set) {
                publisher.on_handshake_status_updated(event::builder::HandshakeStatusUpdated {
                    status: event::builder::HandshakeStatus::HandshakeDoneLost,
                });
            }
        }
    }

    /// Queries if any HANDSHAKE_DONE frames need to get sent
    pub fn on_transmit<C: WriteContext>(&mut self, context: &mut C) {
        if let HandshakeStatus::ServerCompleteConfirmed(flag) = self {
            //= https://www.rfc-editor.org/rfc/rfc9000#section-19.20
            //# A HANDSHAKE_DONE frame can only be sent by the server.
            let _ = flag.on_transmit(context);
        }
    }
}

#[derive(Debug, Default)]
pub struct HandshakeDoneWriter;

impl flag::Writer for HandshakeDoneWriter {
    fn write_frame<W: WriteContext>(&mut self, context: &mut W) -> Option<PacketNumber> {
        debug_assert!(
            context.local_endpoint_type().is_server(),
            "Only servers should transmit HANDSHAKE_DONE frames"
        );
        context.write_frame(&HandshakeDone)
    }
}

impl transmission::interest::Provider for HandshakeStatus {
    #[inline]
    fn transmission_interest<Q: transmission::interest::Query>(
        &self,
        query: &mut Q,
    ) -> transmission::interest::Result {
        if let HandshakeStatus::ServerCompleteConfirmed(flag) = self {
            flag.transmission_interest(query)
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod fuzz_target;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{contexts::testing::*, transmission::interest::Provider};
    use s2n_quic_core::{endpoint, event::testing::Publisher, time::clock::testing as time};

    #[test]
    fn server_test() {
        let mut publisher = Publisher::snapshot();
        let mut frame_buffer = OutgoingFrameBuffer::new();
        let mut context = MockWriteContext::new(
            time::now(),
            &mut frame_buffer,
            transmission::Constraint::None,
            transmission::Mode::Normal,
            endpoint::Type::Server,
        );

        let mut status = HandshakeStatus::default();

        assert!(!status.is_confirmed());
        assert!(!status.is_complete());
        assert_eq!(
            status.get_transmission_interest(),
            transmission::Interest::default(),
            "status should not express interest in default state"
        );

        status.on_transmit(&mut context);
        assert!(
            context.frame_buffer.is_empty(),
            "status should not transmit in default state"
        );

        //= https://www.rfc-editor.org/rfc/rfc9001#section-4.1.2
        //= type=test
        //# the TLS handshake is considered confirmed at the
        //# server when the handshake completes.
        status.on_handshake_complete(endpoint::Type::Server, &mut publisher);
        assert!(status.is_confirmed());
        assert!(status.is_complete());

        assert_eq!(
            status.get_transmission_interest(),
            transmission::Interest::NewData,
            "status should express interest in deliver after handshake complete"
        );

        status.on_transmit(&mut context);

        let packet_number = context
            .frame_buffer
            .pop_front()
            .expect("status should write HANDSHAKE_DONE frames")
            .packet_nr;

        status.on_packet_ack(&packet_number, &mut publisher);
        assert!(status.is_confirmed());

        assert!(
            !status.has_transmission_interest(),
            "status should not express interest after complete",
        );

        status.on_transmit(&mut context);
        assert!(
            context.frame_buffer.is_empty(),
            "status should not transmit after complete"
        );
    }

    #[test]
    fn client_test() {
        let mut publisher = Publisher::snapshot();
        let mut frame_buffer = OutgoingFrameBuffer::new();
        let mut context = MockWriteContext::new(
            time::now(),
            &mut frame_buffer,
            transmission::Constraint::None,
            transmission::Mode::Normal,
            endpoint::Type::Client,
        );

        let mut status = HandshakeStatus::default();

        assert!(!status.is_complete());
        assert!(!status.is_confirmed());

        assert!(
            !status.has_transmission_interest(),
            "client does not transmit a HANDSHAKE_DONE"
        );

        status.on_transmit(&mut context);
        assert!(
            context.frame_buffer.is_empty(),
            "client does not transmit a HANDSHAKE_DONE"
        );

        // the handshake must be complete prior to being confirmed
        status.on_handshake_done_received(&mut publisher);
        assert!(!status.is_complete());
        assert!(!status.is_confirmed());

        status.on_handshake_complete(endpoint::Type::Client, &mut publisher);
        assert!(status.is_complete());

        assert!(
            !status.has_transmission_interest(),
            "client does not transmit a HANDSHAKE_DONE"
        );

        status.on_transmit(&mut context);
        assert!(
            context.frame_buffer.is_empty(),
            "client does not transmit a HANDSHAKE_DONE"
        );

        // confirm the client handshake
        status.on_handshake_done_received(&mut publisher);
        assert!(status.is_confirmed());

        // try calling it multiple times
        status.on_handshake_done_received(&mut publisher);
        assert!(status.is_confirmed());
    }
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    contexts::{OnTransmitError, WriteContext},
    sync::flag,
    transmission,
};
use s2n_quic_core::{ack, frame::HandshakeDone, packet::number::PacketNumber};

pub type Flag = flag::Flag<HandshakeDoneWriter>;

#[derive(Debug, Default)]
pub struct HandshakeStatus {
    flag: Flag,
}

impl HandshakeStatus {
    /// This method is called on the server after the handshake has been completed
    pub fn on_handshake_complete(&mut self) {
        if self.flag.is_idle() {
            // TODO: the following requirement was removed from the final RFC.
            // Confirm if the implementation can be optimized by relaxing the
            // implemenated requirement.
            //
            // removed: [https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#4.9.2]
            // The server MUST send a HANDSHAKE_DONE
            // frame as soon as it completes the handshake.

            //= https://www.rfc-editor.org/rfc/rfc9001.txt#4.1.2
            //# the TLS handshake is considered confirmed at the
            //# server when the handshake completes.
            //
            //= https://www.rfc-editor.org/rfc/rfc9000.txt#13.3
            //= type=TODO
            //# The HANDSHAKE_DONE frame MUST be retransmitted until it is
            //# acknowledged.
            self.flag.send();
        }
    }

    /// This method is called on the client when the HANDSHAKE_DONE frame has been received
    pub fn on_handshake_done_received(&mut self) {
        //= https://www.rfc-editor.org/rfc/rfc9001.txt#4.1.2
        //# At the client, the handshake is
        //# considered confirmed when a HANDSHAKE_DONE frame is received.
        self.flag.finish();
    }

    /// Returns `true` if the handshake has been confirmed
    pub fn is_confirmed(&self) -> bool {
        // As long as it's not in progress it should be considered confirmed
        !self.flag.is_idle()
    }

    /// Returns `true` if the handshake has been completed
    pub fn is_complete(&self) -> bool {
        // As long as it's not in progress it should be considered confirmed
        !self.flag.is_idle()
    }

    pub fn on_packet_ack<A: ack::Set>(&mut self, ack_set: &A) -> bool {
        self.flag.on_packet_ack(ack_set)
    }

    /// This method gets called when a packet loss is reported
    pub fn on_packet_loss<A: ack::Set>(&mut self, ack_set: &A) {
        self.flag.on_packet_loss(ack_set)
    }

    /// Queries the component for any outgoing frames that need to get sent
    pub fn on_transmit<C: WriteContext>(&mut self, context: &mut C) -> Result<(), OnTransmitError> {
        self.flag.on_transmit(context)
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
        self.flag.transmission_interest(query)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{contexts::testing::*, transmission::interest::Provider};
    use s2n_quic_core::endpoint;
    use s2n_quic_platform::time;

    #[test]
    fn server_test() {
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
        assert_eq!(
            status.get_transmission_interest(),
            transmission::Interest::default(),
            "status should not express interest in default state"
        );

        status.on_transmit(&mut context).unwrap();

        assert!(
            context.frame_buffer.is_empty(),
            "status should not transmit in default state"
        );

        status.on_handshake_complete();
        assert!(status.is_confirmed());

        assert_eq!(
            status.get_transmission_interest(),
            transmission::Interest::NewData,
            "status should express interest in deliver after handshake complete"
        );

        status.on_handshake_complete();
        assert_eq!(
            status.get_transmission_interest(),
            transmission::Interest::NewData,
            "status should accept duplicate calls to handshake_complete"
        );

        status.on_transmit(&mut context).unwrap();

        let packet_number = context
            .frame_buffer
            .pop_front()
            .expect("status should write HANDSHAKE_DONE frames")
            .packet_nr;

        status.on_packet_ack(&packet_number);

        assert!(status.is_confirmed());

        assert!(
            !status.has_transmission_interest(),
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
        let mut frame_buffer = OutgoingFrameBuffer::new();
        let mut context = MockWriteContext::new(
            time::now(),
            &mut frame_buffer,
            transmission::Constraint::None,
            transmission::Mode::Normal,
            endpoint::Type::Client,
        );

        let mut status = HandshakeStatus::default();

        assert!(!status.is_confirmed());

        assert!(
            !status.has_transmission_interest(),
            "status should not express interest in default state"
        );

        status.on_transmit(&mut context).unwrap();
        assert!(
            context.frame_buffer.is_empty(),
            "status should not transmit in default state"
        );

        status.on_handshake_done_received();
        assert!(status.is_confirmed());

        assert!(
            !status.has_transmission_interest(),
            "status should not express interest after complete",
        );

        status.on_transmit(&mut context).unwrap();
        assert!(
            context.frame_buffer.is_empty(),
            "status should not transmit after complete"
        );

        // try calling it multiple times
        status.on_handshake_done_received();
        assert!(status.is_confirmed());
    }
}

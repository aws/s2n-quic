use crate::{contexts::WriteContext, sync::flag, transmission};
use core::ops::{Deref, DerefMut};
use s2n_quic_core::{frame::HandshakeDone, packet::number::PacketNumber};

pub type Flag = flag::Flag<HandshakeDoneWriter>;

#[derive(Default)]
pub struct HandshakeStatus(Flag);

impl HandshakeStatus {
    /// This method is called on the server after the handshake has been completed
    pub fn on_handshake_done(&mut self) {
        if self.is_idle() {
            //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#4.9.2
            //# The server MUST send a HANDSHAKE_DONE
            //# frame as soon as it completes the handshake.

            //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#4.1.2
            //# the TLS handshake is considered confirmed at the
            //# server when the handshake completes.
            self.send();
        }
    }

    /// This method is called on the client when the HANDSHAKE_DONE frame has been received
    pub fn on_handshake_done_received(&mut self) {
        //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#4.1.2
        //# At the client, the handshake is
        //# considered confirmed when a HANDSHAKE_DONE frame is received.
        self.finish();
    }

    /// Returns `true` if the handshake has been confirmed
    pub fn is_confirmed(&self) -> bool {
        // As long as it's not in progress it should be considered confirmed
        !self.is_idle()
    }
}

#[derive(Default)]
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

impl Deref for HandshakeStatus {
    type Target = Flag;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for HandshakeStatus {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl transmission::interest::Provider for HandshakeStatus {
    fn transmission_interest(&self) -> transmission::Interest {
        self.0.transmission_interest()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{contexts::testing::*, transmission::interest::Provider};
    use s2n_quic_core::endpoint::EndpointType;
    use s2n_quic_platform::time;

    #[test]
    fn server_test() {
        let connection_context = MockConnectionContext::new(EndpointType::Server);
        let mut frame_buffer = OutgoingFrameBuffer::new();
        let mut context = MockWriteContext::new(
            &connection_context,
            time::now(),
            &mut frame_buffer,
            transmission::Constraint::None,
        );

        let mut status = HandshakeStatus::default();

        assert!(!status.is_confirmed());
        assert_eq!(
            status.transmission_interest(),
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
            status.transmission_interest(),
            transmission::Interest::NewData,
            "status should accept duplicate calls to handshake_done"
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
            status.transmission_interest().is_none(),
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
        let mut context = MockWriteContext::new(
            &connection_context,
            time::now(),
            &mut frame_buffer,
            transmission::Constraint::None,
        );

        let mut status = HandshakeStatus::default();

        assert!(!status.is_confirmed());

        assert!(
            status.transmission_interest().is_none(),
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
            status.transmission_interest().is_none(),
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

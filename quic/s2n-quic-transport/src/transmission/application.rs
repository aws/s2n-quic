use crate::{
    connection::{self, ConnectionTransmissionContext},
    recovery,
    space::{rx_packet_numbers::AckManager, HandshakeStatus, TxPacketNumbers},
    stream::AbstractStreamManager,
    transmission::{self, interest::Provider as _},
};
use s2n_codec::{Encoder, EncoderBuffer};
use s2n_quic_core::{
    frame::Padding,
    packet::{encoding::PacketPayloadEncoder, number::PacketNumber},
};

pub struct Transmission<'a, Config: connection::Config> {
    pub ack_manager: &'a mut AckManager,
    pub context: &'a ConnectionTransmissionContext<'a, Config>,
    pub handshake_status: &'a mut HandshakeStatus,
    pub packet_number: PacketNumber,
    pub recovery_manager: &'a mut recovery::Manager,
    pub stream_manager: &'a mut AbstractStreamManager<Config::Stream>,
    pub tx_packet_numbers: &'a mut TxPacketNumbers,
    pub outcome: &'a mut transmission::Outcome,
    pub transmission_constraint: transmission::Constraint,
}

impl<'a, Config: connection::Config> PacketPayloadEncoder for Transmission<'a, Config> {
    fn encoding_size_hint<E: Encoder>(&mut self, _encoder: &E, minimum_len: usize) -> usize {
        // TODO ask the stream manager. We need to return something that assures that
        // - either Padding gets written
        // - or the StreamManager can write a `Stream` frame without length information (which must
        //   be the last frame) of sufficient size.
        // Note that we can not write a short `Stream` frame without length information and then
        // pad it.
        if !matches!(self.transmission_interest(), transmission::Interest::None) {
            minimum_len.max(1)
        } else {
            0
        }
    }

    fn encode(&mut self, buffer: &mut EncoderBuffer, minimum_len: usize, overhead_len: usize) {
        debug_assert!(
            buffer.is_empty(),
            "the implementation assumes an empty buffer"
        );

        let mut context = super::Context {
            outcome: self.outcome,
            buffer,
            context: self.context,
            packet_number: self.packet_number,
            transmission_constraint: self.transmission_constraint,
        };

        let did_send_ack = self.ack_manager.on_transmit(&mut context);

        // TODO: Handle errors
        let _ = self.handshake_status.on_transmit(&mut context);
        let _ = self.stream_manager.on_transmit(&mut context);
        self.recovery_manager.on_transmit(&mut context);

        if did_send_ack {
            // inform the ack manager the packet is populated
            self.ack_manager.on_transmit_complete(&mut context);
        }

        if !context.buffer.is_empty() {
            // Add padding up to minimum_len
            let length = minimum_len.saturating_sub(context.buffer.len());
            if length > 0 {
                context.buffer.encode(&Padding { length });
            }

            self.tx_packet_numbers.on_transmit(self.packet_number);
            self.outcome.bytes_sent = overhead_len + buffer.len();
        }
    }
}

impl<'a, Config: connection::Config> transmission::interest::Provider for Transmission<'a, Config> {
    fn transmission_interest(&self) -> transmission::Interest {
        transmission::Interest::default()
            + self.ack_manager.transmission_interest()
            + self.handshake_status.transmission_interest()
            + self.recovery_manager.transmission_interest()
            + self.stream_manager.transmission_interest()
    }
}

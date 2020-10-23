use crate::{
    connection::{self, ConnectionTransmissionContext},
    recovery,
    space::{rx_packet_numbers::AckManager, CryptoStream, TxPacketNumbers},
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
    pub crypto_stream: &'a mut CryptoStream,
    pub packet_number: PacketNumber,
    pub recovery_manager: &'a mut recovery::Manager,
    pub tx_packet_numbers: &'a mut TxPacketNumbers,
    pub outcome: &'a mut transmission::Outcome,
    pub transmission_constraint: transmission::Constraint,
}

impl<'a, Config: connection::Config> PacketPayloadEncoder for Transmission<'a, Config> {
    fn encoding_size_hint<E: Encoder>(&mut self, _encoder: &E, minimum_len: usize) -> usize {
        // TODO return the minimum length required to encode a crypto frame + a certain amount of data
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
            packet_number: self.packet_number,
            buffer,
            context: self.context,
            transmission_constraint: self.transmission_constraint,
        };

        let did_send_ack = self.ack_manager.on_transmit(&mut context);

        let _ = self.crypto_stream.tx.on_transmit((), &mut context);
        self.recovery_manager.on_transmit(&mut context);

        if did_send_ack {
            // inform the ack manager the packet is populated
            self.ack_manager.on_transmit_complete(&mut context);
        }

        // TODO add required padding if client

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
            + self.crypto_stream.transmission_interest()
    }
}

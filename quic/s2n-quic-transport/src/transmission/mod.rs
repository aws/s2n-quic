pub mod context;
use context::Context;

pub mod application;
pub mod early;
pub mod interest;

pub use crate::contexts::WriteContext;
pub use interest::Interest;

/// re-export core
pub use s2n_quic_core::transmission::*;

use crate::{
    connection::{self},
    recovery,
    space::{rx_packet_numbers::AckManager, TxPacketNumbers},
    transmission::{self, interest::Provider as _},
};
use core::{marker::PhantomData, ops::RangeInclusive};
use s2n_codec::{Encoder, EncoderBuffer};
use s2n_quic_core::{
    frame::Padding,
    packet::{
        encoding::PacketPayloadEncoder,
        number::{PacketNumber, PacketNumberSpace},
    },
    time::Timestamp,
};

pub trait Payload: interest::Provider {
    fn size_hint(&self, payload_range: RangeInclusive<usize>) -> usize;
    fn on_transmit<W: WriteContext>(&mut self, context: &mut W);
    fn packet_number_space(&self) -> PacketNumberSpace;
}

pub struct Transmission<'a, Config: connection::Config, P: Payload> {
    pub ack_manager: &'a mut AckManager,
    pub config: PhantomData<Config>,
    pub outcome: &'a mut transmission::Outcome,
    pub payload: P,
    pub packet_number: PacketNumber,
    pub recovery_manager: &'a mut recovery::Manager,
    pub timestamp: Timestamp,
    pub transmission_constraint: transmission::Constraint,
    pub tx_packet_numbers: &'a mut TxPacketNumbers,
}

impl<'a, Config: connection::Config, P: Payload> PacketPayloadEncoder
    for Transmission<'a, Config, P>
{
    fn encoding_size_hint<E: Encoder>(&mut self, encoder: &E, minimum_len: usize) -> usize {
        if !self.transmission_interest().is_none() {
            self.payload.size_hint(minimum_len..=encoder.capacity())
        } else {
            0
        }
    }

    fn encode(&mut self, buffer: &mut EncoderBuffer, minimum_len: usize, overhead_len: usize) {
        debug_assert!(
            buffer.is_empty(),
            "the implementation assumes an empty buffer"
        );

        let mut context: Context<Config> = Context {
            outcome: self.outcome,
            buffer,
            packet_number: self.packet_number,
            transmission_constraint: self.transmission_constraint,
            timestamp: self.timestamp,
            config: Default::default(),
        };

        let did_send_ack = self.ack_manager.on_transmit(&mut context);

        // Payloads can only transmit and retransmit
        if matches!(
            context.transmission_constraint(),
            transmission::Constraint::None | transmission::Constraint::RetransmissionOnly
        ) {
            self.payload.on_transmit(&mut context);
        }

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

impl<'a, Config: connection::Config, P: Payload> transmission::interest::Provider
    for Transmission<'a, Config, P>
{
    fn transmission_interest(&self) -> transmission::Interest {
        transmission::Interest::default()
            + self.ack_manager.transmission_interest()
            + self.recovery_manager.transmission_interest()
            + self.payload.transmission_interest()
    }
}

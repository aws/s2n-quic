// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

pub mod context;
use context::Context;

pub mod application;
pub mod connection_close;
pub mod early;
pub mod interest;

pub use crate::contexts::WriteContext;
pub use interest::Interest;

/// re-export core
pub use s2n_quic_core::transmission::*;

use crate::{
    endpoint, path,
    space::TxPacketNumbers,
    transmission::{self, interest::Provider as _},
};
use core::{marker::PhantomData, ops::RangeInclusive};
use s2n_codec::{Encoder, EncoderBuffer};
use s2n_quic_core::{
    event,
    frame::Padding,
    packet::{
        encoding::PacketPayloadEncoder,
        number::{PacketNumber, PacketNumberSpace},
        stateless_reset,
    },
    time::Timestamp,
};

pub trait Payload: interest::Provider {
    fn size_hint(&self, payload_range: RangeInclusive<usize>) -> usize;
    fn on_transmit<W: WriteContext>(&mut self, context: &mut W);
    fn packet_number_space(&self) -> PacketNumberSpace;
}

pub struct Transmission<'a, 'sub, Config: endpoint::Config, P: Payload> {
    pub config: PhantomData<Config>,
    pub outcome: &'a mut transmission::Outcome,
    pub payload: P,
    pub packet_number: PacketNumber,
    pub timestamp: Timestamp,
    pub transmission_constraint: transmission::Constraint,
    pub transmission_mode: transmission::Mode,
    pub tx_packet_numbers: &'a mut TxPacketNumbers,
    pub path_id: path::Id,
    pub publisher: &'a mut event::ConnectionPublisherSubscriber<
        'sub,
        <Config as endpoint::Config>::EventSubscriber,
    >,
    pub packet_interceptor: &'a mut <Config as endpoint::Config>::PacketInterceptor,
}

impl<'a, 'sub, Config: endpoint::Config, P: Payload> PacketPayloadEncoder
    for Transmission<'a, 'sub, Config, P>
{
    fn encoding_size_hint<E: Encoder>(&mut self, encoder: &E, minimum_len: usize) -> usize {
        if self.has_transmission_interest() {
            self.payload.size_hint(minimum_len..=encoder.capacity())
        } else {
            0
        }
    }

    fn encode(
        &mut self,
        buffer: &mut EncoderBuffer,
        minimum_len: usize,
        header_len: usize,
        tag_len: usize,
    ) {
        debug_assert!(
            buffer.is_empty(),
            "the implementation assumes an empty buffer"
        );

        let mut context: Context<Config> = Context {
            outcome: self.outcome,
            buffer,
            packet_number: self.packet_number,
            transmission_constraint: self.transmission_constraint,
            transmission_mode: self.transmission_mode,
            timestamp: self.timestamp,
            header_len,
            tag_len,
            config: Default::default(),
            path_id: self.path_id,
            publisher: self.publisher,
        };

        self.payload.on_transmit(&mut context);

        if !context.buffer.is_empty() {
            // Add padding up to minimum_len
            let mut length = minimum_len.saturating_sub(context.buffer.len());

            // if we've only got a few bytes left in the buffer may as well pad it to full
            // capacity
            let remaining_capacity = context.buffer.remaining_capacity();
            if remaining_capacity < stateless_reset::min_indistinguishable_packet_len(tag_len) {
                length = remaining_capacity;
            }

            if length > 0 {
                // Use `write_frame_forced` to bypass congestion controller checks
                // since we still want to send this packet despite Padding being
                // congestion controlled.
                context.write_frame_forced(&Padding { length });
            }

            {
                // allow the packet_interceptor provider to do its thing
                use s2n_quic_core::packet::interceptor::{Interceptor, Packet};
                self.packet_interceptor.intercept_tx_packet(
                    Packet {
                        number: self.packet_number,
                        timestamp: self.timestamp,
                    },
                    buffer,
                );
            }

            self.tx_packet_numbers.on_transmit(self.packet_number);
            self.outcome.bytes_sent = header_len + tag_len + buffer.len();
        }
    }
}

impl<'a, 'sub, Config: endpoint::Config, P: Payload> transmission::interest::Provider
    for Transmission<'a, 'sub, Config, P>
{
    fn transmission_interest<Q: transmission::interest::Query>(
        &self,
        query: &mut Q,
    ) -> transmission::interest::Result {
        self.payload.transmission_interest(query)
    }
}

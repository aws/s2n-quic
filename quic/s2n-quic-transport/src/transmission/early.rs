// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    contexts::WriteContext,
    recovery,
    space::{rx_packet_numbers::AckManager, CryptoStream},
    transmission,
};
use core::ops::RangeInclusive;
use s2n_quic_core::packet::number::PacketNumberSpace;

pub struct Payload<'a> {
    pub ack_manager: &'a mut AckManager,
    pub crypto_stream: &'a mut CryptoStream,
    pub packet_number_space: PacketNumberSpace,
    pub recovery_manager: &'a mut recovery::Manager,
}

/// Rather than creating a packet with a very small CRYPTO frame (under 16 bytes), it would be
/// better to wait for another transmission and send something larger. This should be better for
/// performance, anyway, since you end up paying for encryption/decryption.
const MIN_SIZE: usize = 16;

impl<'a> super::Payload for Payload<'a> {
    fn size_hint(&self, range: RangeInclusive<usize>) -> usize {
        (*range.start()).max(MIN_SIZE)
    }

    fn on_transmit<W: WriteContext>(&mut self, context: &mut W) {
        let did_send_ack = self.ack_manager.on_transmit(context);

        // Payloads can only transmit and retransmit
        if context.transmission_constraint().can_transmit()
            || context.transmission_constraint().can_retransmit()
        {
            let _ = self.crypto_stream.tx.on_transmit((), context);
        }

        self.recovery_manager.on_transmit(context);

        if did_send_ack {
            // inform the ack manager the packet is populated
            self.ack_manager.on_transmit_complete(context);
        }
    }

    fn packet_number_space(&self) -> PacketNumberSpace {
        self.packet_number_space
    }
}

impl<'a> transmission::interest::Provider for Payload<'a> {
    fn transmission_interest(&self) -> transmission::Interest {
        transmission::Interest::default()
            + self.ack_manager.transmission_interest()
            + self.crypto_stream.transmission_interest()
            + self.recovery_manager.transmission_interest()
    }
}

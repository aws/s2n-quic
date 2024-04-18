// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    ack::transmission::Transmission,
    packet::number::{PacketNumber, PacketNumberSpace},
    varint::VarInt,
};

/// Generates AckElicitingTransmissions from increasing packet numbers
pub fn transmissions_iter() -> impl Iterator<Item = Transmission> {
    packet_numbers_iter().map(|pn| Transmission {
        sent_in_packet: pn,
        largest_received_packet_number_acked: pn,
    })
}

/// Generates increasing packet numbers
pub fn packet_numbers_iter() -> impl Iterator<Item = PacketNumber> {
    Iterator::map(0u32.., |pn| {
        PacketNumberSpace::ApplicationData.new_packet_number(VarInt::from_u32(pn))
    })
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::packet::stream::decoder::Packet;
use s2n_codec::EncoderValue;
use s2n_quic_core::{
    ack,
    frame::{self, ack::EcnCounts},
    packet::number::{PacketNumberSpace, SlidingWindow, SlidingWindowError},
    varint::VarInt,
};

#[derive(Debug, Default)]
pub struct StreamFilter {
    window: SlidingWindow,
}

impl StreamFilter {
    #[inline]
    pub fn on_packet(&mut self, packet: &Packet) -> Result<(), SlidingWindowError> {
        let packet_number = PacketNumberSpace::Initial.new_packet_number(packet.packet_number());
        self.window.insert(packet_number)
    }
}

#[derive(Debug)]
pub struct Space {
    pub packets: ack::Ranges,
    pub transmission: ack::transmission::Set,
    pub filter: StreamFilter,
}

impl Default for Space {
    #[inline]
    fn default() -> Self {
        Self {
            packets: ack::Ranges::new(usize::MAX),
            transmission: Default::default(),
            filter: Default::default(),
        }
    }
}

impl Space {
    #[inline]
    pub fn on_largest_delivered_packet(&mut self, largest_delivered_packet: VarInt) {
        let packet_number = PacketNumberSpace::Initial.new_packet_number(largest_delivered_packet);
        if let Some(to_remove) = self.transmission.on_update(&packet_number) {
            let _ = self.packets.remove(to_remove);
        }
    }

    #[inline]
    pub fn clear(&mut self) {
        self.packets.clear();
    }

    #[inline]
    pub fn encoding(
        &mut self,
        max_data_encoding_size: VarInt,
        ack_delay: VarInt,
        ecn_counts: Option<EcnCounts>,
        mtu: u16,
    ) -> (Option<frame::Ack<&ack::Ranges>>, VarInt) {
        loop {
            if self.packets.is_empty() {
                return (None, max_data_encoding_size);
            }

            {
                let frame = frame::Ack {
                    ack_delay,
                    ack_ranges: &self.packets,
                    ecn_counts,
                };
                let encoding_size: VarInt = frame.encoding_size().try_into().unwrap();
                let encoding_size = max_data_encoding_size + encoding_size;

                if encoding_size + 100 <= mtu as usize {
                    let frame = frame::Ack {
                        ack_delay,
                        ack_ranges: &self.packets,
                        ecn_counts,
                    };
                    return (Some(frame), encoding_size);
                }
            }

            // pop packet numbers until we fit in the mtu
            let _ = self.packets.pop_min();
        }
    }

    #[inline]
    pub fn on_transmit(&mut self, packet_number: VarInt) {
        if let Some(largest_received_packet_number_acked) = self.packets.max_value() {
            let sent_in_packet = PacketNumberSpace::Initial.new_packet_number(packet_number);
            self.transmission
                .on_transmit(ack::transmission::Transmission {
                    sent_in_packet,
                    largest_received_packet_number_acked,
                });
        }
    }
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::packet::stream::decoder::Packet;
use s2n_codec::EncoderValue;
use s2n_quic_core::{
    ack, ensure,
    frame::{self, ack::EcnCounts},
    packet::number::{PacketNumber, PacketNumberSpace, SlidingWindow, SlidingWindowError},
    time::{Clock, Timestamp},
    varint::VarInt,
};

#[derive(Debug, Default)]
pub struct StreamFilter {
    window: SlidingWindow,
}

impl StreamFilter {
    #[inline]
    pub fn on_packet(&mut self, packet: &Packet) -> Result<(), SlidingWindowError> {
        self.on_packet_number(packet.packet_number())
    }

    /// Check and record a packet number for duplicate filtering.
    ///
    /// Returns Ok if the packet is new, Err if it's a duplicate.
    #[inline]
    pub fn on_packet_number(&mut self, packet_number: VarInt) -> Result<(), SlidingWindowError> {
        let packet_number = PacketNumberSpace::Initial.new_packet_number(packet_number);
        self.window.insert(packet_number)
    }
}

#[derive(Debug)]
pub struct Space {
    packets: ack::Ranges,
    transmission: ack::transmission::Set,
    max_received_packet_time: Option<Timestamp>,
    pub filter: StreamFilter,
}

impl Default for Space {
    #[inline]
    fn default() -> Self {
        Self {
            packets: ack::Ranges::new(usize::MAX),
            transmission: Default::default(),
            filter: Default::default(),
            max_received_packet_time: None,
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

    pub fn max_received_packet(&self) -> Option<PacketNumber> {
        self.packets.max_value()
    }

    pub fn interval_len(&self) -> usize {
        self.packets.interval_len()
    }

    pub fn ack_delay(&self, now: Timestamp) -> VarInt {
        let delay = self
            .max_received_packet_time
            .map(|time| now.saturating_duration_since(time))
            .unwrap_or_default();
        VarInt::try_from(delay.as_micros()).unwrap_or(VarInt::MAX)
    }

    pub fn on_packet_received(&mut self, packet_number: VarInt, now: Timestamp) {
        let packet_number = PacketNumberSpace::Initial.new_packet_number(packet_number);
        ensure!(self.packets.insert_packet_number(packet_number).is_ok());
        if self.packets.max_value() == Some(packet_number) {
            self.max_received_packet_time = Some(now);
        }
    }

    #[inline]
    pub fn clear(&mut self) {
        self.packets.clear();
    }

    #[inline]
    pub fn encoding<Clk>(
        &mut self,
        max_data_encoding_size: VarInt,
        ecn_counts: Option<EcnCounts>,
        mtu: u16,
        clock: &Clk,
    ) -> (Option<frame::Ack<&ack::Ranges>>, VarInt)
    where
        Clk: Clock + ?Sized,
    {
        let ack_delay = self.ack_delay(clock.get_time());

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

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::packet::stream::decoder::Packet;
use s2n_quic_core::packet::number::{PacketNumberSpace, SlidingWindow, SlidingWindowError};

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

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use s2n_quic_core::{
    frame::ack_elicitation::{AckElicitable, AckElicitation},
    inet::DatagramInfo,
    packet::number::PacketNumber,
};

/// Tracks information about a packet that has been processed
#[derive(Clone, Copy, Debug)]
pub struct ProcessedPacket<'a> {
    pub(crate) packet_number: PacketNumber,
    pub(crate) datagram: &'a DatagramInfo,
    pub(crate) ack_elicitation: AckElicitation,
}

impl<'a> ProcessedPacket<'a> {
    /// Creates a processed packet tracker
    pub fn new(packet_number: PacketNumber, datagram: &'a DatagramInfo) -> Self {
        Self {
            packet_number,
            datagram,
            ack_elicitation: AckElicitation::default(),
        }
    }

    /// Records information about a processed frame
    pub fn on_processed_frame<F: AckElicitable>(&mut self, frame: &F) {
        self.ack_elicitation |= frame.ack_elicitation()
    }

    /// Returns `true` if any of the processed frames are ack eliciting
    pub fn is_ack_eliciting(&self) -> bool {
        self.ack_elicitation.is_ack_eliciting()
    }
}

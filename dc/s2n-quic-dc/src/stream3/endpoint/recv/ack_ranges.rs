// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Lightweight ACK range tracker for the stream3 recv path.
//!
//! Tracks received packet numbers and produces pre-encoded ACK range bodies suitable
//! for writing into the shared ACK state. Unlike the stream2 ack::Space, this struct
//! does not encode ack_delay (that's computed by the sender at assembly time) and does
//! not maintain transmission tracking (ACK-of-ACK trimming is deferred).

use bytes::Bytes;
use s2n_codec::EncoderValue;
use s2n_quic_core::{
    ack,
    frame::{self, ack::EcnCounts},
    packet::number::PacketNumberSpace,
    time::Timestamp,
    varint::VarInt,
};

/// Conservative overhead estimate for packet-level framing around an ACK body.
///
/// Accounts for: tag, credentials, wire_version, source_control_port, packet_number,
/// routing_info, header_len varint, Header::Ack metadata, payload_len varint, crypto tag.
pub const PACKET_OVERHEAD: usize = 100;

/// Tracks received packet numbers and encodes ACK range bodies for the shared state.
pub(crate) struct AckRanges {
    packets: ack::Ranges,
    /// When the largest packet number was received — written to the shared state so
    /// the sender can compute ack_delay at assembly time.
    max_received_packet_time: Option<Timestamp>,
}

impl Default for AckRanges {
    fn default() -> Self {
        Self {
            packets: ack::Ranges::new(usize::MAX),
            max_received_packet_time: None,
        }
    }
}

impl AckRanges {
    /// Record a received packet number and its arrival time.
    pub fn on_packet_received(&mut self, packet_number: VarInt, now: Timestamp) {
        let pn = PacketNumberSpace::Initial.new_packet_number(packet_number);
        if self.packets.insert_packet_number(pn).is_err() {
            return;
        }
        if self.packets.max_value() == Some(pn) {
            self.max_received_packet_time = Some(now);
        }
    }

    /// Returns when the largest acknowledged packet was received, if any.
    pub fn largest_recv_time(&self) -> Option<Timestamp> {
        self.max_received_packet_time
    }

    /// Encode the ACK ranges (and optional ECN counts) into a `Bytes` buffer.
    ///
    /// Pops the lowest ranges if the encoding exceeds `max_body_len` so the ACK
    /// always fits in a single packet. The most recent ranges (highest PNs) are
    /// preserved since those are most useful for loss detection.
    ///
    /// Currently uses the standard QUIC ACK frame encoding with ack_delay=0 as a
    /// placeholder. The sender stamps the real delay in the Header::Ack field.
    ///
    /// TODO: use a custom encoding that drops the tag, count, and ack_delay fields to save
    /// 3 bytes per ACK body. We own both sides of the wire format.
    ///
    /// Returns `None` if there are no ranges to encode.
    pub fn encode_body(
        &mut self,
        ecn_counts: Option<EcnCounts>,
        max_body_len: usize,
    ) -> Option<Bytes> {
        loop {
            if self.packets.is_empty() {
                return None;
            }

            let frame = frame::Ack {
                ack_delay: VarInt::ZERO,
                ack_ranges: &self.packets,
                ecn_counts,
            };

            let encoding_size = frame.encoding_size();
            if encoding_size <= max_body_len {
                return Some(Bytes::from(frame.encode_to_vec()));
            }

            let _ = self.packets.pop_min();
        }
    }
}

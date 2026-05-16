// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Decode multi-frame packets produced by the stream encoder.
//!
//! The stream assembler packs multiple frames into one encrypted datagram:
//!
//! ```text
//! [packet header: tag, credentials, wire_version, src_ctrl_port, pkt_number]
//! [RoutingInfo::SenderId { source_sender_id }]
//! [payload_len varint]
//! [header_len varint][frame metadata: per-frame Header + optional payload_len]
//! --- encrypted ---
//! [frame payloads concatenated]
//! [auth tag]
//! ```
//!
//! After the outer packet has been decrypted in place, the application header holds
//! per-frame metadata (Header type tag + optional payload length VarInt), while the
//! payload descriptor holds the concatenated, decrypted frame payloads.
//!
//! This module provides [`decode_frames`], a lazy iterator that parses the application
//! header and yields `(Header, payload_len)` pairs without any heap allocation.  The
//! caller is responsible for slicing the corresponding payload bytes from the decrypted
//! payload region (e.g. via [`bytes::BytesMut::split_to`]).

use crate::endpoint::frame::Header;
use s2n_codec::{DecoderBuffer, DecoderError};
use s2n_quic_core::varint::VarInt;

#[cfg(test)]
mod tests;

/// A lazy iterator over the per-frame metadata in a stream application header.
///
/// Each call to [`Iterator::next`] decodes the next frame's [`Header`] and
/// payload length from the application header bytes, yielding
/// `Ok((header, payload_len))` on success or `Err(DecoderError)` on malformed input.
///
/// The caller is responsible for consuming exactly `payload_len` bytes from the
/// corresponding payload region (e.g. via [`bytes::BytesMut::split_to`]) for
/// each yielded frame.
///
/// Obtain an instance via [`decode_frames`].
pub(crate) struct FrameIter<'a> {
    metadata: DecoderBuffer<'a>,
}

impl<'a> Iterator for FrameIter<'a> {
    type Item = Result<(Header, usize), DecoderError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.metadata.is_empty() {
            return None;
        }

        let result = (|| {
            let (header, rest) = self.metadata.decode::<Header>()?;
            self.metadata = rest;

            let payload_len = if header.has_payload_length() {
                let (len, rest) = self.metadata.decode::<VarInt>()?;
                self.metadata = rest;
                len.as_u64() as usize
            } else {
                0
            };

            Ok((header, payload_len))
        })();

        Some(result)
    }
}

/// Returns a lazy iterator over the per-frame metadata in the application header.
///
/// `application_header` contains the per-frame metadata (header type tag + optional
/// payload_len VarInt) as encoded by the stream assembler.
///
/// Each item from the iterator is `Ok((header, payload_len))`. The caller must
/// consume exactly `payload_len` bytes from the decrypted payload region for each
/// yielded frame, and verify that all payload bytes have been consumed after the
/// iterator is exhausted.
///
/// No heap allocation is performed.
pub(crate) fn decode_frames(application_header: &[u8]) -> FrameIter<'_> {
    FrameIter {
        metadata: DecoderBuffer::new(application_header),
    }
}

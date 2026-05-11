// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Decode multi-frame packets produced by the stream3 encoder.
//!
//! The stream3 assembler packs multiple frames into one encrypted datagram:
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

use crate::stream3::frame::Header;
use s2n_codec::{DecoderBuffer, DecoderError};
use s2n_quic_core::varint::VarInt;

/// A lazy iterator over the per-frame metadata in a stream3 application header.
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
/// payload_len VarInt) as encoded by the stream3 assembler.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packet::datagram::{QueuePair, ResetTarget};
    use crate::stream3::frame::Header;
    use s2n_codec::{Encoder, EncoderBuffer, EncoderValue};
    use s2n_quic_core::varint::VarInt;

    /// Encode a single frame entry into the application header buffer, mirroring
    /// the format written by `assemble::push_frame_metadata`.
    fn push_frame_metadata(buf: &mut Vec<u8>, header: &Header, payload_len: usize) {
        let payload_len_varint = VarInt::try_from(payload_len as u64).unwrap_or(VarInt::ZERO);
        let entry_size = if header.has_payload_length() {
            header.encoding_size() + payload_len_varint.encoding_size()
        } else {
            debug_assert_eq!(payload_len, 0);
            header.encoding_size()
        };
        let start = buf.len();
        buf.resize(start + entry_size, 0);
        let mut enc = EncoderBuffer::new(&mut buf[start..]);
        enc.encode(header);
        if header.has_payload_length() {
            enc.encode(&payload_len_varint);
        }
    }

    struct FrameSpec {
        header: Header,
        payload: Vec<u8>,
    }

    fn encode_frames(specs: &[FrameSpec]) -> (Vec<u8>, Vec<u8>) {
        let mut app_header = Vec::new();
        let mut payload = Vec::new();
        for spec in specs {
            let plen = if spec.header.has_payload_length() {
                spec.payload.len()
            } else {
                0
            };
            push_frame_metadata(&mut app_header, &spec.header, plen);
            if spec.header.has_payload_length() {
                payload.extend_from_slice(&spec.payload);
            }
        }
        (app_header, payload)
    }

    /// Collect decode_frames iterator results into (header, payload slice) pairs,
    /// pairing each metadata entry with the corresponding bytes from `payload`.
    fn collect_frames<'p>(
        app_header: &[u8],
        payload: &'p [u8],
    ) -> Result<Vec<(Header, &'p [u8])>, DecoderError> {
        let mut offset = 0usize;
        let mut result = Vec::new();
        for item in decode_frames(app_header) {
            let (header, payload_len) = item?;
            let end = offset + payload_len;
            assert!(end <= payload.len(), "payload underflow in test");
            result.push((header, &payload[offset..end]));
            offset = end;
        }
        assert_eq!(offset, payload.len(), "leftover payload bytes in test");
        Ok(result)
    }

    #[test]
    fn round_trip_single_flow_data() {
        let header = Header::FlowData {
            queue_pair: QueuePair {
                source_queue_id: VarInt::from_u8(1),
                dest_queue_id: VarInt::from_u8(2),
            },
            stream_id: VarInt::from_u8(42),
            offset: VarInt::ZERO,
            is_fin: false,
        };
        let data = b"hello world";
        let specs = [FrameSpec {
            header,
            payload: data.to_vec(),
        }];
        let (app_header, payload) = encode_frames(&specs);
        let frames = collect_frames(&app_header, &payload).unwrap();
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].0, header);
        assert_eq!(frames[0].1, data);
    }

    #[test]
    fn round_trip_multiple_frames() {
        let specs = vec![
            FrameSpec {
                header: Header::FlowData {
                    queue_pair: QueuePair {
                        source_queue_id: VarInt::from_u8(1),
                        dest_queue_id: VarInt::from_u8(2),
                    },
                    stream_id: VarInt::from_u8(10),
                    offset: VarInt::ZERO,
                    is_fin: false,
                },
                payload: b"stream10".to_vec(),
            },
            FrameSpec {
                header: Header::FlowReset {
                    dest_queue_id: VarInt::from_u8(3),
                    stream_id: VarInt::from_u8(20),
                    reset_target: ResetTarget::Both,
                    error_code: VarInt::from_u8(1),
                },
                payload: vec![],
            },
            FrameSpec {
                header: Header::FlowData {
                    queue_pair: QueuePair {
                        source_queue_id: VarInt::from_u8(4),
                        dest_queue_id: VarInt::from_u8(5),
                    },
                    stream_id: VarInt::from_u8(30),
                    offset: VarInt::from_u8(8),
                    is_fin: true,
                },
                payload: b"fin data".to_vec(),
            },
        ];

        let (app_header, payload) = encode_frames(&specs);
        let frames = collect_frames(&app_header, &payload).unwrap();

        assert_eq!(frames.len(), 3);
        assert_eq!(frames[0].0, specs[0].header);
        assert_eq!(frames[0].1, b"stream10");
        assert_eq!(frames[1].0, specs[1].header);
        assert_eq!(frames[1].1, b"");
        assert_eq!(frames[2].0, specs[2].header);
        assert_eq!(frames[2].1, b"fin data");
    }

    #[test]
    fn empty_application_header() {
        let frames: Vec<_> = decode_frames(&[]).collect();
        assert!(frames.is_empty());
    }

    #[test]
    fn iterator_yields_claimed_payload_len() {
        let header = Header::FlowData {
            queue_pair: QueuePair {
                source_queue_id: VarInt::ZERO,
                dest_queue_id: VarInt::ZERO,
            },
            stream_id: VarInt::ZERO,
            offset: VarInt::ZERO,
            is_fin: false,
        };
        // Encode a header claiming a 100-byte payload.
        // The iterator decodes only the metadata; payload consumption is the caller's job.
        let mut app_header = Vec::new();
        push_frame_metadata(&mut app_header, &header, 100);
        let frames: Vec<_> = decode_frames(&app_header)
            .collect::<Result<_, _>>()
            .expect("well-formed metadata must decode");
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].0, header);
        assert_eq!(frames[0].1, 100); // payload_len reported by iterator
    }
}

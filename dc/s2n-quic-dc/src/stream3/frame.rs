// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Frame: the application's unit of work for the reliable datagram pipeline.
//!
//! A Frame decouples the application's unit of work from the transport's unit of work (packets).
//! Writers produce Frames; the transport layer decides how to pack multiple Frames into packets,
//! encrypt them, and transmit them. For small payloads, many Frames pack into a single packet.
//! For large payloads, a single Frame fills an entire packet by itself.
//!
//! The Frame carries a Header (routing metadata), a payload, completion notification, TTL for
//! bounded retransmission, and a target transmission time for pacing. Transport-assigned fields
//! like source_sender_id and attempt_id live in the Header as mutable slots that the Peer
//! Context fills during packet assembly.

use crate::{
    byte_vec::ByteVec,
    clock::precision,
    datagram::batch::Priority,
    packet::datagram::{QueuePair, ResetTarget},
    path::secret::map::Entry as PathSecretEntry,
    socket::channel::{intrusive_queue::datagram_completion, ByteCost},
};
use s2n_codec::{decoder_invariant, Encoder, EncoderValue};
use s2n_quic_core::varint::VarInt;
use std::sync::Arc;

/// Default TTL for frames (number of transmission attempts before failure).
pub const DEFAULT_TTL: u8 = 10;

/// Completion channel sender typed on Frame.
pub type CompletionSender = datagram_completion::Sender<Frame>;

/// Completion channel receiver typed on Frame.
pub type CompletionReceiver = datagram_completion::Receiver<Frame>;

/// Submission channel sender typed on Frame.
pub type SubmissionSender =
    crate::socket::channel::intrusive_queue::sharded::Sender<crate::intrusive_queue::EntryAdapter<Frame>>;

/// Submission channel receiver typed on Frame.
pub type SubmissionReceiver =
    crate::socket::channel::intrusive_queue::sharded::Receiver<crate::intrusive_queue::EntryAdapter<Frame>>;

/// Creates a new frame submission channel.
///
/// `shard_count` must be a power of two. More shards reduce contention between concurrent senders
/// at the cost of receiver bookkeeping. A good default is the number of workers rounded up to the
/// next power of two, multiplied by a small constant (e.g. 4).
pub fn submission_channel(shard_count: usize) -> (SubmissionSender, SubmissionReceiver) {
    crate::socket::channel::intrusive_queue::sharded::new::<Frame>(shard_count)
}

/// Create a new completion channel for Frames.
pub fn completion_channel() -> CompletionReceiver {
    datagram_completion::new()
}

/// Status of a frame's transmission through the pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TransmissionStatus {
    /// Frame is pending transmission or in flight
    #[default]
    Pending,
    /// Frame was acknowledged by the peer
    Acknowledged,
    /// Frame failed to be delivered
    Failed(FailureReason),
}

/// Reasons why a frame might fail to be delivered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureReason {
    /// The peer was declared dead (PTO reached max idle time)
    PeerDead,
    /// Transmission error (peer active but refused packet)
    TransmissionError,
    /// Unknown path secret (path secret refused by peer)
    UnknownPathSecret,
    /// The sender was dropped and requested cancellation
    Cancelled,
}

/// Routing metadata for a Frame.
///
/// Describes what kind of frame this is and the per-frame routing fields. The
/// source_sender_id is NOT here — it lives on the Frame struct and gets stamped
/// into the packet header at encryption time by the peer context.
#[cfg_attr(test, derive(bolero_generator::TypeGenerator))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Header {
    /// Initialize a new flow with the server
    FlowInit {
        source_queue_id: VarInt,
        dest_acceptor_id: VarInt,
        attempt_id: VarInt,
        stream_id: VarInt,
        is_fin: bool,
    },
    /// Stream data routed via queue pair
    FlowData {
        queue_pair: QueuePair,
        stream_id: VarInt,
        offset: VarInt,
        is_fin: bool,
    },
    /// Flow control (MAX_DATA and other control frames)
    FlowControl {
        queue_pair: QueuePair,
        stream_id: VarInt,
    },
    /// Reset a flow
    FlowReset {
        dest_queue_id: VarInt,
        stream_id: VarInt,
        reset_target: ResetTarget,
        error_code: VarInt,
    },
    /// Client response to a FlowValidateRequest
    FlowInitValidate {
        queue_pair: QueuePair,
        attempt_id: VarInt,
        stream_id: VarInt,
    },
    /// Server challenge when deduplication can't be guaranteed
    FlowValidateRequest {
        dest_sender_id: VarInt,
        queue_pair: QueuePair,
        attempt_id: VarInt,
        stream_id: VarInt,
    },
    /// ACK frame routed to a specific remote sender
    Control { dest_sender_id: VarInt },
}

impl Header {
    const FLOW_INIT_TYPE: u8 = 1;
    const FLOW_VALIDATE_REQUEST_TYPE: u8 = 2;
    const FLOW_INIT_VALIDATE_TYPE: u8 = 3;
    const FLOW_DATA_NO_FIN_TYPE: u8 = 4;
    const FLOW_DATA_WITH_FIN_TYPE: u8 = 5;
    const FLOW_CONTROL_TYPE: u8 = 6;
    const FLOW_RESET_BOTH_TYPE: u8 = 7;
    const FLOW_INIT_WITH_FIN_TYPE: u8 = 8;
    const FLOW_RESET_STREAM_TYPE: u8 = 9;
    const FLOW_RESET_CONTROL_TYPE: u8 = 10;
    const CONTROL_TYPE: u8 = 13;

    #[inline]
    pub fn priority(&self) -> Priority {
        match self {
            Self::FlowInit { attempt_id, .. } => {
                if *attempt_id != VarInt::MAX {
                    Priority::FlowRetryReset
                } else {
                    Priority::FlowInit
                }
            }
            Self::FlowData { .. } => Priority::FlowData,
            Self::FlowControl { .. } => Priority::FlowControl,
            Self::FlowReset { .. }
            | Self::FlowInitValidate { .. }
            | Self::FlowValidateRequest { .. } => Priority::FlowRetryReset,
            Self::Control { .. } => Priority::Ack,
        }
    }

    /// Returns true if this header variant carries a per-frame payload length entry.
    ///
    /// This indicates whether the application header includes an explicit payload length for the
    /// frame, not whether the frame must contain non-empty payload bytes. Control and flow-control
    /// frames can legitimately encode a zero-length payload while still carrying the length field.
    #[inline]
    pub fn has_payload_length(&self) -> bool {
        match self {
            Self::FlowInit { .. }
            | Self::FlowData { .. }
            | Self::FlowControl { .. }
            | Self::Control { .. } => true,
            Self::FlowReset { .. }
            | Self::FlowInitValidate { .. }
            | Self::FlowValidateRequest { .. }
            => false,
        }
    }

    /// Returns the number of bytes this header occupies in the application header region,
    /// including the optional payload-length varint when [`has_payload_length`] is true.
    ///
    /// This is the single canonical implementation of the frame-metadata size calculation.
    /// `assemble::frame_metadata_len` delegates here so both callers operate on the same
    /// assumptions. Debug builds assert that header variants without a payload-length field
    /// always receive an empty payload.
    #[inline]
    pub fn metadata_len(&self, payload_len: usize) -> usize {
        if self.has_payload_length() {
            let payload_len_varint = VarInt::try_from(payload_len as u64).unwrap_or(VarInt::ZERO);
            self.encoding_size() + payload_len_varint.encoding_size()
        } else {
            debug_assert_eq!(
                payload_len, 0,
                "frames without payload_length must have zero payload"
            );
            self.encoding_size()
        }
    }
}

impl EncoderValue for Header {
    fn encode<E: Encoder>(&self, encoder: &mut E) {
        match self {
            Self::FlowInit {
                source_queue_id,
                dest_acceptor_id,
                attempt_id,
                stream_id,
                is_fin,
            } => {
                let tag = if *is_fin {
                    Self::FLOW_INIT_WITH_FIN_TYPE
                } else {
                    Self::FLOW_INIT_TYPE
                };
                encoder.encode(&tag);
                encoder.encode(source_queue_id);
                encoder.encode(dest_acceptor_id);
                encoder.encode(attempt_id);
                encoder.encode(stream_id);
            }
            Self::FlowValidateRequest {
                dest_sender_id,
                queue_pair,
                attempt_id,
                stream_id,
            } => {
                encoder.encode(&Self::FLOW_VALIDATE_REQUEST_TYPE);
                encoder.encode(dest_sender_id);
                encoder.encode(queue_pair);
                encoder.encode(attempt_id);
                encoder.encode(stream_id);
            }
            Self::FlowInitValidate {
                queue_pair,
                attempt_id,
                stream_id,
            } => {
                encoder.encode(&Self::FLOW_INIT_VALIDATE_TYPE);
                encoder.encode(queue_pair);
                encoder.encode(attempt_id);
                encoder.encode(stream_id);
            }
            Self::FlowData {
                queue_pair,
                stream_id,
                offset,
                is_fin,
            } => {
                let tag = if *is_fin {
                    Self::FLOW_DATA_WITH_FIN_TYPE
                } else {
                    Self::FLOW_DATA_NO_FIN_TYPE
                };
                encoder.encode(&tag);
                encoder.encode(queue_pair);
                encoder.encode(stream_id);
                encoder.encode(offset);
            }
            Self::FlowControl {
                queue_pair,
                stream_id,
            } => {
                encoder.encode(&Self::FLOW_CONTROL_TYPE);
                encoder.encode(queue_pair);
                encoder.encode(stream_id);
            }
            Self::FlowReset {
                dest_queue_id,
                stream_id,
                reset_target,
                error_code,
            } => {
                let reset_type = match reset_target {
                    ResetTarget::Both => Self::FLOW_RESET_BOTH_TYPE,
                    ResetTarget::Stream => Self::FLOW_RESET_STREAM_TYPE,
                    ResetTarget::Control => Self::FLOW_RESET_CONTROL_TYPE,
                };
                encoder.encode(&reset_type);
                encoder.encode(dest_queue_id);
                encoder.encode(stream_id);
                encoder.encode(error_code);
            }
            Self::Control { dest_sender_id } => {
                encoder.encode(&Self::CONTROL_TYPE);
                encoder.encode(dest_sender_id);
            }
        }
    }
}

impl<'a> s2n_codec::DecoderValue<'a> for Header {
    #[inline]
    fn decode(buffer: s2n_codec::DecoderBuffer<'a>) -> s2n_codec::DecoderBufferResult<'a, Self> {
        let (tag, buffer) = buffer.decode::<u8>()?;

        match tag {
            Self::FLOW_INIT_TYPE | Self::FLOW_INIT_WITH_FIN_TYPE => {
                let (source_queue_id, buffer) = buffer.decode()?;
                let (dest_acceptor_id, buffer) = buffer.decode()?;
                let (attempt_id, buffer) = buffer.decode()?;
                let (stream_id, buffer) = buffer.decode()?;
                let is_fin = tag == Self::FLOW_INIT_WITH_FIN_TYPE;
                Ok((
                    Self::FlowInit {
                        source_queue_id,
                        dest_acceptor_id,
                        attempt_id,
                        stream_id,
                        is_fin,
                    },
                    buffer,
                ))
            }
            Self::FLOW_VALIDATE_REQUEST_TYPE => {
                let (dest_sender_id, buffer) = buffer.decode()?;
                let (queue_pair, buffer) = buffer.decode()?;
                let (attempt_id, buffer) = buffer.decode()?;
                let (stream_id, buffer) = buffer.decode()?;
                Ok((
                    Self::FlowValidateRequest {
                        dest_sender_id,
                        queue_pair,
                        attempt_id,
                        stream_id,
                    },
                    buffer,
                ))
            }
            Self::FLOW_INIT_VALIDATE_TYPE => {
                let (queue_pair, buffer) = buffer.decode()?;
                let (attempt_id, buffer) = buffer.decode()?;
                let (stream_id, buffer) = buffer.decode()?;
                Ok((
                    Self::FlowInitValidate {
                        queue_pair,
                        attempt_id,
                        stream_id,
                    },
                    buffer,
                ))
            }
            Self::FLOW_DATA_NO_FIN_TYPE | Self::FLOW_DATA_WITH_FIN_TYPE => {
                let (queue_pair, buffer) = buffer.decode()?;
                let (stream_id, buffer) = buffer.decode()?;
                let (offset, buffer) = buffer.decode()?;
                let is_fin = tag == Self::FLOW_DATA_WITH_FIN_TYPE;
                Ok((
                    Self::FlowData {
                        queue_pair,
                        stream_id,
                        offset,
                        is_fin,
                    },
                    buffer,
                ))
            }
            Self::FLOW_CONTROL_TYPE => {
                let (queue_pair, buffer) = buffer.decode()?;
                let (stream_id, buffer) = buffer.decode()?;
                Ok((Self::FlowControl { queue_pair, stream_id }, buffer))
            }
            Self::FLOW_RESET_BOTH_TYPE
            | Self::FLOW_RESET_STREAM_TYPE
            | Self::FLOW_RESET_CONTROL_TYPE => {
                let reset_target = match tag {
                    Self::FLOW_RESET_BOTH_TYPE => ResetTarget::Both,
                    Self::FLOW_RESET_STREAM_TYPE => ResetTarget::Stream,
                    Self::FLOW_RESET_CONTROL_TYPE => ResetTarget::Control,
                    _ => unreachable!(),
                };
                let (dest_queue_id, buffer) = buffer.decode()?;
                let (stream_id, buffer) = buffer.decode()?;
                let (error_code, buffer) = buffer.decode()?;
                Ok((
                    Self::FlowReset {
                        dest_queue_id,
                        stream_id,
                        reset_target,
                        error_code,
                    },
                    buffer,
                ))
            }
            Self::CONTROL_TYPE => {
                let (dest_sender_id, buffer) = buffer.decode()?;
                Ok((Self::Control { dest_sender_id }, buffer))
            }
            _ => {
                decoder_invariant!(false, "unknown frame header type");
                Err(s2n_codec::DecoderError::InvariantViolation(
                    "unknown frame header type",
                ))
            }
        }
    }
}

/// A frame submitted by application-level components (Writers, control message senders).
///
/// This is the universal unit of work in the frame aggregation architecture. The transport
/// layer aggregates multiple Frames into single encrypted packets to amortize per-packet costs.
///
/// The same Frame moves through different intrusive queues during its lifecycle (submission,
/// wheel, peer context, packet_number_map, completion) without boxing/unboxing.
pub struct Frame {
    /// Routing metadata for this frame
    pub header: Header,
    /// Source sender ID for the packet header. VarInt::MAX means no preference (round-robin).
    /// When set to a specific value, the frame is sticky-routed to that send socket.
    pub source_sender_id: VarInt,
    /// Payload data (stream bytes for FlowData, control frame bytes for FlowControl,
    /// ACK frames for Control, empty for resets)
    pub payload: ByteVec,
    /// Path secret entry identifying the destination peer.
    ///
    /// Used by the wheel to group frames by peer (Arc pointer comparison) and by the
    /// Peer Context to obtain crypto state for encryption.
    pub path_secret_entry: Arc<PathSecretEntry>,
    /// Completion notification sender. When the frame is acknowledged (or fails), the
    /// completion fires to notify the Writer so it can free inflight budget.
    ///
    /// Also provides cancellation: `completion.should_transmit()` returns false when the
    /// Writer has been dropped or the stream cancelled, causing the transport to skip
    /// this frame rather than transmitting it.
    pub completion: Option<CompletionSender>,
    /// Current transmission status (updated by the pipeline on ACK or loss)
    pub status: TransmissionStatus,
    /// Remaining transmission attempts. Decremented on each retransmission.
    /// When zero, the frame completes with failure instead of being retransmitted.
    pub ttl: u8,
    /// Target transmission time for pacing. Writers assign times at 1us granularity to
    /// interleave fairly with frames from other streams rather than forming bursts.
    /// Advisory — actual pacing happens at the Peer Context level.
    pub transmission_time: Option<precision::Timestamp>,
}

impl Frame {
    #[inline]
    pub fn priority(&self) -> Priority {
        self.header.priority()
    }

    #[inline]
    pub fn requires_sticky_sender(&self) -> bool {
        self.source_sender_id != VarInt::MAX
    }

    #[inline]
    pub fn peer_addr(&self) -> std::net::SocketAddr {
        self.path_secret_entry.data_addr()
    }

    #[inline]
    pub fn payload_len(&self) -> usize {
        self.payload.len()
    }

    /// Returns true if this frame should still be transmitted.
    ///
    /// Delegates to the completion sender's cancellation flag. Returns true if there's no
    /// completion sender (best-effort frames are always transmittable).
    #[inline]
    pub fn should_transmit(&self) -> bool {
        self.completion
            .as_ref()
            .map_or(true, |c| c.should_transmit())
    }
}

impl ByteCost for Frame {
    /// Returns the total wire cost of this frame: payload bytes plus the header metadata
    /// (type tag, routing fields, and optional payload-length varint).
    ///
    /// Used by `send::Context::pending_bytes` to track in-queue load without traversal,
    /// and by the pick-two load balancer via `publish_next_transmission_time`.
    #[inline]
    fn byte_cost(&self) -> u64 {
        let payload_len = self.payload.len();
        (payload_len + self.header.metadata_len(payload_len)) as u64
    }
}

impl std::fmt::Debug for Frame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Frame")
            .field("header", &self.header)
            .field("payload_len", &self.payload.len())
            .field("peer", &self.path_secret_entry.data_addr())
            .field("ttl", &self.ttl)
            .field("transmission_time", &self.transmission_time)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_data() {
        let entry = PathSecretEntry::fake("127.0.0.1:8080".parse().unwrap(), None);
        let mut payload = ByteVec::new();
        payload.push_back(bytes::Bytes::from_static(b"hello"));

        let frame = Frame {
            header: Header::FlowData {
                queue_pair: QueuePair {
                    source_queue_id: VarInt::from_u8(1),
                    dest_queue_id: VarInt::from_u8(2),
                },
                stream_id: VarInt::from_u8(42),
                offset: VarInt::ZERO,
                is_fin: false,
            },
            source_sender_id: VarInt::MAX,
            payload,
            path_secret_entry: entry,
            completion: None,
            status: TransmissionStatus::default(),
            ttl: DEFAULT_TTL,
            transmission_time: None,
        };

        assert_eq!(frame.payload_len(), 5);
        assert_eq!(frame.ttl, DEFAULT_TTL);
        assert!(frame.should_transmit());
        assert_eq!(frame.priority(), Priority::FlowData);
        assert!(!frame.requires_sticky_sender());
    }

    #[test]
    fn flow_init_priority() {
        let entry = PathSecretEntry::fake("127.0.0.1:8080".parse().unwrap(), None);

        let frame = Frame {
            header: Header::FlowInit {
                source_queue_id: VarInt::from_u8(1),
                dest_acceptor_id: VarInt::from_u8(10),
                attempt_id: VarInt::MAX,
                stream_id: VarInt::from_u8(42),
                is_fin: false,
            },
            source_sender_id: VarInt::MAX,
            payload: ByteVec::new(),
            path_secret_entry: entry,
            completion: None,
            status: TransmissionStatus::default(),
            ttl: DEFAULT_TTL,
            transmission_time: None,
        };

        assert_eq!(frame.priority(), Priority::FlowInit);
        assert!(!frame.requires_sticky_sender());
    }

    #[test]
    fn flow_reset() {
        let entry = PathSecretEntry::fake("10.0.0.1:9000".parse().unwrap(), None);

        let frame = Frame {
            header: Header::FlowReset {
                dest_queue_id: VarInt::from_u8(3),
                stream_id: VarInt::from_u8(42),
                reset_target: ResetTarget::Both,
                error_code: VarInt::from_u8(1),
            },
            source_sender_id: VarInt::MAX,
            payload: ByteVec::new(),
            path_secret_entry: entry,
            completion: None,
            status: TransmissionStatus::default(),
            ttl: DEFAULT_TTL,
            transmission_time: None,
        };

        assert_eq!(frame.priority(), Priority::FlowRetryReset);
        assert!(!frame.requires_sticky_sender());
        assert_eq!(frame.payload_len(), 0);
        assert_eq!(frame.peer_addr(), "10.0.0.1:9000".parse().unwrap());
    }

    #[test]
    fn control() {
        let entry = PathSecretEntry::fake("10.0.0.1:9000".parse().unwrap(), None);
        let mut ack_data = ByteVec::new();
        ack_data.push_back(bytes::Bytes::from_static(b"\x02\x00\x00"));

        let frame = Frame {
            header: Header::Control {
                dest_sender_id: VarInt::from_u8(5),
            },
            source_sender_id: VarInt::MAX,
            payload: ack_data,
            path_secret_entry: entry,
            completion: None,
            status: TransmissionStatus::default(),
            ttl: DEFAULT_TTL,
            transmission_time: None,
        };

        assert_eq!(frame.priority(), Priority::Ack);
        assert!(!frame.requires_sticky_sender());
        assert_eq!(frame.payload_len(), 3);
    }

    #[test]
    fn sticky_sender_after_assignment() {
        let entry = PathSecretEntry::fake("127.0.0.1:8080".parse().unwrap(), None);

        let frame = Frame {
            header: Header::FlowInit {
                source_queue_id: VarInt::from_u8(1),
                dest_acceptor_id: VarInt::from_u8(10),
                attempt_id: VarInt::from_u8(0),
                stream_id: VarInt::from_u8(42),
                is_fin: false,
            },
            source_sender_id: VarInt::from_u8(7),
            payload: ByteVec::new(),
            path_secret_entry: entry,
            completion: None,
            status: TransmissionStatus::default(),
            ttl: DEFAULT_TTL,
            transmission_time: None,
        };

        assert!(frame.requires_sticky_sender());
        assert_eq!(frame.priority(), Priority::FlowRetryReset);
    }
}

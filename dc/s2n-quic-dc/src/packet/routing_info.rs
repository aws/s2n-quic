// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use s2n_quic_core::varint::VarInt;

/// Routing information for packets.
///
/// This enum indicates how the packet should be routed to its destination.
/// When a packet tag's `has_routing_info` is false, this is implicitly `None`.
/// When true, the routing type and fields are encoded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoutingInfo {
    /// No routing information - packet is routed based on socket address only.
    /// This variant is never encoded; it's represented by has_routing_info=false in the tag.
    None,
    /// Route to specific queue pair (e.g., for multiplexing multiple streams)
    QueuePair {
        source_sender_id: VarInt,
        source_queue_id: VarInt,
        dest_queue_id: VarInt,
    },
    /// Route to a specific sender socket by global sender ID
    /// Used for control packets (ACKs) to route back to the originating socket
    SenderId { sender_id: VarInt },
}

impl Default for RoutingInfo {
    fn default() -> Self {
        Self::None
    }
}

impl RoutingInfo {
    const QUEUE_PAIR_TYPE: u8 = 0;
    const SENDER_ID_TYPE: u8 = 1;

    /// Get the source sender ID for data packets (used for ACK routing)
    pub fn source_sender_id(&self) -> Option<VarInt> {
        match self {
            Self::None => None,
            Self::QueuePair {
                source_sender_id, ..
            } => Some(*source_sender_id),
            Self::SenderId { .. } => None,
        }
    }

    /// Update the source sender ID in routing info
    pub fn with_source_sender_id(&self, source_sender_id: VarInt) -> Self {
        match self {
            Self::QueuePair {
                source_sender_id: _,
                source_queue_id,
                dest_queue_id,
            } => Self::QueuePair {
                source_sender_id,
                source_queue_id: *source_queue_id,
                dest_queue_id: *dest_queue_id,
            },
            _ => *self,
        }
    }
}

impl s2n_codec::EncoderValue for RoutingInfo {
    #[inline]
    fn encode<E: s2n_codec::Encoder>(&self, encoder: &mut E) {
        match self {
            Self::None => {
                // None is not encoded - indicated by has_routing_info=false in tag
            }
            Self::QueuePair {
                source_sender_id,
                source_queue_id,
                dest_queue_id,
            } => {
                encoder.encode(&VarInt::from_u8(Self::QUEUE_PAIR_TYPE));
                encoder.encode(source_sender_id);
                encoder.encode(source_queue_id);
                encoder.encode(dest_queue_id);
            }
            Self::SenderId { sender_id } => {
                encoder.encode(&VarInt::from_u8(Self::SENDER_ID_TYPE));
                encoder.encode(sender_id);
            }
        }
    }
}

impl<'a> s2n_codec::DecoderValue<'a> for RoutingInfo {
    #[inline]
    fn decode(buffer: s2n_codec::DecoderBuffer<'a>) -> s2n_codec::DecoderBufferResult<'a, Self> {
        use s2n_codec::decoder_invariant;
        use s2n_quic_core::varint::VarInt;

        let (routing_type, buffer) = buffer.decode::<VarInt>()?;
        let routing_type = routing_type.as_u64();

        match routing_type {
            0 => {
                let (source_sender_id, buffer) = buffer.decode()?;
                let (source_queue_id, buffer) = buffer.decode()?;
                let (dest_queue_id, buffer) = buffer.decode()?;
                Ok((
                    Self::QueuePair {
                        source_sender_id,
                        source_queue_id,
                        dest_queue_id,
                    },
                    buffer,
                ))
            }
            1 => {
                let (sender_id, buffer) = buffer.decode()?;
                Ok((Self::SenderId { sender_id }, buffer))
            }
            _ => {
                decoder_invariant!(false, "unknown routing info type");
                Err(s2n_codec::DecoderError::InvariantViolation(
                    "unknown routing info type",
                ))
            }
        }
    }
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use s2n_quic_core::varint::VarInt;

/// Target for a flow reset operation
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(bolero_generator::TypeGenerator))]
pub enum ResetTarget {
    /// Reset both stream and control halves
    Both,
    /// Reset only the stream half
    Stream,
    /// Reset only the control half
    Control,
}

/// Routing information for datagram packets.
///
/// This enum indicates how the packet should be routed to its destination.
/// When a packet tag's `has_routing_info` is false, this is implicitly `None`.
/// When true, the routing type and fields are encoded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(bolero_generator::TypeGenerator))]
#[derive(Default)]
pub enum RoutingInfo {
    /// No routing information - packet is routed based on socket address only.
    /// This variant is never encoded; it's represented by has_routing_info=false in the tag.
    #[default]
    None,
    /// Identifies the source sender for aggregated frame packets.
    ///
    /// Used when a packet contains multiple frames from different streams that share
    /// only the source sender identity. Per-frame routing metadata (including ACK frames
    /// with their dest_sender_id) is encoded in the application header region.
    SenderId { source_sender_id: VarInt },
}

/// Queue pair used in frame-level headers for stream routing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(bolero_generator::TypeGenerator))]
pub struct QueuePair {
    /// The queue ID used for routing messages to the sender
    pub source_queue_id: VarInt,
    /// The queue ID used for routing messages to the receiver
    pub dest_queue_id: VarInt,
}

impl QueuePair {
    /// Returns a new QueuePair with source and dest swapped
    #[inline]
    pub fn reverse(self) -> Self {
        Self {
            source_queue_id: self.dest_queue_id,
            dest_queue_id: self.source_queue_id,
        }
    }
}

impl s2n_codec::EncoderValue for QueuePair {
    #[inline]
    fn encode<E: s2n_codec::Encoder>(&self, encoder: &mut E) {
        encoder.encode(&self.source_queue_id);
        encoder.encode(&self.dest_queue_id);
    }
}

impl<'a> s2n_codec::DecoderValue<'a> for QueuePair {
    #[inline]
    fn decode(buffer: s2n_codec::DecoderBuffer<'a>) -> s2n_codec::DecoderBufferResult<'a, Self> {
        let (source_queue_id, buffer) = buffer.decode()?;
        let (dest_queue_id, buffer) = buffer.decode()?;
        let pair = Self {
            source_queue_id,
            dest_queue_id,
        };
        Ok((pair, buffer))
    }
}

impl RoutingInfo {
    const SENDER_ID_TYPE: u8 = 11;

    pub fn source_sender_id(&self) -> Option<VarInt> {
        match self {
            Self::None => None,
            Self::SenderId {
                source_sender_id, ..
            } => Some(*source_sender_id),
        }
    }

    pub fn set_source_sender_id(&mut self, new_source_sender_id: VarInt) {
        match self {
            Self::None => {}
            Self::SenderId {
                source_sender_id, ..
            } => {
                *source_sender_id = new_source_sender_id;
            }
        }
    }
}

impl s2n_codec::EncoderValue for RoutingInfo {
    #[inline]
    fn encode<E: s2n_codec::Encoder>(&self, encoder: &mut E) {
        match self {
            Self::None => {}
            Self::SenderId { source_sender_id } => {
                encoder.encode(&Self::SENDER_ID_TYPE);
                encoder.encode(source_sender_id);
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
        let routing_type: u8 = routing_type
            .try_into()
            .map_err(|_| s2n_codec::DecoderError::InvariantViolation("unexpected routing type"))?;

        match routing_type {
            Self::SENDER_ID_TYPE => {
                let (source_sender_id, buffer) = buffer.decode()?;
                let header = Self::SenderId { source_sender_id };
                Ok((header, buffer))
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

#[cfg(test)]
mod tests {
    use super::*;
    use s2n_codec::Encoder;

    #[test]
    fn round_trip_queue_pair() {
        bolero::check!().with_type::<QueuePair>().for_each(|value| {
            let mut buffer = [0u8; 32];
            let len = {
                let mut encoder = s2n_codec::EncoderBuffer::new(&mut buffer);
                encoder.encode(value);
                encoder.len()
            };

            let buffer = s2n_codec::DecoderBuffer::new(&buffer[..len]);
            let (decoded, remaining) = buffer.decode::<QueuePair>().unwrap();
            assert_eq!(value, &decoded);
            assert!(remaining.is_empty());
        });
    }

    #[test]
    fn round_trip_routing_info() {
        bolero::check!()
            .with_type::<RoutingInfo>()
            .for_each(|value| {
                if matches!(value, RoutingInfo::None) {
                    return;
                }

                let mut buffer = [0u8; 128];
                let len = {
                    let mut encoder = s2n_codec::EncoderBuffer::new(&mut buffer);
                    encoder.encode(value);
                    encoder.len()
                };

                let buffer = s2n_codec::DecoderBuffer::new(&buffer[..len]);
                let (decoded, remaining) = buffer.decode::<RoutingInfo>().unwrap();
                assert_eq!(value, &decoded);
                assert!(remaining.is_empty());
            });
    }
}

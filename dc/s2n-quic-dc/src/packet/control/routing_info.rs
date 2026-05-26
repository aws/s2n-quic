// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use s2n_quic_core::varint::VarInt;

/// Routing information for control packets.
///
/// Control packets (ACKs) use sender-based routing to route back to the originating socket.
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
    /// Route to a specific sender socket by global sender ID
    /// Used for control packets (ACKs) to route back to the originating socket
    Sender {
        /// The sender ID used to transmit the actual packet
        source_sender_id: VarInt,
        /// The destination of the packet
        dest_sender_id: VarInt,
    },
}

impl RoutingInfo {
    const SENDER_TYPE: u8 = 0;

    /// Get the source sender ID (used for ACK routing)
    pub fn source_sender_id(&self) -> Option<VarInt> {
        match self {
            Self::None => None,
            Self::Sender {
                source_sender_id, ..
            } => Some(*source_sender_id),
        }
    }

    /// Get the destination sender ID (used for routing to the correct sender)
    pub fn dest_sender_id(&self) -> Option<VarInt> {
        match self {
            Self::Sender { dest_sender_id, .. } => Some(*dest_sender_id),
            _ => None,
        }
    }

    /// Update the source sender ID in routing info
    pub fn with_source_sender_id(&self, source_sender_id: VarInt) -> Self {
        match self {
            Self::None => Self::None,
            Self::Sender {
                source_sender_id: _,
                dest_sender_id,
            } => Self::Sender {
                source_sender_id,
                dest_sender_id: *dest_sender_id,
            },
        }
    }

    /// Returns true if this control packet requires sticky sender routing.
    ///
    /// When source_sender_id is set to a value other than VarInt::MAX (the
    /// sentinel for "no preference"), it indicates a precomputed target send socket.
    pub fn requires_sticky_sender_id(&self) -> bool {
        match self {
            Self::Sender {
                source_sender_id, ..
            } => *source_sender_id != VarInt::MAX,
            Self::None => false,
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
            Self::Sender {
                source_sender_id,
                dest_sender_id,
            } => {
                encoder.encode(&Self::SENDER_TYPE);
                encoder.encode(source_sender_id);
                encoder.encode(dest_sender_id);
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
            Self::SENDER_TYPE => {
                let (source_sender_id, buffer) = buffer.decode()?;
                let (dest_sender_id, buffer) = buffer.decode()?;
                let header = Self::Sender {
                    source_sender_id,
                    dest_sender_id,
                };
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
    fn round_trip_routing_info() {
        bolero::check!()
            .with_type::<RoutingInfo>()
            .for_each(|value| {
                // Skip None variant since it's not encoded
                if matches!(value, RoutingInfo::None) {
                    return;
                }

                let mut buffer = [0u8; 32];
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

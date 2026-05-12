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
pub enum RoutingInfo {
    /// No routing information - packet is routed based on socket address only.
    /// This variant is never encoded; it's represented by has_routing_info=false in the tag.
    None,
    /// Sent by the client to initialize a flow
    FlowInit {
        /// The sender ID used to transmit the actual packet
        source_sender_id: VarInt,
        /// The queue ID used for routing future messages to the client
        source_queue_id: VarInt,
        /// The acceptor responsible for handling the init request
        dest_acceptor_id: VarInt,
        /// Sender-local monotonic identifier for deduplication
        ///
        /// This is used by the server's sliding window to detect duplicate FlowInit
        /// packets during retransmission. Scoped to (credentials, source_sender_id).
        attempt_id: VarInt,
        /// Global client-wide stream identifier for validation
        ///
        /// Once established, this flow will validate that all subsequent packets
        /// present the same (credential_id, stream_id) pair.
        stream_id: VarInt,
        /// Indicates the flow is ended here
        is_fin: bool,
    },
    /// Send by the server to request a flow validation from the client
    ///
    /// This is used any time a server cannot guarantee a FlowInit packet has
    /// not been seen before.
    FlowValidateRequest {
        /// The sender ID used to transmit the actual packet
        source_sender_id: VarInt,
        /// The destination sender ID from the original FlowInit (for client to route response)
        dest_sender_id: VarInt,
        /// Queue routing info
        queue_pair: QueuePair,
        /// Echoed attempt_id from FlowInit for client validation
        attempt_id: VarInt,
        /// Echoed stream_id from FlowInit for client validation
        stream_id: VarInt,
    },
    /// Sent by the client to respond to a FlowValidateRequest
    FlowInitValidate {
        /// The sender ID used to transmit the actual packet
        source_sender_id: VarInt,
        /// Queue routing info
        queue_pair: QueuePair,
        /// Same attempt_id as original FlowInit (proves freshness)
        attempt_id: VarInt,
        /// Same stream_id as original FlowInit (establishes flow identity)
        stream_id: VarInt,
    },
    /// Route stream data to specific queue pair
    ///
    /// This is used for stream data and is the most common case
    FlowData {
        /// The sender ID used to transmit the actual packet
        source_sender_id: VarInt,
        /// Queue routing info
        queue_pair: QueuePair,
        /// Identifies which stream the data is for
        stream_id: VarInt,
        /// The stream offset that the data writes into
        offset: VarInt,
        /// Indicates the flow is ended here
        is_fin: bool,
    },
    /// Route control data to specific queue pair
    FlowControl {
        /// The sender ID used to transmit the actual packet
        source_sender_id: VarInt,
        /// Queue routing info
        queue_pair: QueuePair,
        stream_id: VarInt,
    },
    /// Sent when no stream state is associated with the local queue
    FlowReset {
        /// The sender ID used to transmit the actual packet
        source_sender_id: VarInt,
        /// The destination queue ID to reset
        dest_queue_id: VarInt,
        /// Identifies which stream is being rejected
        stream_id: VarInt,
        /// Which half(s) of the flow to reset
        reset_target: ResetTarget,
        /// Reason for rejection
        error_code: VarInt,
    },
    /// Identifies the source sender for aggregated frame packets.
    ///
    /// Used when a packet contains multiple frames from different streams that share
    /// only the source sender identity. Per-frame routing metadata (including ACK frames
    /// with their dest_sender_id) is encoded in the application header region.
    SenderId { source_sender_id: VarInt },
}

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

impl Default for RoutingInfo {
    fn default() -> Self {
        Self::None
    }
}

impl RoutingInfo {
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
    const SENDER_ID_TYPE: u8 = 11;
    const SENDER_PAIR_TYPE: u8 = 12;

    /// Get the source sender ID for data packets (used for ACK routing)
    pub fn source_sender_id(&self) -> Option<VarInt> {
        match self {
            Self::None => None,
            Self::FlowInit {
                source_sender_id, ..
            } => Some(*source_sender_id),
            Self::FlowValidateRequest {
                source_sender_id, ..
            } => Some(*source_sender_id),
            Self::FlowInitValidate {
                source_sender_id, ..
            } => Some(*source_sender_id),
            Self::FlowData {
                source_sender_id, ..
            } => Some(*source_sender_id),
            Self::FlowControl {
                source_sender_id, ..
            } => Some(*source_sender_id),
            Self::FlowReset {
                source_sender_id, ..
            } => Some(*source_sender_id),
            Self::SenderId {
                source_sender_id, ..
            } => Some(*source_sender_id),
        }
    }

    /// Returns true if this packet requires sticky sender_id routing (non-sentinel value)
    ///
    /// FlowInit and FlowInitValidate packets with a specific sender_id (not VarInt::MAX)
    /// must be routed to that specific sender. Packets with VarInt::MAX can be
    /// randomly distributed during initial submission.
    pub fn requires_sticky_sender_id(&self) -> bool {
        match self {
            Self::FlowInit {
                source_sender_id, ..
            }
            | Self::FlowInitValidate {
                source_sender_id, ..
            }
            | Self::FlowValidateRequest {
                source_sender_id, ..
            } => *source_sender_id != VarInt::MAX,
            _ => false,
        }
    }

    /// Fill in the attempt ID for FlowInit and FlowInitValidate packets
    ///
    /// The attempt_id is a monotonically increasing per-sender identifier used for
    /// deduplication on the server side. This method increments the counter if the
    /// packet has the sentinel value (VarInt::MAX).
    pub fn set_attempt_id(&mut self, attempt_id_counter: &mut VarInt) {
        match self {
            Self::FlowInit { attempt_id, .. } | Self::FlowInitValidate { attempt_id, .. } => {
                if *attempt_id == VarInt::MAX {
                    // Sentinel value - allocate a new attempt_id
                    *attempt_id = *attempt_id_counter;
                    *attempt_id_counter += 1;
                }
            }
            // Other packet types don't have attempt_id
            _ => {}
        }
    }

    /// Validate that sentinel values have been filled before encoding
    ///
    /// This should be called just before encoding to ensure all required fields
    /// have been populated. In debug builds, this will panic if sentinel values remain.
    pub fn before_encode(&self) {
        #[cfg(debug_assertions)]
        match self {
            Self::None => {}
            Self::FlowInit {
                source_sender_id,
                attempt_id,
                ..
            } => {
                debug_assert_ne!(
                    *source_sender_id,
                    VarInt::MAX,
                    "FlowInit source_sender_id must be filled before encoding"
                );
                debug_assert_ne!(
                    *attempt_id,
                    VarInt::MAX,
                    "FlowInit attempt_id must be filled before encoding"
                );
            }
            Self::FlowValidateRequest {
                source_sender_id, ..
            } => {
                debug_assert_ne!(
                    *source_sender_id,
                    VarInt::MAX,
                    "FlowValidateRequest source_sender_id must be filled before encoding"
                );
            }
            Self::FlowInitValidate {
                source_sender_id,
                attempt_id,
                ..
            } => {
                debug_assert_ne!(
                    *source_sender_id,
                    VarInt::MAX,
                    "FlowInitValidate source_sender_id must be filled before encoding"
                );
                debug_assert_ne!(
                    *attempt_id,
                    VarInt::MAX,
                    "FlowInitValidate attempt_id must be filled before encoding"
                );
            }
            Self::FlowData {
                source_sender_id, ..
            } => {
                debug_assert_ne!(
                    *source_sender_id,
                    VarInt::MAX,
                    "FlowData source_sender_id must be filled before encoding"
                );
            }
            Self::FlowControl {
                source_sender_id, ..
            } => {
                debug_assert_ne!(
                    *source_sender_id,
                    VarInt::MAX,
                    "FlowControl source_sender_id must be filled before encoding"
                );
            }
            Self::FlowReset {
                source_sender_id, ..
            } => {
                debug_assert_ne!(
                    *source_sender_id,
                    VarInt::MAX,
                    "FlowReset source_sender_id must be filled before encoding"
                );
            }
            Self::SenderId {
                source_sender_id, ..
            } => {
                debug_assert_ne!(
                    *source_sender_id,
                    VarInt::MAX,
                    "SenderId source_sender_id must be filled before encoding"
                );
            }
        }
    }

    /// Set the source sender ID in place (mutates self)
    ///
    /// For FlowInit and FlowInitValidate packets, this enforces stickiness with debug assertions.
    pub(crate) fn set_source_sender_id(&mut self, new_source_sender_id: VarInt) {
        match self {
            Self::None => {}
            // Sticky packets - enforce that sender_id doesn't change after first assignment
            Self::FlowInit {
                source_sender_id, ..
            }
            | Self::FlowInitValidate {
                source_sender_id, ..
            }
            | Self::FlowValidateRequest {
                source_sender_id, ..
            } => {
                debug_assert!(
                    *source_sender_id == VarInt::MAX || *source_sender_id == new_source_sender_id,
                    "Sticky packet sender_id mismatch: existing={:?}, new={:?}",
                    source_sender_id,
                    new_source_sender_id
                );
                *source_sender_id = new_source_sender_id;
            }
            // Non-sticky packets - can change sender_id freely
            Self::FlowData {
                source_sender_id, ..
            }
            | Self::FlowControl {
                source_sender_id, ..
            }
            | Self::FlowReset {
                source_sender_id, ..
            }
            | Self::SenderId {
                source_sender_id, ..
            } => {
                *source_sender_id = new_source_sender_id;
            }
        }
    }

    /// Update the source sender ID in routing info
    ///
    /// For FlowInit and FlowInitValidate packets (sticky packets), this updates self in place
    /// and returns a copy. For other packet types, returns a copy with the new sender_id.
    pub fn with_source_sender_id(&mut self, new_source_sender_id: VarInt) -> Self {
        // Check if this is a sticky packet type
        let is_sticky = matches!(self, Self::FlowInit { .. } | Self::FlowInitValidate { .. });

        if is_sticky {
            // Sticky packets: update in place and return copy
            self.set_source_sender_id(new_source_sender_id);
            *self
        } else {
            // Non-sticky packets: create copy, update it, and return
            let mut copy = *self;
            copy.set_source_sender_id(new_source_sender_id);
            copy
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
            Self::FlowInit {
                source_sender_id,
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
                encoder.encode(source_sender_id);
                encoder.encode(source_queue_id);
                encoder.encode(dest_acceptor_id);
                encoder.encode(attempt_id);
                encoder.encode(stream_id);
            }
            Self::FlowValidateRequest {
                source_sender_id,
                dest_sender_id,
                queue_pair,
                attempt_id,
                stream_id,
            } => {
                encoder.encode(&Self::FLOW_VALIDATE_REQUEST_TYPE);
                encoder.encode(source_sender_id);
                encoder.encode(dest_sender_id);
                encoder.encode(queue_pair);
                encoder.encode(attempt_id);
                encoder.encode(stream_id);
            }
            Self::FlowInitValidate {
                source_sender_id,
                queue_pair,
                attempt_id,
                stream_id,
            } => {
                encoder.encode(&Self::FLOW_INIT_VALIDATE_TYPE);
                encoder.encode(source_sender_id);
                encoder.encode(queue_pair);
                encoder.encode(attempt_id);
                encoder.encode(stream_id);
            }
            Self::FlowData {
                source_sender_id,
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
                encoder.encode(source_sender_id);
                encoder.encode(queue_pair);
                encoder.encode(stream_id);
                encoder.encode(offset);
            }
            Self::FlowControl {
                source_sender_id,
                queue_pair,
                stream_id,
            } => {
                encoder.encode(&Self::FLOW_CONTROL_TYPE);
                encoder.encode(source_sender_id);
                encoder.encode(queue_pair);
                encoder.encode(stream_id);
            }
            Self::FlowReset {
                source_sender_id,
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
                encoder.encode(source_sender_id);
                encoder.encode(dest_queue_id);
                encoder.encode(stream_id);
                encoder.encode(error_code);
            }
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
            Self::FLOW_INIT_TYPE | Self::FLOW_INIT_WITH_FIN_TYPE => {
                let (source_sender_id, buffer) = buffer.decode()?;
                let (source_queue_id, buffer) = buffer.decode()?;
                let (dest_acceptor_id, buffer) = buffer.decode()?;
                let (attempt_id, buffer) = buffer.decode()?;
                let (stream_id, buffer) = buffer.decode()?;
                let is_fin = routing_type == Self::FLOW_INIT_WITH_FIN_TYPE;
                let header = Self::FlowInit {
                    source_sender_id,
                    source_queue_id,
                    dest_acceptor_id,
                    attempt_id,
                    stream_id,
                    is_fin,
                };
                Ok((header, buffer))
            }
            Self::FLOW_VALIDATE_REQUEST_TYPE => {
                let (source_sender_id, buffer) = buffer.decode()?;
                let (dest_sender_id, buffer) = buffer.decode()?;
                let (queue_pair, buffer) = buffer.decode()?;
                let (attempt_id, buffer) = buffer.decode()?;
                let (stream_id, buffer) = buffer.decode()?;
                let header = Self::FlowValidateRequest {
                    source_sender_id,
                    dest_sender_id,
                    queue_pair,
                    attempt_id,
                    stream_id,
                };
                Ok((header, buffer))
            }
            Self::FLOW_INIT_VALIDATE_TYPE => {
                let (source_sender_id, buffer) = buffer.decode()?;
                let (queue_pair, buffer) = buffer.decode()?;
                let (attempt_id, buffer) = buffer.decode()?;
                let (stream_id, buffer) = buffer.decode()?;
                let header = Self::FlowInitValidate {
                    source_sender_id,
                    queue_pair,
                    attempt_id,
                    stream_id,
                };
                Ok((header, buffer))
            }
            Self::FLOW_DATA_WITH_FIN_TYPE | Self::FLOW_DATA_NO_FIN_TYPE => {
                let (source_sender_id, buffer) = buffer.decode()?;
                let (queue_pair, buffer) = buffer.decode()?;
                let (stream_id, buffer) = buffer.decode()?;
                let (offset, buffer) = buffer.decode()?;
                let is_fin = routing_type == Self::FLOW_DATA_WITH_FIN_TYPE;
                let header = Self::FlowData {
                    source_sender_id,
                    queue_pair,
                    stream_id,
                    offset,
                    is_fin,
                };
                Ok((header, buffer))
            }
            Self::FLOW_CONTROL_TYPE => {
                let (source_sender_id, buffer) = buffer.decode()?;
                let (queue_pair, buffer) = buffer.decode()?;
                let (stream_id, buffer) = buffer.decode()?;
                let header = Self::FlowControl {
                    source_sender_id,
                    queue_pair,
                    stream_id,
                };
                Ok((header, buffer))
            }
            Self::FLOW_RESET_BOTH_TYPE
            | Self::FLOW_RESET_STREAM_TYPE
            | Self::FLOW_RESET_CONTROL_TYPE => {
                let reset_target = match routing_type {
                    Self::FLOW_RESET_BOTH_TYPE => ResetTarget::Both,
                    Self::FLOW_RESET_STREAM_TYPE => ResetTarget::Stream,
                    Self::FLOW_RESET_CONTROL_TYPE => ResetTarget::Control,
                    _ => unreachable!(),
                };
                let (source_sender_id, buffer) = buffer.decode()?;
                let (dest_queue_id, buffer) = buffer.decode()?;
                let (stream_id, buffer) = buffer.decode()?;
                let (error_code, buffer) = buffer.decode()?;
                let header = Self::FlowReset {
                    source_sender_id,
                    dest_queue_id,
                    stream_id,
                    reset_target,
                    error_code,
                };
                Ok((header, buffer))
            }
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
                // Skip None variant since it's not encoded
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

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::fmt;
use s2n_codec::{decoder_value, DecoderError, Encoder, EncoderValue};
use s2n_quic_core::{probe, varint::VarInt};

/// The maximum queue_id that can be encoded without data loss.
///
/// The wire encoding is `(queue_id << 2) | flags`, which must fit in a 62-bit VarInt.
/// Therefore queue_id must be strictly less than 2^60.
const MAX_QUEUE_ID: u64 = 1 << 60;

pub const IS_RELIABLE_MASK: u64 = 0b10;
pub const IS_BIDIRECTIONAL_MASK: u64 = 0b01;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
#[cfg_attr(
    any(feature = "testing", test),
    derive(bolero_generator::TypeGenerator)
)]
pub struct Id {
    // Private to preserve invariant (MAX_QUEUE_ID).
    #[cfg_attr(any(feature = "testing", test), generator(Self::GENERATOR))]
    queue_id: VarInt,
    pub is_reliable: bool,
    pub is_bidirectional: bool,
}

impl fmt::Debug for Id {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("stream::Id")
            .field("queue_id", &self.queue_id)
            .field("is_reliable", &self.is_reliable)
            .field("is_bidirectional", &self.is_bidirectional)
            .finish()
    }
}

impl fmt::Display for Id {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.into_varint().fmt(f)
    }
}

impl probe::Arg for Id {
    #[inline]
    fn into_usdt(self) -> isize {
        self.into_varint().into_usdt()
    }
}

impl Id {
    #[cfg(any(feature = "testing", test))]
    const GENERATOR: core::ops::Range<VarInt> =
        VarInt::ZERO..unsafe { VarInt::new_unchecked(MAX_QUEUE_ID) };

    /// Creates a reliable, bidirectional Id.
    /// Returns `None` if `queue_id` cannot be encoded.
    #[inline]
    pub fn normal(queue_id: VarInt) -> Option<Self> {
        if *queue_id >= MAX_QUEUE_ID {
            return None;
        }
        Some(Self {
            queue_id,
            is_reliable: true,
            is_bidirectional: true,
        })
    }

    /// Creates an unreliable, unidirectional Id.
    /// Returns `None` if `queue_id` cannot be encoded.
    #[inline]
    pub fn unreliable_unidirectional(queue_id: VarInt) -> Option<Self> {
        if *queue_id >= MAX_QUEUE_ID {
            return None;
        }
        Some(Self {
            queue_id,
            is_reliable: false,
            is_bidirectional: false,
        })
    }

    /// Returns a copy with a different queue_id.
    /// Returns `None` if `queue_id` cannot be encoded.
    #[inline]
    pub fn with_queue_id(self, queue_id: VarInt) -> Option<Self> {
        if *queue_id >= MAX_QUEUE_ID {
            return None;
        }
        Some(Self { queue_id, ..self })
    }

    #[inline]
    pub fn queue_id(&self) -> VarInt {
        self.queue_id
    }

    #[inline]
    pub fn bidirectional(mut self) -> Self {
        self.is_bidirectional = true;
        self
    }

    #[inline]
    pub fn reliable(mut self) -> Self {
        self.is_reliable = true;
        self
    }

    #[inline]
    pub fn into_varint(self) -> VarInt {
        let queue_id = *self.queue_id;
        // Enforced by construction.
        debug_assert!(queue_id < MAX_QUEUE_ID);
        let is_reliable = if self.is_reliable {
            IS_RELIABLE_MASK
        } else {
            0b00
        };
        let is_bidirectional = if self.is_bidirectional {
            IS_BIDIRECTIONAL_MASK
        } else {
            0b00
        };
        let value = (queue_id << 2) | is_reliable | is_bidirectional;
        VarInt::new(value).unwrap()
    }

    #[inline]
    fn from_varint(value: VarInt) -> Option<Self> {
        let queue_id = *value >> 2;
        if queue_id >= MAX_QUEUE_ID {
            return None;
        }
        let is_reliable = *value & IS_RELIABLE_MASK == IS_RELIABLE_MASK;
        let is_bidirectional = *value & IS_BIDIRECTIONAL_MASK == IS_BIDIRECTIONAL_MASK;
        Some(Self {
            queue_id: VarInt::new(queue_id).unwrap(),
            is_reliable,
            is_bidirectional,
        })
    }
}

decoder_value!(
    impl<'a> Id {
        fn decode(buffer: Buffer) -> Result<Self> {
            let (value, buffer) = buffer.decode::<VarInt>()?;
            let id = Self::from_varint(value).ok_or(DecoderError::InvariantViolation(
                "stream ID queue_id exceeds maximum",
            ))?;
            Ok((id, buffer))
        }
    }
);

impl EncoderValue for Id {
    #[inline]
    fn encode<E: Encoder>(&self, encoder: &mut E) {
        self.into_varint().encode(encoder)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use s2n_codec::{DecoderBuffer, EncoderBuffer};

    const MAX_VALID: VarInt = unsafe { VarInt::new_unchecked(MAX_QUEUE_ID - 1) };
    const TOO_LARGE: VarInt = unsafe { VarInt::new_unchecked(MAX_QUEUE_ID) };

    #[test]
    fn normal_rejects_at_boundary() {
        assert!(Id::normal(MAX_VALID).is_some());
        assert!(Id::normal(TOO_LARGE).is_none());
    }

    #[test]
    fn unreliable_unidirectional_rejects_at_boundary() {
        assert!(Id::unreliable_unidirectional(MAX_VALID).is_some());
        assert!(Id::unreliable_unidirectional(TOO_LARGE).is_none());
    }

    #[test]
    fn with_queue_id_rejects_at_boundary() {
        let id = Id::normal(VarInt::ZERO).unwrap();
        assert!(id.with_queue_id(MAX_VALID).is_some());
        assert!(id.with_queue_id(TOO_LARGE).is_none());
    }

    #[test]
    fn with_queue_id_preserves_flags() {
        let id = Id::unreliable_unidirectional(VarInt::from_u8(1)).unwrap();
        let updated = id.with_queue_id(VarInt::from_u8(2)).unwrap();
        assert_eq!(updated.queue_id(), VarInt::from_u8(2));
        assert!(!updated.is_reliable);
        assert!(!updated.is_bidirectional);
    }

    #[test]
    fn normal_sets_flags() {
        let id = Id::normal(VarInt::from_u8(5)).unwrap();
        assert!(id.is_reliable);
        assert!(id.is_bidirectional);
        assert_eq!(id.queue_id(), VarInt::from_u8(5));
    }

    #[test]
    fn unreliable_unidirectional_clears_flags() {
        let id = Id::unreliable_unidirectional(VarInt::from_u8(5)).unwrap();
        assert!(!id.is_reliable);
        assert!(!id.is_bidirectional);
    }

    #[test]
    fn round_trip_varint() {
        for queue_id in [0u64, 1, 100, MAX_QUEUE_ID - 1] {
            let qid = VarInt::new(queue_id).unwrap();
            for (reliable, bidirectional) in
                [(true, true), (false, false), (true, false), (false, true)]
            {
                let id = Id::normal(qid).unwrap();
                let id = if reliable {
                    id.reliable()
                } else {
                    Id {
                        is_reliable: false,
                        ..id
                    }
                };
                let id = if bidirectional {
                    id.bidirectional()
                } else {
                    Id {
                        is_bidirectional: false,
                        ..id
                    }
                };

                let v = id.into_varint();
                let recovered = Id::from_varint(v).unwrap();
                assert_eq!(id, recovered);
            }
        }
    }

    #[test]
    fn round_trip_encode_decode() {
        for queue_id in [0u64, 1, 42, MAX_QUEUE_ID - 1] {
            let qid = VarInt::new(queue_id).unwrap();
            let id = Id::normal(qid).unwrap();

            let mut buf = [0u8; 16];
            let len = {
                let mut encoder = EncoderBuffer::new(&mut buf);
                id.encode(&mut encoder);
                encoder.len()
            };

            let decoder = DecoderBuffer::new(&buf[..len]);
            let (decoded, _) = decoder.decode::<Id>().unwrap();
            assert_eq!(id, decoded);
        }
    }

    #[test]
    fn from_varint_boundary() {
        // The largest encoded value whose queue_id is still valid
        let max_valid_encoded = VarInt::new((MAX_QUEUE_ID - 1) << 2 | 0b11).unwrap();
        assert!(Id::from_varint(max_valid_encoded).is_some());

        // One above the valid range (if representable as a VarInt)
        if let Ok(over) = VarInt::new(MAX_QUEUE_ID << 2) {
            assert!(Id::from_varint(over).is_none());
        }
    }

    #[test]
    fn zero_is_valid() {
        let id = Id::normal(VarInt::ZERO).unwrap();
        assert_eq!(id.queue_id(), VarInt::ZERO);
    }

    #[test]
    fn bolero_round_trip() {
        bolero::check!().with_type::<Id>().for_each(|id| {
            let v = id.into_varint();
            let recovered = Id::from_varint(v).unwrap();
            assert_eq!(*id, recovered);

            let mut buf = [0u8; 16];
            let len = {
                let mut encoder = EncoderBuffer::new(&mut buf);
                id.encode(&mut encoder);
                encoder.len()
            };
            let decoder = DecoderBuffer::new(&buf[..len]);
            let (decoded, _) = decoder.decode::<Id>().unwrap();
            assert_eq!(*id, decoded);
        });
    }
}

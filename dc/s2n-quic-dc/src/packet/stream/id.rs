// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::fmt;
use s2n_codec::{decoder_value, Encoder, EncoderValue};
use s2n_quic_core::{probe, varint::VarInt};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
#[cfg_attr(
    any(feature = "testing", test),
    derive(bolero_generator::TypeGenerator)
)]
pub struct Id {
    #[cfg_attr(any(feature = "testing", test), generator(Self::GENERATOR))]
    pub route_key: VarInt,
    pub is_reliable: bool,
    pub is_bidirectional: bool,
}

impl fmt::Debug for Id {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if f.alternate() {
            f.debug_struct("stream::Id")
                .field("route_key", &self.route_key)
                .field("is_reliable", &self.is_reliable)
                .field("is_bidirectional", &self.is_bidirectional)
                .finish()
        } else {
            self.into_varint().as_u64().fmt(f)
        }
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
        VarInt::ZERO..unsafe { VarInt::new_unchecked(1 << 60) };

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
    pub fn next(&self) -> Option<Self> {
        Some(Self {
            route_key: self.route_key.checked_add_usize(1)?,
            is_reliable: self.is_reliable,
            is_bidirectional: self.is_bidirectional,
        })
    }

    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = Self> {
        let mut next = Some(*self);
        core::iter::from_fn(move || {
            let current = next;
            next = next.and_then(|v| v.next());
            current
        })
    }

    #[inline]
    pub fn into_varint(self) -> VarInt {
        let key_id = *self.route_key;
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
        // FIXME: We may need to clamp key IDs at 2^60 or move reliable/bidirectional out of the ID
        // entirely, or not use a VarInt.
        //
        // This will panic at runtime when the key ID reaches 2^60th.
        let value = (key_id << 2) | is_reliable | is_bidirectional;
        VarInt::new(value).unwrap()
    }

    #[inline]
    pub fn from_varint(value: VarInt) -> Self {
        let is_reliable = *value & IS_RELIABLE_MASK == IS_RELIABLE_MASK;
        let is_bidirectional = *value & IS_BIDIRECTIONAL_MASK == IS_BIDIRECTIONAL_MASK;
        Self {
            route_key: VarInt::new(*value >> 2).unwrap(),
            is_reliable,
            is_bidirectional,
        }
    }
}

pub const IS_RELIABLE_MASK: u64 = 0b10;
pub const IS_BIDIRECTIONAL_MASK: u64 = 0b01;

decoder_value!(
    impl<'a> Id {
        fn decode(buffer: Buffer) -> Result<Self> {
            let (value, buffer) = buffer.decode::<VarInt>()?;
            Ok((Self::from_varint(value), buffer))
        }
    }
);

impl EncoderValue for Id {
    #[inline]
    fn encode<E: Encoder>(&self, encoder: &mut E) {
        self.into_varint().encode(encoder)
    }
}

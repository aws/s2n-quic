// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::fmt;
use s2n_codec::{decoder_invariant, decoder_value, Encoder, EncoderValue};
use s2n_quic_core::{assume, ensure, probe, varint::VarInt};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct Id {
    pub generation_id: u32,
    pub sequence_id: u16,
    pub is_reliable: bool,
    pub is_bidirectional: bool,
}

impl fmt::Debug for Id {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if f.alternate() {
            f.debug_struct("stream::Id")
                .field("generation_id", &self.generation_id)
                .field("sequence_id", &self.sequence_id)
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
        let mut generation_id = self.generation_id;
        let (sequence_id, overflowed) = self.sequence_id.overflowing_add(1);
        if overflowed {
            generation_id = generation_id.checked_add(1)?;
        }
        Some(Self {
            generation_id,
            sequence_id,
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
        let generation_id = (self.generation_id as u64) << 18;
        let sequence_id = (self.sequence_id as u64) << 2;
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
        let value = generation_id | sequence_id | is_reliable | is_bidirectional;
        unsafe {
            assume!(value <= MAX_ID_VALUE);
            VarInt::new_unchecked(value)
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TryFromIntError(());

impl fmt::Display for TryFromIntError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "could not convert the provided u64 to a stream ID")
    }
}

impl std::error::Error for TryFromIntError {}

impl TryFrom<u64> for Id {
    type Error = TryFromIntError;

    #[inline]
    fn try_from(value: u64) -> Result<Self, Self::Error> {
        ensure!(value <= (1 << (32 + 16)), Err(TryFromIntError(())));
        let generation_id = (value >> 16) as u32;
        let sequence_id = value as u16;
        Ok(Self {
            generation_id,
            sequence_id,
            is_reliable: false,
            is_bidirectional: false,
        })
    }
}

const MAX_ID_VALUE: u64 = 1 << (32 + 16 + 1 + 1);
const IS_RELIABLE_MASK: u64 = 0b10;
const IS_BIDIRECTIONAL_MASK: u64 = 0b01;

decoder_value!(
    impl<'a> Id {
        fn decode(buffer: Buffer) -> Result<Self> {
            let (value, buffer) = buffer.decode::<VarInt>()?;
            let value = *value;
            decoder_invariant!(value <= MAX_ID_VALUE, "invalid range");
            let generation_id = (value >> 18) as u32;
            let sequence_id = (value >> 2) as u16;
            let is_reliable = value & IS_RELIABLE_MASK == IS_RELIABLE_MASK;
            let is_bidirectional = value & IS_BIDIRECTIONAL_MASK == IS_BIDIRECTIONAL_MASK;
            Ok((
                Self {
                    generation_id,
                    sequence_id,
                    is_reliable,
                    is_bidirectional,
                },
                buffer,
            ))
        }
    }
);

impl EncoderValue for Id {
    #[inline]
    fn encode<E: Encoder>(&self, encoder: &mut E) {
        self.into_varint().encode(encoder)
    }
}

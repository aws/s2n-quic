// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::packet::number::{
    decode_packet_number,
    packet_number::PacketNumber,
    packet_number_len::{PacketNumberLen, PacketNumberLenValue},
    packet_number_space::PacketNumberSpace,
};
use s2n_codec::{u24, DecoderBuffer, DecoderBufferResult, DecoderValue, Encoder, EncoderValue};

#[cfg(any(test, feature = "generator"))]
use bolero_generator::prelude::*;

/// A truncated packet number, which is derived from the largest acknowledged packet number
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(any(test, feature = "generator"), derive(TypeGenerator))]
pub struct TruncatedPacketNumber {
    pub(crate) space: PacketNumberSpace,
    pub(crate) value: TruncatedPacketNumberValue,
}

#[allow(clippy::len_without_is_empty)] // Clippy gets confused by the const on is_empty
impl TruncatedPacketNumber {
    /// Returns the space for the given `TruncatedPacketNumber`
    #[inline]
    pub const fn space(self) -> PacketNumberSpace {
        self.space
    }

    /// Expands the `TruncatedPacketNumber` into a `PacketNumber`
    #[inline]
    pub fn expand(self, largest_acknowledged_packet_number: PacketNumber) -> PacketNumber {
        decode_packet_number(largest_acknowledged_packet_number, self)
    }

    #[inline]
    pub fn len(&self) -> PacketNumberLen {
        self.value.len(self.space)
    }

    #[inline]
    pub const fn is_empty(self) -> bool {
        false
    }

    /// Internal function to create a `TruncatedPacketNumber`.
    ///
    /// `TruncatedPacketNumber` values should be created either
    /// by using a `PacketNumberLen` to decode a buffer, or truncating
    /// a `PacketNumber`.
    #[inline]
    pub(crate) fn new<Value: Into<TruncatedPacketNumberValue>>(
        value: Value,
        space: PacketNumberSpace,
    ) -> Self {
        Self {
            value: value.into(),
            space,
        }
    }

    /// Internal function to decode a `TruncatedPacketNumber` with a given size.
    #[inline]
    pub(crate) fn decode<'a, Value: Into<TruncatedPacketNumberValue> + DecoderValue<'a>>(
        buffer: DecoderBuffer<'a>,
        space: PacketNumberSpace,
    ) -> DecoderBufferResult<'a, Self> {
        let (value, buffer) = buffer.decode()?;
        let packet_number = Self::new::<Value>(value, space);
        Ok((packet_number, buffer))
    }

    /// Convert the `TruncatedPacketNumber` into `u64`
    #[inline]
    pub(crate) fn into_u64(self) -> u64 {
        self.value.into_u64()
    }

    /// Get the bitsize for the given `TruncatedPacketNumber`
    #[inline]
    pub(crate) fn bitsize(self) -> usize {
        self.len().bitsize()
    }
}

impl EncoderValue for TruncatedPacketNumber {
    #[inline]
    fn encode<E: Encoder>(&self, buffer: &mut E) {
        buffer.encode(&self.value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(any(test, feature = "generator"), derive(TypeGenerator))]
pub enum TruncatedPacketNumberValue {
    U8(u8),
    U16(u16),
    U24(u24),
    U32(u32),
}

impl TruncatedPacketNumberValue {
    #[inline]
    fn into_u64(self) -> u64 {
        match self {
            Self::U8(value) => value.into(),
            Self::U16(value) => value.into(),
            Self::U24(value) => value.into(),
            Self::U32(value) => value.into(),
        }
    }

    #[inline]
    fn len(self, space: PacketNumberSpace) -> PacketNumberLen {
        let value = match self {
            Self::U8(_value) => PacketNumberLenValue::U8,
            Self::U16(_value) => PacketNumberLenValue::U16,
            Self::U24(_value) => PacketNumberLenValue::U24,
            Self::U32(_value) => PacketNumberLenValue::U32,
        };

        PacketNumberLen { space, value }
    }
}

impl EncoderValue for TruncatedPacketNumberValue {
    #[inline]
    fn encode<E: Encoder>(&self, buffer: &mut E) {
        match self {
            Self::U8(value) => buffer.encode(value),
            Self::U16(value) => buffer.encode(value),
            Self::U24(value) => buffer.encode(value),
            Self::U32(value) => buffer.encode(value),
        }
    }
}

impl From<u8> for TruncatedPacketNumberValue {
    #[inline]
    fn from(value: u8) -> Self {
        Self::U8(value)
    }
}

impl From<u16> for TruncatedPacketNumberValue {
    #[inline]
    fn from(value: u16) -> Self {
        Self::U16(value)
    }
}

impl From<u24> for TruncatedPacketNumberValue {
    #[inline]
    fn from(value: u24) -> Self {
        Self::U24(value)
    }
}

impl From<u32> for TruncatedPacketNumberValue {
    #[inline]
    fn from(value: u32) -> Self {
        Self::U32(value)
    }
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    packet::number::{
        packet_number_space::PacketNumberSpace, truncated_packet_number::TruncatedPacketNumber,
        PACKET_NUMBER_LEN_MASK,
    },
    varint::VarInt,
};
use s2n_codec::{u24, DecoderBuffer, DecoderBufferResult};

/// A fully-decoded and unprotected packet number length
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PacketNumberLen {
    pub(crate) space: PacketNumberSpace,
    pub(crate) value: PacketNumberLenValue,
}

impl PacketNumberLen {
    pub const MAX_LEN: usize = U32_SIZE;

    /// Returns the max `PacketNumberLen` value for the given `PacketNumberSpace`
    pub const fn max(space: PacketNumberSpace) -> Self {
        Self {
            value: PacketNumberLenValue::U32,
            space,
        }
    }

    /// Returns the `PacketNumberSpace` for the given `PacketNumberLen`
    #[inline]
    pub const fn space(self) -> PacketNumberSpace {
        self.space
    }

    /// Decodes a `TruncatedPacketNumber` with the given `PacketNumberLen`
    #[inline]
    pub fn decode_truncated_packet_number(
        self,
        buffer: DecoderBuffer,
    ) -> DecoderBufferResult<TruncatedPacketNumber> {
        self.value
            .decode_truncated_packet_number(buffer, self.space)
    }

    /// Returns a packet tag mask for the given `PacketNumberLen`.
    #[inline]
    pub fn into_packet_tag_mask(self) -> u8 {
        self.value.into_packet_tag_mask()
    }

    /// Returns the bytesize required for encoding the given `PacketNumberLen`
    #[inline]
    pub fn bytesize(self) -> usize {
        self.value.bytesize()
    }

    /// Returns the bitsize required for encoding the given `PacketNumberLen`
    #[inline]
    pub fn bitsize(self) -> usize {
        self.value.bitsize()
    }

    #[inline]
    pub(crate) fn truncate_packet_number(self, value: VarInt) -> TruncatedPacketNumber {
        self.value.truncate_packet_number(value, self.space)
    }

    #[inline]
    pub(crate) fn from_packet_tag(tag: u8, space: PacketNumberSpace) -> Self {
        Self {
            value: PacketNumberLenValue::from_packet_tag(tag),
            space,
        }
    }

    #[inline]
    pub(crate) fn from_varint(value: VarInt, space: PacketNumberSpace) -> Option<Self> {
        Some(Self {
            value: PacketNumberLenValue::from_varint(value)?,
            space,
        })
    }
}

const U8_TAG: u8 = 0; // (8 / 8) - 1;
const U16_TAG: u8 = (16 / 8) - 1;
const U24_TAG: u8 = (24 / 8) - 1;
const U32_TAG: u8 = (32 / 8) - 1;

const U8_SIZE: usize = 1; // 8 / 8
const U16_SIZE: usize = 16 / 8;
const U24_SIZE: usize = 24 / 8;
const U32_SIZE: usize = 32 / 8;

const U8_MAX: u64 = (1 << 8) - 1;
const U16_MAX: u64 = (1 << 16) - 1;
const U24_MAX: u64 = (1 << 24) - 1;
const U32_MAX: u64 = (1 << 32) - 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum PacketNumberLenValue {
    U8,
    U16,
    U24,
    U32,
}

impl PacketNumberLenValue {
    #[inline]
    pub fn decode_truncated_packet_number(
        self,
        buffer: DecoderBuffer,
        space: PacketNumberSpace,
    ) -> DecoderBufferResult<TruncatedPacketNumber> {
        match self {
            Self::U8 => TruncatedPacketNumber::decode::<u8>(buffer, space),
            Self::U16 => TruncatedPacketNumber::decode::<u16>(buffer, space),
            Self::U24 => TruncatedPacketNumber::decode::<u24>(buffer, space),
            Self::U32 => TruncatedPacketNumber::decode::<u32>(buffer, space),
        }
    }

    #[inline]
    pub(crate) fn truncate_packet_number(
        self,
        value: VarInt,
        space: PacketNumberSpace,
    ) -> TruncatedPacketNumber {
        match self {
            Self::U8 => TruncatedPacketNumber::new(*value as u8, space),
            Self::U16 => TruncatedPacketNumber::new(*value as u16, space),
            Self::U24 => TruncatedPacketNumber::new(u24::new_truncated(*value as u32), space),
            Self::U32 => TruncatedPacketNumber::new(*value as u32, space),
        }
    }

    #[inline]
    pub fn into_packet_tag_mask(self) -> u8 {
        match self {
            Self::U8 => U8_TAG,
            Self::U16 => U16_TAG,
            Self::U24 => U24_TAG,
            Self::U32 => U32_TAG,
        }
    }

    #[inline]
    pub fn bytesize(self) -> usize {
        match self {
            Self::U8 => U8_SIZE,
            Self::U16 => U16_SIZE,
            Self::U24 => U24_SIZE,
            Self::U32 => U32_SIZE,
        }
    }

    #[inline]
    pub fn bitsize(self) -> usize {
        self.bytesize() * 8
    }

    #[inline]
    pub fn from_packet_tag(tag: u8) -> Self {
        match tag & PACKET_NUMBER_LEN_MASK {
            U8_TAG => Self::U8,
            U16_TAG => Self::U16,
            U24_TAG => Self::U24,
            U32_TAG => Self::U32,
            _ => unreachable!("the mask only allows for 4 valid values"),
        }
    }

    #[inline]
    pub fn from_varint(value: VarInt) -> Option<Self> {
        #[allow(clippy::match_overlapping_arm)]
        match *value {
            0..=U8_MAX => Some(Self::U8),
            0..=U16_MAX => Some(Self::U16),
            0..=U24_MAX => Some(Self::U24),
            0..=U32_MAX => Some(Self::U32),
            _ => None,
        }
    }
}

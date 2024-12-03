// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    packet::number::{packet_number::PacketNumber, packet_number_len::PacketNumberLen},
    varint::VarInt,
};

#[cfg(any(test, feature = "generator"))]
use bolero_generator::prelude::*;

/// Contains all of the available packet spaces for QUIC packets
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(any(test, feature = "generator"), derive(TypeGenerator))]
#[repr(u8)]
pub enum PacketNumberSpace {
    // This MUST start with 1 to enable optimized memory layout
    #[default]
    Initial = 1,
    Handshake = 2,
    ApplicationData = 3,
}

impl PacketNumberSpace {
    /// Returns `true` if the `PacketNumberSpace` is set to `Initial`
    #[inline]
    pub fn is_initial(self) -> bool {
        matches!(self, Self::Initial)
    }

    /// Returns `true` if the `PacketNumberSpace` is set to `Handshake`
    #[inline]
    pub fn is_handshake(self) -> bool {
        matches!(self, Self::Handshake)
    }

    /// Returns `true` if the `PacketNumberSpace` is set to `ApplicationData`
    #[inline]
    pub fn is_application_data(self) -> bool {
        matches!(self, Self::ApplicationData)
    }

    /// Create a new `PacketNumber` for the given `PacketNumberSpace`
    #[inline]
    pub const fn new_packet_number(self, value: VarInt) -> PacketNumber {
        PacketNumber::from_varint(value, self)
    }

    /// Create a new `PacketNumberLen` for the given `PacketNumberSpace` with a packet `tag`
    #[inline]
    pub fn new_packet_number_len(self, tag: u8) -> PacketNumberLen {
        PacketNumberLen::from_packet_tag(tag, self)
    }

    /// Asserts the `PacketNumberSpace` is equal
    #[inline(always)]
    pub(crate) fn assert_eq(self, other: Self) {
        debug_assert_eq!(
            self, other,
            "PacketNumbers cannot be compared across packet spaces"
        );
    }

    /// Returns the tag representation of PacketNumberSpace
    #[inline]
    pub(crate) const fn as_tag(self) -> u8 {
        self as u8
    }

    /// Creates a PacketNumberSpace from a tag representation
    ///
    /// # Safety
    ///
    /// Callers must ensure tag is less than `3`
    #[inline]
    pub(crate) fn from_tag(tag: u8) -> Self {
        match tag {
            1 => Self::Initial,
            2 => Self::Handshake,
            3 => Self::ApplicationData,
            _ if cfg!(debug_assertions) => panic!("invalid tag for PacketNumberSpace"),
            _ => Self::Initial,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_test() {
        let spaces = [
            PacketNumberSpace::Initial,
            PacketNumberSpace::Handshake,
            PacketNumberSpace::ApplicationData,
        ];

        for space in spaces.iter().cloned() {
            assert_eq!(PacketNumberSpace::from_tag(space.as_tag()), space);
        }
    }
}

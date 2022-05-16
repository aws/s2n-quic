// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    event::IntoEvent,
    packet::number::{
        derive_truncation_range, packet_number_space::PacketNumberSpace,
        truncated_packet_number::TruncatedPacketNumber,
    },
    varint::VarInt,
};
use core::{
    cmp::Ordering,
    fmt,
    hash::{Hash, Hasher},
    mem::size_of,
    num::NonZeroU64,
};

#[cfg(any(test, feature = "generator"))]
use bolero_generator::*;

const PACKET_SPACE_BITLEN: usize = 2;
const PACKET_SPACE_SHIFT: usize = (size_of::<PacketNumber>() * 8) - PACKET_SPACE_BITLEN;
const PACKET_NUMBER_MASK: u64 = core::u64::MAX >> PACKET_SPACE_BITLEN;

/// Contains a fully-decoded packet number in a given space
///
/// Internally the packet number is represented as a [`NonZeroU64`]
/// to ensure optimal memory layout.
///
/// The lower 62 bits are used to store the actual packet number value.
/// The upper 2 bits are used to store the packet number space. Because
/// there are only 3 spaces, the zero state is never used, which is why
/// [`NonZeroU64`] can be used instead of `u64`.
#[derive(Clone, Copy, Eq)]
#[cfg_attr(any(test, feature = "generator"), derive(TypeGenerator))]
pub struct PacketNumber(NonZeroU64);

impl IntoEvent<u64> for PacketNumber {
    fn into_event(self) -> u64 {
        self.as_u64()
    }
}

impl Default for PacketNumber {
    fn default() -> Self {
        Self::from_varint(Default::default(), PacketNumberSpace::Initial)
    }
}

impl Hash for PacketNumber {
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.hash(state)
    }
}

impl PartialEq for PacketNumber {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl PartialOrd for PacketNumber {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PacketNumber {
    #[inline]
    fn cmp(&self, other: &Self) -> Ordering {
        if cfg!(debug_assertions) {
            self.space().assert_eq(other.space());
        }
        self.0.cmp(&other.0)
    }
}

impl fmt::Debug for PacketNumber {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_tuple("PacketNumber")
            .field(&self.space())
            .field(&self.as_u64())
            .finish()
    }
}

impl fmt::Display for PacketNumber {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.as_u64().fmt(f)
    }
}

impl PacketNumber {
    /// Creates a PacketNumber for a given VarInt and PacketNumberSpace
    #[inline]
    pub(crate) const fn from_varint(value: VarInt, space: PacketNumberSpace) -> Self {
        let tag = space.as_tag() as u64;
        let pn = (tag << PACKET_SPACE_SHIFT) | value.as_u64();
        let pn = unsafe {
            // Safety: packet number space tag is never 0
            NonZeroU64::new_unchecked(pn)
        };
        Self(pn)
    }

    /// Returns the `PacketNumberSpace` for the given `PacketNumber`
    #[inline]
    pub fn space(self) -> PacketNumberSpace {
        let tag = self.0.get() >> PACKET_SPACE_SHIFT;
        PacketNumberSpace::from_tag(tag as u8)
    }

    /// Converts the `PacketNumber` into a `VarInt` value.
    ///
    /// Note: Even though some scenarios require this function, it should be
    /// avoided in most cases, as it removes the corresponding `PacketNumberSpace`
    /// and allows math operations to be performed, which can easily result in
    /// protocol errors.
    #[allow(clippy::wrong_self_convention)] // Don't use `self` here to make conversion explicit
    pub const fn as_varint(packet_number: Self) -> VarInt {
        // Safety: when converting to a u64, we remove the top 2 bits which
        //         will force the value to fit into a VarInt.
        unsafe { VarInt::new_unchecked(packet_number.as_u64()) }
    }

    /// Truncates the `PacketNumber` into a `TruncatedPacketNumber` based on
    /// the largest acknowledged packet number
    #[inline]
    pub fn truncate(
        self,
        largest_acknowledged_packet_number: Self,
    ) -> Option<TruncatedPacketNumber> {
        Some(
            derive_truncation_range(largest_acknowledged_packet_number, self)?
                .truncate_packet_number(Self::as_varint(self)),
        )
    }

    /// Compute the next packet number in the space. If the packet number has
    /// exceeded the maximum value allowed `None` will be returned.
    #[inline]
    pub fn next(self) -> Option<Self> {
        let value = Self::as_varint(self).checked_add(VarInt::from_u8(1))?;
        let space = self.space();
        Some(Self::from_varint(value, space))
    }

    /// Compute the prev packet number in the space. If the packet number has
    /// underflowed `None` will be returned.
    #[inline]
    pub fn prev(self) -> Option<Self> {
        let value = Self::as_varint(self).checked_sub(VarInt::from_u8(1))?;
        let space = self.space();
        Some(Self::from_varint(value, space))
    }

    /// Create a nonce for crypto from the packet number value
    ///
    /// Note: This should not be used by anything other than crypto-related
    /// functionality.
    #[inline]
    pub const fn as_crypto_nonce(self) -> u64 {
        self.as_u64()
    }

    /// Returns the value with the top 2 bits removed
    #[inline]
    pub const fn as_u64(self) -> u64 {
        self.0.get() & PACKET_NUMBER_MASK
    }

    /// Computes the distance between this packet number and the given packet number,
    /// returning None if overflow occurred.
    #[inline]
    pub fn checked_distance(self, rhs: PacketNumber) -> Option<u64> {
        self.space().assert_eq(rhs.space());
        Self::as_u64(self).checked_sub(Self::as_u64(rhs))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Make sure the assumptions around packet number space tags holds true
    #[test]
    fn packet_number_space_assumptions_test() {
        assert!(PacketNumberSpace::Initial.as_tag() != 0);
        assert!(PacketNumberSpace::Handshake.as_tag() != 0);
        assert!(PacketNumberSpace::ApplicationData.as_tag() != 0);
    }

    #[test]
    fn round_trip_test() {
        let spaces = [
            PacketNumberSpace::Initial,
            PacketNumberSpace::Handshake,
            PacketNumberSpace::ApplicationData,
        ];

        let values = [
            VarInt::from_u8(0),
            VarInt::from_u8(1),
            VarInt::from_u8(2),
            VarInt::from_u8(core::u8::MAX / 2),
            VarInt::from_u8(core::u8::MAX - 1),
            VarInt::from_u8(core::u8::MAX),
            VarInt::from_u16(core::u16::MAX / 2),
            VarInt::from_u16(core::u16::MAX - 1),
            VarInt::from_u16(core::u16::MAX),
            VarInt::from_u32(core::u32::MAX / 2),
            VarInt::from_u32(core::u32::MAX - 1),
            VarInt::from_u32(core::u32::MAX),
            VarInt::MAX,
        ];

        for space in spaces.iter().cloned() {
            for value in values.iter().cloned() {
                let pn = PacketNumber::from_varint(value, space);
                assert_eq!(pn.space(), space, "{:#064b}", pn.0);
                assert_eq!(PacketNumber::as_varint(pn), value, "{:#064b}", pn.0);
            }
        }
    }
    #[test]
    #[should_panic]
    fn wrong_packet_number_space() {
        PacketNumberSpace::ApplicationData
            .new_packet_number(VarInt::from_u8(0))
            .checked_distance(PacketNumberSpace::Handshake.new_packet_number(VarInt::from_u8(0)));
    }
}

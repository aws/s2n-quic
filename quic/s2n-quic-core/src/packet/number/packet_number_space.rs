use crate::{
    packet::number::{packet_number::PacketNumber, packet_number_len::PacketNumberLen},
    varint::VarInt,
};

/// Contains all of the available packet spaces for QUIC packets
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum PacketNumberSpace {
    // This MUST start with 1 to enable optimized memory layout
    Initial = 1,
    Handshake = 2,
    ApplicationData = 3,
}

impl Default for PacketNumberSpace {
    fn default() -> Self {
        PacketNumberSpace::Initial
    }
}

impl PacketNumberSpace {
    /// Returns `true` if the `PacketNumberSpace` is set to `Initial`
    #[inline]
    pub fn is_initial(self) -> bool {
        match self {
            Self::Initial => true,
            _ => false,
        }
    }

    /// Returns `true` if the `PacketNumberSpace` is set to `Handshake`
    #[inline]
    pub fn is_handshake(self) -> bool {
        match self {
            Self::Handshake => true,
            _ => false,
        }
    }

    /// Returns `true` if the `PacketNumberSpace` is set to `ApplicationData`
    #[inline]
    pub fn is_application_data(self) -> bool {
        match self {
            Self::ApplicationData => true,
            _ => false,
        }
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
    #[inline]
    pub(crate) fn assert_eq(self, other: Self) {
        debug_assert_eq!(
            self, other,
            "PacketNumbers cannot be compared across packet spaces"
        );
    }

    /// Returns the tag representation of PacketNumberSpace
    pub(crate) const fn as_tag(self) -> u8 {
        self as u8
    }

    /// Creates a PacketNumberSpace from a tag representation
    ///
    /// # Safety
    ///
    /// Callers must ensure tag is less than `3`
    pub(crate) fn from_tag(tag: u8) -> Self {
        match tag {
            1 => Self::Initial,
            2 => Self::Handshake,
            3 => Self::ApplicationData,
            _ => panic!("invalid tag for PacketNumberSpace"),
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

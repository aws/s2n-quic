//! A set of acknowledgments

use crate::packet::number::{PacketNumber, PacketNumberRange};
use core::ops::RangeInclusive;

/// A set of Acknowledgments
///
/// The implementation of the set is allowed to store packet numbers in
/// an arbitrary form.
pub trait AckSet {
    /// Returns whether the [`AckSet`] contains a given packet number
    fn contains(&self, packet_number: PacketNumber) -> bool;

    /// Smallest packet number in the set
    fn smallest(&self) -> PacketNumber;

    /// Largest packet number in the set
    fn largest(&self) -> PacketNumber;
}

// A single packet number is also a set

impl AckSet for PacketNumber {
    fn contains(&self, packet_number: PacketNumber) -> bool {
        *self == packet_number
    }

    fn smallest(&self) -> PacketNumber {
        *self
    }

    fn largest(&self) -> PacketNumber {
        *self
    }
}

impl AckSet for RangeInclusive<PacketNumber> {
    fn contains(&self, packet_number: PacketNumber) -> bool {
        RangeInclusive::contains(self, &packet_number)
    }

    fn smallest(&self) -> PacketNumber {
        *self.start()
    }

    fn largest(&self) -> PacketNumber {
        *self.end()
    }
}

impl AckSet for PacketNumberRange {
    fn contains(&self, packet_number: PacketNumber) -> bool {
        PacketNumberRange::contains(self, packet_number)
    }

    fn smallest(&self) -> PacketNumber {
        self.start()
    }

    fn largest(&self) -> PacketNumber {
        self.end()
    }
}

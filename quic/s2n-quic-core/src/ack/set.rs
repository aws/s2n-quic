// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! A set of acknowledgments

use crate::packet::number::{PacketNumber, PacketNumberRange};
use core::ops::RangeInclusive;

/// A set of Acknowledgments
///
/// The implementation of the set is allowed to store packet numbers in
/// an arbitrary form.
pub trait Set {
    /// Returns whether the [`Set`] contains a given packet number
    fn contains(&self, packet_number: PacketNumber) -> bool;

    /// Smallest packet number in the set
    fn smallest(&self) -> PacketNumber;

    /// Largest packet number in the set
    fn largest(&self) -> PacketNumber;
}

// A single packet number is also a set

impl Set for PacketNumber {
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

impl Set for RangeInclusive<PacketNumber> {
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

impl Set for PacketNumberRange {
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

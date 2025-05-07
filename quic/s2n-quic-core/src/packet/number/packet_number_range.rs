// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{event::IntoEvent, packet::number::PacketNumber};
use core::ops::RangeInclusive;

/// An inclusive range of `PacketNumber`s
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct PacketNumberRange {
    start: PacketNumber,
    end: PacketNumber,
    exhausted: bool,
}

impl PacketNumberRange {
    /// Creates a new packet number range.
    #[inline]
    pub fn new(start: PacketNumber, end: PacketNumber) -> Self {
        assert!(start <= end, "start must be less than or equal to end");
        Self {
            start,
            end,
            exhausted: false,
        }
    }

    /// Returns true if the range contains the given packet number
    #[inline]
    pub fn contains(&self, packet_number: PacketNumber) -> bool {
        self.start <= packet_number && packet_number <= self.end
    }

    /// Returns the lower bound of the range (inclusive).
    #[inline]
    pub fn start(&self) -> PacketNumber {
        self.start
    }

    /// Returns the upper bound of the range (inclusive).
    #[inline]
    pub fn end(&self) -> PacketNumber {
        self.end
    }
}

impl IntoEvent<RangeInclusive<u64>> for PacketNumberRange {
    #[inline]
    fn into_event(self) -> RangeInclusive<u64> {
        self.start().into_event()..=self.end().into_event()
    }
}

impl Iterator for PacketNumberRange {
    type Item = PacketNumber;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if !self.exhausted && self.start <= self.end {
            let current = self.start;
            if let Some(next) = current.next() {
                self.start = next;
            } else {
                // PacketNumber range has been exceeded
                self.exhausted = true;
            }
            Some(current)
        } else {
            self.exhausted = true;
            None
        }
    }
}

impl DoubleEndedIterator for PacketNumberRange {
    #[inline]
    fn next_back(&mut self) -> Option<Self::Item> {
        if !self.exhausted && self.start <= self.end {
            let current = self.end;
            if let Some(prev) = current.prev() {
                self.end = prev;
                self.exhausted = self.start > self.end;
            } else {
                // PacketNumber range has been exceeded
                self.exhausted = true;
            }
            Some(current)
        } else {
            self.exhausted = true;
            None
        }
    }
}

#[cfg(test)]
mod test {
    use crate::{
        packet::number::{PacketNumberRange, PacketNumberSpace},
        varint::VarInt,
    };

    #[test]
    fn iterator() {
        let mut counter = 1;
        let start = PacketNumberSpace::Handshake.new_packet_number(VarInt::from_u8(1));
        let end = PacketNumberSpace::Handshake.new_packet_number(VarInt::from_u8(10));

        let range = PacketNumberRange::new(start, end);
        assert_eq!(start, range.start());
        assert_eq!(end, range.end());

        for packet_number in range {
            assert_eq!(counter, packet_number.as_u64());
            counter += 1;
        }

        assert_eq!(counter, 11);
    }

    #[test]
    fn double_ended_iterator() {
        let mut counter = 10;
        let start = PacketNumberSpace::Handshake.new_packet_number(VarInt::from_u8(1));
        let end = PacketNumberSpace::Handshake.new_packet_number(VarInt::from_u8(10));

        let mut range = PacketNumberRange::new(start, end);
        assert_eq!(start, range.start());
        assert_eq!(end, range.end());

        while let Some(packet_number) = range.next_back() {
            assert_eq!(counter, packet_number.as_u64());
            counter -= 1;
        }

        assert_eq!(counter, 0);
    }

    #[test]
    fn double_ended_iterator_zero() {
        let mut items = vec![];
        let start = PacketNumberSpace::Handshake.new_packet_number(VarInt::from_u8(0));
        let end = PacketNumberSpace::Handshake.new_packet_number(VarInt::from_u8(9));

        let mut range = PacketNumberRange::new(start, end);
        assert_eq!(start, range.start());
        assert_eq!(end, range.end());

        while let Some(packet_number) = range.next_back() {
            items.push(packet_number.as_u64());
        }

        items.reverse();

        for (idx, value) in items.iter().enumerate() {
            assert_eq!(idx as u64, *value);
        }
    }

    #[test]
    fn start_equals_end() {
        let start = PacketNumberSpace::Handshake.new_packet_number(VarInt::from_u8(1));
        let end = PacketNumberSpace::Handshake.new_packet_number(VarInt::from_u8(1));

        let mut range = PacketNumberRange::new(start, end);

        assert_eq!(1, range.count());
        assert_eq!(start, range.next_back().unwrap());
    }

    #[test]
    #[should_panic]
    fn start_greater_than_end() {
        let start = PacketNumberSpace::Handshake.new_packet_number(VarInt::from_u8(1));
        let end = PacketNumberSpace::Handshake.new_packet_number(VarInt::from_u8(10));
        PacketNumberRange::new(end, start);
    }

    #[test]
    fn end_is_max_packet_number() {
        let start = PacketNumberSpace::Handshake
            .new_packet_number(VarInt::new((u64::MAX >> 2) - 1).unwrap());
        let end =
            PacketNumberSpace::Handshake.new_packet_number(VarInt::new(u64::MAX >> 2).unwrap());

        assert_eq!(2, PacketNumberRange::new(start, end).count());
    }
}

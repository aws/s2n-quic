// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{packet::number::PacketNumber, varint::VarInt};
use core::mem;

#[derive(Clone, Default, Debug)]
pub struct SlidingWindow {
    /// Bitfield representing each packet number less than
    /// the right edge up to the window width.
    window: Window,
    /// The highest packet number seen so far, which is the
    /// right edge of the window.
    right_edge: Option<PacketNumber>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SlidingWindowError {
    Duplicate,
    TooOld,
}

/// 128-bit wide window allowing for 128 packets, plus the highest
/// packet representing the right edge to be tracked.
type Window = u128;

/// The total width of the window = the size of the 128-bit bitfield + 1 more bit
/// representing the right edge, which is always set.
const WINDOW_WIDTH: u64 = 1 + mem::size_of::<Window>() as u64 * 8;

#[derive(Debug, PartialEq, Eq)]
enum WindowPosition {
    /// Left of the window, assumed to be a duplicate.
    Left,
    /// Right of the window, the value is the offset from the right edge.
    Right(u64),
    /// Equal to the highest value tracked by the window.
    RightEdge,
    /// Within the window, the value is the offset from the right edge.
    Within(u64),
    /// The window is empty.
    Empty,
}

/// A set with entries that were removed/slid past from SlidingWindow during insertion.
#[derive(Default, Clone)]
pub struct EvictedSet {
    /// Bitfield representing each packet number less than
    /// the right edge up to the window width.
    ///
    /// Bits are 1 if they are returned by next().
    window: Window,
    /// The highest packet number seen so far, which is the
    /// right edge of the window.
    ///
    /// This number is never included in this set.
    right_edge: PacketNumber,
}

impl core::fmt::Debug for EvictedSet {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_set().entries(self.clone()).finish()
    }
}

impl PartialEq for EvictedSet {
    fn eq(&self, other: &Self) -> bool {
        self.clone().eq(other.clone())
    }
}

impl Iterator for EvictedSet {
    type Item = PacketNumber;

    fn next(&mut self) -> Option<PacketNumber> {
        loop {
            // If the window is empty, there is nothing in the set.
            if self.window == 0 {
                return None;
            }

            let shift = self.window.leading_zeros() + 1;

            // Shift the right edge to the right such that the first set bit is in the leftmost
            // position. If we have a leading 1, then this is a shift by 1.
            //
            // That set bit represents the smallest included bit.
            self.right_edge = PacketNumber::from_varint(
                PacketNumber::as_varint(self.right_edge) + VarInt::from_u32(shift),
                self.right_edge.space(),
            );

            // We shift in zeros, which are not in the set.
            if shift == Window::BITS {
                self.window = 0;
            } else {
                self.window <<= shift;
            }

            if let Some(left_edge) = PacketNumber::as_varint(self.right_edge)
                .checked_sub(VarInt::from_u32(WINDOW_WIDTH as u32))
            {
                return Some(PacketNumber::from_varint(
                    left_edge,
                    self.right_edge.space(),
                ));
            } else {
                // If the bit is set, but it's less than zero in value, then that's a spuriously set
                // bit.
                continue;
            }
        }
    }
}

//= https://www.rfc-editor.org/rfc/rfc4303#section-3.4.3
//# Duplicates are rejected through the use of a sliding receive window.
//# How the window is implemented is a local matter, but the following
//# text describes the functionality that the implementation must
//# exhibit.
//#
//# The "right" edge of the window represents the highest, validated
//# Sequence Number value received on this SA.  Packets that contain
//# sequence numbers lower than the "left" edge of the window are
//# rejected.  Packets falling within the window are checked against a
//# list of received packets within the window.
impl SlidingWindow {
    /// Inserts the `packet_number` into the sliding window, returning
    /// a SlidingWindowError::Duplicate if the `packet_number` has already
    /// been inserted into the sliding window or a SlidingWindowError::TooOld
    /// if the `packet_number` is beyond the capacity of the sliding window and
    /// thus cannot be determined if it is a duplicate.
    pub fn insert(&mut self, packet_number: PacketNumber) -> Result<(), SlidingWindowError> {
        self.insert_with_evicted(packet_number).map(|_| ())
    }

    /// Inserts the `packet_number` into the sliding window, returning
    /// a SlidingWindowError::Duplicate if the `packet_number` has already
    /// been inserted into the sliding window or a SlidingWindowError::TooOld
    /// if the `packet_number` is beyond the capacity of the sliding window and
    /// thus cannot be determined if it is a duplicate.
    ///
    /// Returns Ok(None) if insert didn't slide past an entry that would have returned Ok(...) had it
    /// been inserted prior to this insert, and Ok(entries) for those we did slide past.
    pub fn insert_with_evicted(
        &mut self,
        packet_number: PacketNumber,
    ) -> Result<EvictedSet, SlidingWindowError> {
        #[cfg(debug_assertions)]
        let initial = self.clone();

        let res = self.insert_with_evicted_inner(packet_number);

        #[cfg(debug_assertions)]
        self.check_insert_result(packet_number, initial, &res);

        res
    }

    fn insert_with_evicted_inner(
        &mut self,
        packet_number: PacketNumber,
    ) -> Result<EvictedSet, SlidingWindowError> {
        match self.window_position(packet_number) {
            WindowPosition::Left => Err(SlidingWindowError::TooOld),
            WindowPosition::RightEdge => Err(SlidingWindowError::Duplicate),
            WindowPosition::Right(delta) => {
                let removed = if delta < WINDOW_WIDTH {
                    // Keep only the bits that we would have shifted out.
                    let removed_mask = if delta == 128 {
                        u128::MAX
                    } else {
                        !u128::MAX.wrapping_shr(delta as u32)
                    };
                    let removed = !self.window & removed_mask;
                    // Shift by delta.
                    self.window = self.window.checked_shl(delta as u32).unwrap_or(0);
                    // Set the bit for the current right edge
                    self.window |= 1 << (delta - 1);
                    removed
                } else {
                    // The delta is too large, reset the window
                    let removed = self.window;
                    self.window = 0;
                    // Invert the bits, since we want the not present bits.
                    // Our mask is the full window since it's all getting evicted.
                    !removed
                };
                if let Some(prev_right_edge) = self.right_edge.replace(packet_number) {
                    Ok(EvictedSet {
                        window: removed,
                        right_edge: prev_right_edge,
                    })
                } else {
                    // If there was not a right edge, we should not have any bits set.
                    assert!(removed == 0);
                    Ok(EvictedSet::default())
                }
            }
            WindowPosition::Within(delta) => {
                let mask = 1 << (delta - 1); // Shift by the delta - 1 to account for the right edge
                let duplicate = self.window & mask != 0;
                self.window |= mask;
                if duplicate {
                    Err(SlidingWindowError::Duplicate)
                } else {
                    Ok(EvictedSet::default())
                }
            }
            WindowPosition::Empty => {
                self.right_edge = Some(packet_number);
                Ok(EvictedSet::default())
            }
        }
    }

    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    fn check_insert_result(
        &self,
        packet_number: PacketNumber,
        initial: Self,
        res: &Result<EvictedSet, SlidingWindowError>,
    ) {
        let evicted = match res {
            Ok(evicted) => evicted,
            Err(_) => {
                // On error we should not have changed ourselves.
                assert_eq!(self.window, initial.window);
                assert_eq!(self.right_edge, initial.right_edge);
                return;
            }
        };

        {
            for pn in evicted.clone() {
                // If we are returning it as evicted it should be in the initial set.
                assert_eq!(initial.check(pn), Ok(()), "{pn:?}");
                // ... and unknown in the new set, since we've slid past it.
                assert_eq!(self.check(pn), Err(SlidingWindowError::TooOld), "{pn:?}");
            }
        }

        for pn in initial
            .right_edge
            .map_or(0, |e| e.as_u64())
            .saturating_sub(WINDOW_WIDTH)
            ..initial.right_edge.map_or(WINDOW_WIDTH, |e| e.as_u64())
        {
            let pn = PacketNumber::from_varint(VarInt::new(pn).unwrap(), packet_number.space());

            match self.check(pn) {
                // If we still have this, it must not be in evicted.
                Ok(()) => assert!(evicted.clone().all(|e| e != pn)),
                Err(SlidingWindowError::TooOld) => {
                    // If it's too old after insert, but was previously OK *and in the set*,
                    // then it must be in evicted.
                    if initial.check(pn).is_ok()
                        // An Empty set is fine, but isn't meaningful to track evictions for
                        // (FIXME: Maybe we should evict some range of 129 PNs?)
                        && initial.window_position(pn) != WindowPosition::Empty
                    {
                        assert!(
                            evicted.clone().any(|e| e == pn),
                            "{pn:?} expected in evicted after insert of {packet_number:?}"
                        );
                    }
                }
                Err(SlidingWindowError::Duplicate) => {
                    // If a duplicate after, then it must not be in evicted.
                    assert!(evicted.clone().all(|e| e != pn))
                }
            }
        }
    }

    /// Determines if the given packet number has already been inserted or
    /// is too old to determine if it has already been inserted.
    pub fn check(&self, packet_number: PacketNumber) -> Result<(), SlidingWindowError> {
        match self.window_position(packet_number) {
            WindowPosition::Left => Err(SlidingWindowError::TooOld),
            WindowPosition::RightEdge => Err(SlidingWindowError::Duplicate),
            WindowPosition::Right(_) | WindowPosition::Empty => Ok(()),
            WindowPosition::Within(delta) => {
                let mask = 1 << (delta - 1); // Shift by the delta - 1 to account for the right edge
                if self.window & mask != 0 {
                    Err(SlidingWindowError::Duplicate)
                } else {
                    Ok(())
                }
            }
        }
    }

    /// Gets the position of the `packet_number` relative to the window.
    fn window_position(&self, packet_number: PacketNumber) -> WindowPosition {
        if let Some(right_edge) = self.right_edge {
            match right_edge.checked_distance(packet_number) {
                Some(0) => WindowPosition::RightEdge,
                Some(delta) if delta >= WINDOW_WIDTH => WindowPosition::Left,
                Some(delta) => WindowPosition::Within(delta),
                None => WindowPosition::Right(
                    packet_number
                        .checked_distance(right_edge)
                        .expect("packet_number must be greater than right_edge"),
                ),
            }
        } else {
            WindowPosition::Empty
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{packet::number::PacketNumberSpace, varint::VarInt};
    use bolero::{check, generator::*};
    use SlidingWindowError::*;

    /// This macro asserts that the output of inserting the given packet number,
    /// the window, and the right edge match the expected values.
    macro_rules! assert_window {
        (
            $window:expr, $to_insert:expr, $duplicate:expr, $expected_window:expr, $right_edge:expr
        ) => {{
            assert_eq!($window.check($to_insert), $duplicate);
            assert_eq!($window.insert($to_insert), $duplicate);
            assert_eq!(
                $window.window, $expected_window,
                "Expected: {:b}, Actual: {:b}",
                $expected_window, $window.window
            );
            assert_eq!($window.right_edge.unwrap(), $right_edge);
        }};
    }

    #[test]
    #[allow(clippy::cognitive_complexity)] // several operations are needed to get the desired state
    fn insert() {
        let space = PacketNumberSpace::ApplicationData;
        let mut window = SlidingWindow::default();

        let zero = space.new_packet_number(VarInt::from_u8(0));
        let one = space.new_packet_number(VarInt::from_u8(1));
        let two = space.new_packet_number(VarInt::from_u8(2));
        let three = space.new_packet_number(VarInt::from_u8(3));
        let four = space.new_packet_number(VarInt::from_u8(4));
        let five = space.new_packet_number(VarInt::from_u8(5));
        let six = space.new_packet_number(VarInt::from_u8(6));
        let seven = space.new_packet_number(VarInt::from_u8(7));
        let eight = space.new_packet_number(VarInt::from_u8(8));
        let large = space.new_packet_number(VarInt::MAX);

        assert_eq!(window.window, Window::default());
        assert_eq!(window.right_edge, None);

        assert_window!(window, zero, Ok(()), Window::default(), zero);
        assert_window!(window, zero, Err(Duplicate), Window::default(), zero);
        assert_window!(window, one, Ok(()), 0b1, one);
        assert_window!(window, one, Err(Duplicate), 0b1, one);
        assert_window!(window, two, Ok(()), 0b11, two);
        assert_window!(window, five, Ok(()), 0b11100, five);
        assert_window!(window, eight, Ok(()), 0b1110_0100, eight);
        assert_window!(window, seven, Ok(()), 0b1110_0101, eight);
        assert_window!(window, three, Ok(()), 0b1111_0101, eight);
        assert_window!(window, six, Ok(()), 0b1111_0111, eight);
        assert_window!(window, four, Ok(()), 0b1111_1111, eight);
        assert_window!(window, seven, Err(Duplicate), 0b1111_1111, eight);
        assert_window!(window, two, Err(Duplicate), 0b1111_1111, eight);
        assert_window!(window, eight, Err(Duplicate), 0b1111_1111, eight);
        assert_window!(window, large, Ok(()), Window::default(), large);
        assert_window!(window, five, Err(TooOld), Window::default(), large);
    }

    #[test]
    #[cfg_attr(miri, ignore)] // this test is too expensive for miri
    fn incremental_insert() {
        let mut window = SlidingWindow::default();
        let space = PacketNumberSpace::ApplicationData;
        for right_edge in 0..1000 {
            let pn = space.new_packet_number(VarInt::from_u32(right_edge));
            assert_eq!(window.check(pn), Ok(()));
            assert_eq!(window.insert(pn), Ok(()));
            assert_eq!(window.right_edge.unwrap(), pn);
            for dup in 0..=right_edge {
                let expected_error = if right_edge - dup < WINDOW_WIDTH as u32 {
                    Err(Duplicate)
                } else {
                    Err(TooOld)
                };
                let dup_pn = space.new_packet_number(VarInt::from_u32(dup));
                assert_eq!(window.check(dup_pn), expected_error);
                assert_eq!(window.insert(dup_pn), expected_error);
            }
        }
    }

    #[test]
    #[allow(clippy::cognitive_complexity)] // several comparisons are needed
    fn insert_at_edge() {
        let mut window = SlidingWindow::default();
        let space = PacketNumberSpace::ApplicationData;
        let zero = space.new_packet_number(VarInt::from_u8(0));
        let window_width_minus_1 = space.new_packet_number(VarInt::new(WINDOW_WIDTH - 1).unwrap());
        let window_width = window_width_minus_1.next().unwrap();

        assert_window!(window, zero, Ok(()), Window::default(), zero);
        assert_window!(
            window,
            window_width_minus_1,
            Ok(()),
            (1_u128) << 127,
            window_width_minus_1
        );
        assert_window!(
            window,
            window_width_minus_1,
            Err(Duplicate),
            (1_u128) << 127,
            window_width_minus_1
        );
        assert_window!(window, window_width, Ok(()), 0b1, window_width);

        window = SlidingWindow::default();
        assert_window!(window, zero, Ok(()), Window::default(), zero);
        assert_window!(
            window,
            window_width,
            Ok(()),
            Window::default(),
            window_width
        );
        assert_window!(
            window,
            window_width,
            Err(Duplicate),
            Window::default(),
            window_width
        );
    }

    #[test]
    fn delta_larger_than_32_bits() {
        let mut window = SlidingWindow::default();
        let space = PacketNumberSpace::ApplicationData;
        let zero = space.new_packet_number(VarInt::from_u8(0));
        let large = space.new_packet_number(VarInt::new((1 << 32) + 1).unwrap());
        assert_eq!(window.check(zero), Ok(()));
        assert_eq!(window.insert(zero), Ok(()));
        assert_eq!(window.check(large), Ok(()));
        assert_eq!(window.insert(large), Ok(()));
        assert_eq!(window.check(large), Err(Duplicate));
        assert_eq!(window.insert(large), Err(Duplicate));
        assert_eq!(window.window, 0b0);
    }

    // Inserting into an empty set shouldn't trigger eviction errors.
    //
    // Found via fuzzing, but extracting as a dedicated test case.
    #[test]
    fn insert_into_empty() {
        let pn = VarInt::from_u32(256);
        let mut window = SlidingWindow::default();
        let space = PacketNumberSpace::ApplicationData;
        let packet_number = space.new_packet_number(pn);
        assert!(window.insert(packet_number).is_ok());
    }

    #[test]
    #[cfg_attr(kani, kani::proof, kani::unwind(130), kani::solver(kissat))]
    #[cfg_attr(miri, ignore)] // this test is too expensive for miri
    fn insert_test() {
        // Make sure the two packet numbers are not the same
        let gen = produce::<(VarInt, VarInt)>().filter_gen(|(a, b)| a != b);

        check!()
            .with_generator(gen)
            .cloned()
            .for_each(|(pn, other_pn)| {
                let mut window = SlidingWindow::default();
                let space = PacketNumberSpace::ApplicationData;
                let packet_number = space.new_packet_number(pn);
                let other_packet_number = space.new_packet_number(other_pn);
                assert!(window.insert(packet_number).is_ok());
                assert_eq!(Err(Duplicate), window.check(packet_number));
                assert_ne!(Err(Duplicate), window.check(other_packet_number));
            });
    }

    // This is a basic test to make sure it mostly works.
    //
    // We also do fairly exhaustive checking as part of every insert that the evicted set we've
    // returned matches our expectations.
    #[test]
    #[allow(clippy::cognitive_complexity)] // several operations are needed to get the desired state
    fn insert_evicted() {
        let space = PacketNumberSpace::ApplicationData;
        let mut window = SlidingWindow::default();

        let zero = space.new_packet_number(VarInt::from_u8(0));
        let one = space.new_packet_number(VarInt::from_u8(1));
        let two = space.new_packet_number(VarInt::from_u8(2));
        let three = space.new_packet_number(VarInt::from_u8(3));
        let four = space.new_packet_number(VarInt::from_u8(4));
        let five = space.new_packet_number(VarInt::from_u8(5));
        let six = space.new_packet_number(VarInt::from_u8(6));
        let seven = space.new_packet_number(VarInt::from_u8(7));
        let eight = space.new_packet_number(VarInt::from_u8(8));
        let large = space.new_packet_number(VarInt::MAX);

        assert!(window.insert(zero).is_ok());
        assert!(window.insert(two).is_ok());
        assert!(window.insert(four).is_ok());
        assert!(window.insert(five).is_ok());
        assert!(window.insert(seven).is_ok());
        assert!(window.insert(eight).is_ok());

        let mut evicted = window.insert_with_evicted(large).unwrap();
        assert_eq!(evicted.next(), Some(one));
        assert_eq!(evicted.next(), Some(three));
        assert_eq!(evicted.next(), Some(six));
        assert_eq!(evicted.next(), None);
    }
}

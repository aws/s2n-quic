// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::packet::number::PacketNumber;
use core::mem;

#[derive(Default, Debug)]
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
        match self.window_position(packet_number) {
            WindowPosition::Left => Err(SlidingWindowError::TooOld),
            WindowPosition::RightEdge => Err(SlidingWindowError::Duplicate),
            WindowPosition::Right(delta) => {
                if delta < WINDOW_WIDTH {
                    // Make room for the current right edge
                    self.window <<= 1;
                    // Set the bit for the current right edge
                    self.window |= 1;
                    // Shift by delta while taking account of the right edge
                    self.window <<= delta - 1;
                } else {
                    // The delta is too large, reset the window
                    self.window = Window::default();
                }
                self.right_edge = Some(packet_number);
                Ok(())
            }
            WindowPosition::Within(delta) => {
                let mask = 1 << (delta - 1); // Shift by the delta - 1 to account for the right edge
                let duplicate = self.window & mask != 0;
                self.window |= mask;
                if duplicate {
                    Err(SlidingWindowError::Duplicate)
                } else {
                    Ok(())
                }
            }
            WindowPosition::Empty => {
                self.right_edge = Some(packet_number);
                Ok(())
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

    #[test]
    #[cfg_attr(kani, kani::proof, kani::unwind(2), kani::solver(kissat))]
    #[cfg_attr(miri, ignore)] // this test is too expensive for miri
    fn insert_test() {
        // Make sure the two packet numbers are not the same
        let gen = gen::<(VarInt, VarInt)>().filter_gen(|(a, b)| a != b);

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
}

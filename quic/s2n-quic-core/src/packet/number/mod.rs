// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

mod sliding_window;
pub use sliding_window::{AsEvent as SlidingWindowAsEvent, SlidingWindow};

mod protected_packet_number;
pub use protected_packet_number::ProtectedPacketNumber;

//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#12.3
//# The packet number is an integer in the range 0 to 2^62-1.  This
//# number is used in determining the cryptographic nonce for packet
//# protection.  Each endpoint maintains a separate packet number for
//# sending and receiving.

use crate::varint::VarInt;

mod packet_number;
pub use packet_number::{AsEvent as PacketNumberAsEvent, PacketNumber};

//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#12.3
//# Packet numbers are limited to this range because they need to be
//# representable in whole in the Largest Acknowledged field of an ACK
//# frame (Section 19.3).  When present in a long or short header
//# however, packet numbers are reduced and encoded in 1 to 4 bytes; see
//# Section 17.1.

mod truncated_packet_number;
pub use truncated_packet_number::TruncatedPacketNumber;

//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#12.3
//# *  Initial space: All Initial packets (Section 17.2.2) are in this
//#    space.
//#
//# *  Handshake space: All Handshake packets (Section 17.2.4) are in
//#    this space.
//#
//# *  Application data space: All 0-RTT (Section 17.2.3) and 1-RTT
//#    (Section 17.3) encrypted packets are in this space.

mod packet_number_space;
pub use packet_number_space::PacketNumberSpace;

//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#17.1
//# Packet numbers are integers in the range 0 to 2^62-1 (Section 12.3).
//# When present in long or short packet headers, they are encoded in 1
//# to 4 bytes.  The number of bits required to represent the packet
//# number is reduced by including only the least significant bits of the
//# packet number.

/// The packet number len is the two least significant bits of the packet tag
pub(crate) const PACKET_NUMBER_LEN_MASK: u8 = 0b11;

mod packet_number_len;
pub use packet_number_len::PacketNumberLen;

mod packet_number_range;
pub use packet_number_range::PacketNumberRange;

#[cfg(feature = "alloc")]
pub mod map;
#[cfg(feature = "alloc")]
pub use map::Map;

//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#17.1
//# the sender MUST use a packet number size able to represent more than
//# twice as large a range than the difference between the largest
//# acknowledged packet and packet number being sent.  A peer receiving
//# the packet will then correctly decode the packet number, unless the
//# packet is delayed in transit such that it arrives after many higher-
//# numbered packets have been received.  An endpoint SHOULD use a large
//# enough packet number encoding to allow the packet number to be
//# recovered even if the packet arrives after packets that are sent
//# afterwards.

fn derive_truncation_range(
    largest_acknowledged_packet_number: PacketNumber,
    packet_number: PacketNumber,
) -> Option<PacketNumberLen> {
    let space = packet_number.space();
    space.assert_eq(largest_acknowledged_packet_number.space());
    packet_number
        .as_u64()
        .checked_sub(largest_acknowledged_packet_number.as_u64())
        .and_then(|value| value.checked_mul(2))
        .and_then(|value| VarInt::new(value).ok())
        .and_then(|value| PacketNumberLen::from_varint(value, space))
}

//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#17.1
//# As a result, the size of the packet number encoding is at least one
//# bit more than the base-2 logarithm of the number of contiguous
//# unacknowledged packet numbers, including the new packet.
//#
//# For example, if an endpoint has received an acknowledgment for packet
//# 0xabe8bc, sending a packet with a number of 0xac5c02 requires a
//# packet number encoding with 16 bits or more; whereas the 24-bit
//# packet number encoding is needed to send a packet with a number of
//# 0xace8fe.

#[test]
fn packet_number_len_example_test() {
    let largest_acknowledged_packet_number =
        PacketNumberSpace::default().new_packet_number(VarInt::from_u32(0x00ab_e8bc));

    assert_eq!(
        PacketNumberSpace::default()
            .new_packet_number(VarInt::from_u32(0x00ac_5c02))
            .truncate(largest_acknowledged_packet_number)
            .unwrap()
            .bitsize(),
        16,
    );

    assert_eq!(
        PacketNumberSpace::default()
            .new_packet_number(VarInt::from_u32(0x00ac_e8fe))
            .truncate(largest_acknowledged_packet_number)
            .unwrap()
            .bitsize(),
        24,
    );
}

//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#17.1
//# At a receiver, protection of the packet number is removed prior to
//# recovering the full packet number.  The full packet number is then
//# reconstructed based on the number of significant bits present, the
//# value of those bits, and the largest packet number received on a
//# successfully authenticated packet.  Recovering the full packet number
//# is necessary to successfully remove packet protection.
//#
//# Once header protection is removed, the packet number is decoded by
//# finding the packet number value that is closest to the next expected
//# packet.  The next expected packet is the highest received packet
//# number plus one.  For example, if the highest successfully
//# authenticated packet had a packet number of 0xa82f30ea, then a packet
//# containing a 16-bit value of 0x9b32 will be decoded as 0xa82f9b32.
//# Example pseudo-code for packet number decoding can be found in
//# Appendix A.

#[test]
fn packet_decoding_example_test() {
    let space = PacketNumberSpace::default();
    let largest_packet_number = space.new_packet_number(VarInt::from_u32(0xa82f_30ea));
    let truncated_packet_number = TruncatedPacketNumber::new(0x9b32u16, space);
    let expected = space.new_packet_number(VarInt::from_u32(0xa82f_9b32));
    let actual = decode_packet_number(largest_packet_number, truncated_packet_number);
    assert_eq!(actual, expected);
    assert_eq!(
        expected.truncate(largest_packet_number).unwrap(),
        truncated_packet_number
    );
}

//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#A
//# DecodePacketNumber(largest_pn, truncated_pn, pn_nbits):
//#    expected_pn  = largest_pn + 1
//#    pn_win       = 1 << pn_nbits
//#    pn_hwin      = pn_win / 2
//#    pn_mask      = pn_win - 1
//#    // The incoming packet number should be greater than
//#    // expected_pn - pn_hwin and less than or equal to
//#    // expected_pn + pn_hwin
//#    //
//#    // This means we cannot just strip the trailing bits from
//#    // expected_pn and add the truncated_pn because that might
//#    // yield a value outside the window.
//#    //
//#    // The following code calculates a candidate value and
//#    // makes sure it's within the packet number window.
//#    // Note the extra checks to prevent overflow and underflow.
//#    candidate_pn = (expected_pn & ~pn_mask) | truncated_pn
//#    if candidate_pn <= expected_pn - pn_hwin and
//#       candidate_pn < (1 << 62) - pn_win:
//#       return candidate_pn + pn_win
//#    if candidate_pn > expected_pn + pn_hwin and
//#       candidate_pn >= pn_win:
//#       return candidate_pn - pn_win
//#    return candidate_pn

fn decode_packet_number(
    largest_pn: PacketNumber,
    truncated_pn: TruncatedPacketNumber,
) -> PacketNumber {
    use crate::ct::{ConditionallySelectable, Number};

    let space = largest_pn.space();
    space.assert_eq(truncated_pn.space());

    let pn_nbits = truncated_pn.bitsize();
    // deref to u64 so we have enough room
    let expected_pn = largest_pn.as_u64() + 1;
    let pn_win = 1 << pn_nbits;
    let pn_hwin = pn_win / 2;
    let pn_mask = pn_win - 1;
    let candidate_pn = (expected_pn & !pn_mask) | truncated_pn.into_u64();

    // convert numbers into checked Number
    let expected_pn = Number::new(expected_pn);
    let mut candidate_pn = Number::new(candidate_pn);

    let a_value = candidate_pn + pn_win;
    let a_choice = candidate_pn.ct_le(expected_pn - pn_hwin)
        & a_value.ct_le(Number::new(VarInt::MAX.as_u64()));

    let b_value = candidate_pn - pn_win;
    let b_choice = candidate_pn.ct_gt(expected_pn + pn_hwin) & b_value.is_valid();

    // apply the choices in reverse since it's easier to emulate the early returns
    // with the `conditional_assign` calls
    candidate_pn.conditional_assign(&b_value, b_choice);
    candidate_pn.conditional_assign(&a_value, a_choice);

    let candidate_pn = candidate_pn.unwrap_or_default().min(VarInt::MAX.as_u64());

    let candidate_pn = unsafe {
        // Safety: the value has already been checked in constant time above
        debug_assert!(candidate_pn <= VarInt::MAX.as_u64());
        VarInt::new_unchecked(candidate_pn)
    };

    PacketNumber::from_varint(candidate_pn, space)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bolero::check;

    fn new(value: VarInt) -> PacketNumber {
        PacketNumberSpace::Initial.new_packet_number(value)
    }

    /// This implementation tries to closely follow the RFC psuedo code so it's
    /// easier to ensure it matches.
    #[allow(clippy::blocks_in_if_conditions)]
    fn rfc_decoder(largest_pn: u64, truncated_pn: u64, pn_nbits: usize) -> u64 {
        use std::panic::catch_unwind as catch;

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#A
        //= type=test
        //# DecodePacketNumber(largest_pn, truncated_pn, pn_nbits):
        //#    expected_pn  = largest_pn + 1
        //#    pn_win       = 1 << pn_nbits
        //#    pn_hwin      = pn_win / 2
        //#    pn_mask      = pn_win - 1
        //#    // The incoming packet number should be greater than
        //#    // expected_pn - pn_hwin and less than or equal to
        //#    // expected_pn + pn_hwin
        //#    //
        //#    // This means we cannot just strip the trailing bits from
        //#    // expected_pn and add the truncated_pn because that might
        //#    // yield a value outside the window.
        //#    //
        //#    // The following code calculates a candidate value and
        //#    // makes sure it's within the packet number window.
        //#    // Note the extra checks to prevent overflow and underflow.
        //#    candidate_pn = (expected_pn & ~pn_mask) | truncated_pn
        //#    if candidate_pn <= expected_pn - pn_hwin and
        //#       candidate_pn < (1 << 62) - pn_win:
        //#       return candidate_pn + pn_win
        //#    if candidate_pn > expected_pn + pn_hwin and
        //#       candidate_pn >= pn_win:
        //#       return candidate_pn - pn_win
        //#    return candidate_pn
        let expected_pn = largest_pn + 1;
        let pn_win = 1 << pn_nbits;
        let pn_hwin = pn_win / 2;
        let pn_mask = pn_win - 1;

        let candidate_pn = (expected_pn & !pn_mask) | truncated_pn;
        if catch(|| {
            candidate_pn <= expected_pn.checked_sub(pn_hwin).unwrap()
                && candidate_pn < (1u64 << 62).checked_sub(pn_win).unwrap()
        })
        .unwrap_or_default()
        {
            return candidate_pn + pn_win;
        }

        if catch(|| {
            candidate_pn > expected_pn.checked_add(pn_hwin).unwrap() && candidate_pn >= pn_win
        })
        .unwrap_or_default()
        {
            return candidate_pn - pn_win;
        }

        candidate_pn
    }

    #[test]
    fn truncate_expand_test() {
        check!()
            .with_type()
            .cloned()
            .for_each(|(largest_pn, expected_pn)| {
                let largest_pn = new(largest_pn);
                let expected_pn = new(expected_pn);
                if let Some(truncated_pn) = expected_pn.truncate(largest_pn) {
                    assert_eq!(expected_pn, truncated_pn.expand(largest_pn));
                }
            });
    }

    #[test]
    fn rfc_differential_test() {
        check!()
            .with_type()
            .cloned()
            .for_each(|(largest_pn, truncated_pn)| {
                let largest_pn = new(largest_pn);
                let space = largest_pn.space();
                let truncated_pn = TruncatedPacketNumber {
                    space,
                    value: truncated_pn,
                };
                let rfc_value = rfc_decoder(
                    largest_pn.as_u64(),
                    truncated_pn.into_u64(),
                    truncated_pn.bitsize(),
                )
                .min(VarInt::MAX.as_u64());
                let actual_value = truncated_pn.expand(largest_pn).as_u64();

                assert_eq!(
                    actual_value,
                    rfc_value,
                    "diff: {}",
                    actual_value
                        .checked_sub(rfc_value)
                        .unwrap_or_else(|| rfc_value - actual_value)
                );
            });
    }

    #[test]
    fn example_test() {
        macro_rules! example {
            ($largest:expr, $truncated:expr, $expected:expr) => {{
                let largest = new(VarInt::from_u32($largest));
                let truncated = TruncatedPacketNumber::new($truncated, PacketNumberSpace::Initial);
                let expected = new(VarInt::from_u32($expected));
                assert_eq!(truncated.expand(largest), expected);
            }};
        }

        example!(0xa82e1b31, 0x9b32u16, 0xa82e9b32);
    }

    #[test]
    #[cfg_attr(miri, ignore)] // snapshot tests don't work on miri
    fn size_of_snapshots() {
        use core::mem::size_of;
        use insta::assert_debug_snapshot;

        assert_debug_snapshot!("PacketNumber", size_of::<PacketNumber>());
        assert_debug_snapshot!("PacketNumberLen", size_of::<PacketNumberLen>());
        assert_debug_snapshot!("PacketNumberSpace", size_of::<PacketNumberSpace>());
        assert_debug_snapshot!("ProtectedPacketNumber", size_of::<ProtectedPacketNumber>());
        assert_debug_snapshot!("TruncatedPacketNumber", size_of::<TruncatedPacketNumber>());
    }
}

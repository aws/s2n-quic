// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

mod sliding_window;
pub use sliding_window::{SlidingWindow, SlidingWindowError};

mod protected_packet_number;
pub use protected_packet_number::ProtectedPacketNumber;

//= https://www.rfc-editor.org/rfc/rfc9000#section-12.3
//# The packet number is an integer in the range 0 to 2^62-1.  This
//# number is used in determining the cryptographic nonce for packet
//# protection.  Each endpoint maintains a separate packet number for
//# sending and receiving.

use crate::varint::VarInt;

mod packet_number;
pub use packet_number::PacketNumber;

//= https://www.rfc-editor.org/rfc/rfc9000#section-12.3
//# Packet numbers are limited to this range because they need to be
//# representable in whole in the Largest Acknowledged field of an ACK
//# frame (Section 19.3).  When present in a long or short header
//# however, packet numbers are reduced and encoded in 1 to 4 bytes; see
//# Section 17.1.

mod truncated_packet_number;
pub use truncated_packet_number::TruncatedPacketNumber;

//= https://www.rfc-editor.org/rfc/rfc9000#section-12.3
//# Initial space:  All Initial packets (Section 17.2.2) are in this
//# space.
//#
//# Handshake space:  All Handshake packets (Section 17.2.4) are in this
//# space.
//#
//# Application data space:  All 0-RTT (Section 17.2.3) and 1-RTT
//# (Section 17.3.1) packets are in this space.

mod packet_number_space;
pub use packet_number_space::PacketNumberSpace;

//= https://www.rfc-editor.org/rfc/rfc9000#section-17.1
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

#[cfg(test)]
mod tests;

//= https://www.rfc-editor.org/rfc/rfc9000#section-17.1
//# the sender MUST use a packet number size able to represent more than
//# twice as large a range as the difference between the largest
//# acknowledged packet number and the packet number being sent.  A peer
//# receiving the packet will then correctly decode the packet number,
//# unless the packet is delayed in transit such that it arrives after
//# many higher-numbered packets have been received.  An endpoint SHOULD
//# use a large enough packet number encoding to allow the packet number
//# to be recovered even if the packet arrives after packets that are
//# sent afterwards.

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

//= https://www.rfc-editor.org/rfc/rfc9000#section-17.1
//# As a result, the size of the packet number encoding is at least one
//# bit more than the base-2 logarithm of the number of contiguous
//# unacknowledged packet numbers, including the new packet.

//= https://www.rfc-editor.org/rfc/rfc9000#appendix-A.2
//# For example, if an endpoint has received an acknowledgment for packet
//# 0xabe8b3 and is sending a packet with a number of 0xac5c02, there are
//# 29,519 (0x734f) outstanding packet numbers.  In order to represent at
//# least twice this range (59,038 packets, or 0xe69e), 16 bits are
//# required.
//#
//# In the same state, sending a packet with a number of 0xace8fe uses
//# the 24-bit encoding, because at least 18 bits are required to
//# represent twice the range (131,222 packets, or 0x020096).

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

//= https://www.rfc-editor.org/rfc/rfc9000#section-17.1
//# At a receiver, protection of the packet number is removed prior to
//# recovering the full packet number.  The full packet number is then
//# reconstructed based on the number of significant bits present, the
//# value of those bits, and the largest packet number received in a
//# successfully authenticated packet.  Recovering the full packet number
//# is necessary to successfully complete the removal of packet
//# protection.
//
//# Once header protection is removed, the packet number is decoded by
//# finding the packet number value that is closest to the next expected
//# packet.  The next expected packet is the highest received packet
//# number plus one.  Pseudocode and an example for packet number
//# decoding can be found in Appendix A.3.

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

//= https://www.rfc-editor.org/rfc/rfc9000#appendix-A.3
//# DecodePacketNumber(largest_pn, truncated_pn, pn_nbits):
//#   expected_pn  = largest_pn + 1
//#   pn_win       = 1 << pn_nbits
//#   pn_hwin      = pn_win / 2
//#   pn_mask      = pn_win - 1
//#   // The incoming packet number should be greater than
//#   // expected_pn - pn_hwin and less than or equal to
//#   // expected_pn + pn_hwin
//#   //
//#   // This means we cannot just strip the trailing bits from
//#   // expected_pn and add the truncated_pn because that might
//#   // yield a value outside the window.
//#   //
//#   // The following code calculates a candidate value and
//#   // makes sure it's within the packet number window.
//#   // Note the extra checks to prevent overflow and underflow.
//#   candidate_pn = (expected_pn & ~pn_mask) | truncated_pn
//#   if candidate_pn <= expected_pn - pn_hwin and
//#     candidate_pn < (1 << 62) - pn_win:
//#     return candidate_pn + pn_win
//#   if candidate_pn > expected_pn + pn_hwin and
//#     candidate_pn >= pn_win:
//#     return candidate_pn - pn_win
//#   return candidate_pn

#[inline(never)] // prevent the compiler from optimizing call size values and making this non-constant time
fn decode_packet_number(
    largest_pn: PacketNumber,
    truncated_pn: TruncatedPacketNumber,
) -> PacketNumber {
    let space = largest_pn.space();
    space.assert_eq(truncated_pn.space());

    let pn_nbits = truncated_pn.bitsize();
    // deref to u64 so we have enough room
    let expected_pn = largest_pn.as_u64() + 1;
    let pn_win = 1 << pn_nbits;
    let pn_hwin = pn_win / 2;
    let pn_mask = pn_win - 1;
    let mut candidate_pn = (expected_pn & !pn_mask) | truncated_pn.into_u64();

    let a = expected_pn
        .checked_sub(pn_hwin)
        .filter(|v| candidate_pn <= *v)
        .is_some();
    let b = (1u64 << 62)
        .checked_sub(pn_win)
        .filter(|v| candidate_pn < *v)
        .is_some();
    let c = expected_pn
        .checked_add(pn_hwin)
        .filter(|v| candidate_pn > *v)
        .is_some();
    let d = candidate_pn >= pn_win;

    let ab = a && b;
    let cd = !ab && c && d;

    // Make sure the compiler doesn't try and optimize the if statements
    // See https://godbolt.org/z/348WbbbzM
    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);

    if ab {
        candidate_pn += pn_win;
    }

    if cd {
        candidate_pn -= pn_win
    }

    let candidate_pn = VarInt::new(candidate_pn).unwrap_or(VarInt::MAX);

    PacketNumber::from_varint(candidate_pn, space)
}

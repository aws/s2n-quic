mod sliding_window;
pub use sliding_window::*;

mod protected_packet_number;
pub use protected_packet_number::ProtectedPacketNumber;

//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#12.3
//# The packet number is an integer in the range 0 to 2^62-1.  This
//# number is used in determining the cryptographic nonce for packet
//# protection.  Each endpoint maintains a separate packet number for
//# sending and receiving.

use crate::varint::VarInt;

mod packet_number;
pub use packet_number::PacketNumber;

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
    let actual = decode_packet_number(largest_packet_number, truncated_packet_number).unwrap();
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
) -> Option<PacketNumber> {
    let space = largest_pn.space();
    space.assert_eq(truncated_pn.space());

    let pn_nbits = truncated_pn.bitsize();
    // deref to u64 so we have enough room
    let expected_pn = largest_pn.as_u64() + 1;
    let pn_win = 1 << pn_nbits;
    let pn_hwin = pn_win / 2;
    let pn_mask = pn_win - 1;
    let candidate_pn = (expected_pn & !pn_mask) | truncated_pn.into_u64();

    if let Some(packet_number) = expected_pn
        .checked_sub(pn_hwin)
        .filter(|window| candidate_pn <= *window)
        .and_then(|_| candidate_pn.checked_add(pn_win))
        .and_then(|value| VarInt::new(value).ok())
        .map(|value| PacketNumber::from_varint(value, space))
    {
        return Some(packet_number);
    }

    if let Some(pn) = expected_pn
        .checked_add(pn_hwin)
        .filter(|window| candidate_pn >= *window)
        .and_then(|_| candidate_pn.checked_sub(pn_win))
        .and_then(|value| VarInt::new(value).ok())
        .map(|value| PacketNumber::from_varint(value, space))
    {
        return Some(pn);
    }

    Some(PacketNumber::from_varint(
        VarInt::new(candidate_pn).ok()?,
        space,
    ))
}

#[test]
fn decode_packet_number_test() {
    // Brute-force test the first 2048 packet numbers and
    // assert round trip truncation and expansion
    //
    // In the case we're using miri, shrink this down to reduce the cost of it
    let iterations = if cfg!(miri) { 16 } else { 2048 };

    fn new(value: u64) -> PacketNumber {
        PacketNumberSpace::Initial.new_packet_number(VarInt::new(value).unwrap())
    }

    for largest_pn in (0..iterations).map(new) {
        for expected_pn in (largest_pn.as_u64()..iterations).map(new) {
            let truncated_pn = expected_pn.truncate(largest_pn).unwrap();

            assert_eq!(
                expected_pn,
                decode_packet_number(largest_pn, truncated_pn).unwrap(),
            );
        }
    }
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

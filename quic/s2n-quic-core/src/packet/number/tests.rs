// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::{
    packet::number::{PacketNumber, PacketNumberSpace},
    varint::VarInt,
};
use bolero::{check, generator::*};
use s2n_codec::{testing::encode, DecoderBuffer};

#[test]
#[cfg_attr(kani, kani::proof, kani::unwind(5), kani::solver(cadical))]
fn round_trip() {
    check!()
        .with_generator(
            gen_packet_number_space()
                .and_then_gen(|space| (gen_packet_number(space), gen_packet_number(space))),
        )
        .cloned()
        .for_each(|(packet_number, largest_acked_packet_number)| {
            // Try to encode the packet number to send
            if let Some((mask, bytes)) =
                encode_packet_number(packet_number, largest_acked_packet_number)
            {
                // If encoding was valid, assert that the information can be decoded
                let actual_packet_number =
                    decode_packet_number(mask, bytes, largest_acked_packet_number).unwrap();
                assert_eq!(actual_packet_number, packet_number);
            }
        });
}

fn gen_packet_number_space() -> impl ValueGenerator<Output = PacketNumberSpace> {
    (0u8..=2).map_gen(|id| match id {
        0 => PacketNumberSpace::Initial,
        1 => PacketNumberSpace::Handshake,
        2 => PacketNumberSpace::ApplicationData,
        _ => unreachable!("invalid space id {:?}", id),
    })
}

fn gen_packet_number(space: PacketNumberSpace) -> impl ValueGenerator<Output = PacketNumber> {
    gen().map_gen(move |packet_number| {
        space.new_packet_number(match VarInt::new(packet_number) {
            Ok(packet_number) => packet_number,
            Err(_) => VarInt::from_u32(packet_number as u32),
        })
    })
}

fn encode_packet_number(
    packet_number: PacketNumber,
    largest_acked_packet_number: PacketNumber,
) -> Option<(u8, Vec<u8>)> {
    let truncated_packet_number = packet_number.truncate(largest_acked_packet_number)?;

    let bytes = encode(&truncated_packet_number).unwrap();
    let mask = truncated_packet_number.len().into_packet_tag_mask();

    Some((mask, bytes))
}

fn decode_packet_number(
    packet_tag: u8,
    packet_bytes: Vec<u8>,
    largest_acked_packet_number: PacketNumber,
) -> Result<PacketNumber, String> {
    // decode the packet number len from the packet tag
    let packet_number_len = largest_acked_packet_number
        .space()
        .new_packet_number_len(packet_tag);

    // make sure the packet_tag has the same mask as the len
    assert_eq!(packet_number_len.into_packet_tag_mask(), packet_tag);
    assert_eq!(packet_number_len.bytesize(), packet_bytes.len());

    // try decoding the truncated packet number from the packet bytes
    let (truncated_packet_number, _) = packet_number_len
        .decode_truncated_packet_number(DecoderBuffer::new(&packet_bytes))
        .map_err(|err| err.to_string())?;

    // make sure the packet_number_len round trips
    assert_eq!(truncated_packet_number.len(), packet_number_len);

    // make sure the encoding matches the original bytes
    assert_eq!(packet_bytes, encode(&truncated_packet_number).unwrap());

    // try expanding the truncated packet number
    let packet_number = truncated_packet_number.expand(largest_acked_packet_number);

    // try truncating the packet number
    let actual_truncated_packet_number = packet_number
        .truncate(largest_acked_packet_number)
        .ok_or_else(|| "Could not truncate packet number".to_string())?;

    assert_eq!(actual_truncated_packet_number, truncated_packet_number);

    Ok(packet_number)
}

fn new(value: VarInt) -> PacketNumber {
    PacketNumberSpace::Initial.new_packet_number(value)
}

/// This implementation tries to closely follow the RFC pseudo code so it's
/// easier to ensure it matches.
#[allow(clippy::blocks_in_if_conditions)]
fn rfc_decoder(largest_pn: u64, truncated_pn: u64, pn_nbits: usize) -> u64 {
    macro_rules! catch {
        ($expr:expr) => {
            (|| Some($expr))().unwrap_or(false)
        };
    }

    //= https://www.rfc-editor.org/rfc/rfc9000#appendix-A.3
    //= type=test
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
    let expected_pn = largest_pn + 1;
    let pn_win = 1 << pn_nbits;
    let pn_hwin = pn_win / 2;
    let pn_mask = pn_win - 1;

    let candidate_pn = (expected_pn & !pn_mask) | truncated_pn;
    if catch!(
        candidate_pn <= expected_pn.checked_sub(pn_hwin)?
            && candidate_pn < (1u64 << 62).checked_sub(pn_win)?
    ) {
        return candidate_pn + pn_win;
    }

    if catch!(candidate_pn > expected_pn.checked_add(pn_hwin)? && candidate_pn >= pn_win) {
        return candidate_pn - pn_win;
    }

    candidate_pn
}

#[test]
#[cfg_attr(kani, kani::proof, kani::unwind(5), kani::solver(kissat))]
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
#[cfg_attr(kani, kani::proof, kani::unwind(5), kani::solver(kissat))]
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
                "diff: {:?}",
                actual_value
                    .checked_sub(rfc_value)
                    .or_else(|| rfc_value.checked_sub(actual_value))
            );
        });
}

#[test]
#[cfg_attr(kani, kani::proof, kani::unwind(5), kani::solver(kissat))]
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

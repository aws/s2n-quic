use bolero::{check, generator::*};
use s2n_codec::{testing::encode, DecoderBuffer};
use s2n_quic_core::{
    packet::number::{PacketNumber, PacketNumberSpace},
    VarInt,
};

fn main() {
    check!()
        .with_generator(
            gen_packet_number_space()
                .and_then(|space| (gen_packet_number(space), gen_packet_number(space))),
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
    gen().map(move |packet_number| {
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
    let packet_number = truncated_packet_number
        .expand(largest_acked_packet_number)
        .ok_or_else(|| "Could not expand truncated packet number".to_string())?;

    // try truncating the packet number
    let actual_truncated_packet_number = packet_number
        .truncate(largest_acked_packet_number)
        .ok_or_else(|| "Could not truncate packet number".to_string())?;

    assert_eq!(actual_truncated_packet_number, truncated_packet_number);

    Ok(packet_number)
}

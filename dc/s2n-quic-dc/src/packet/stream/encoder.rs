use crate::{
    crypto::encrypt,
    packet::stream::{self, Tag},
};
use s2n_codec::{u24, Encoder, EncoderBuffer, EncoderValue};
use s2n_quic_core::{
    assume,
    buffer::{self, reader::storage::Infallible as _},
    varint::VarInt,
};

// TODO make sure this is accurate
pub const MAX_RETRANSMISSION_HEADER_LEN: usize = MAX_HEADER_LEN + (24 / 8);
pub const MAX_HEADER_LEN: usize = 50;

#[inline(always)]
#[allow(clippy::too_many_arguments)]
pub fn encode<H, CD, P, C>(
    mut encoder: EncoderBuffer,
    source_control_port: u16,
    source_stream_port: Option<u16>,
    stream_id: stream::Id,
    packet_number: VarInt,
    next_expected_control_packet: VarInt,
    header_len: VarInt,
    header: &mut H,
    control_data_len: VarInt,
    control_data: &CD,
    payload: &mut P,
    crypto: &C,
) -> usize
where
    H: buffer::reader::Storage<Error = core::convert::Infallible>,
    P: buffer::Reader<Error = core::convert::Infallible>,
    CD: EncoderValue,
    C: encrypt::Key,
{
    let stream_offset = payload.current_offset();
    let final_offset = payload.final_offset();

    let mut tag = Tag::default();

    if *control_data_len > 0 {
        tag.set_has_control_data(true);
    }

    if final_offset.is_some() {
        tag.set_has_final_offset(true);
    }

    if *header_len > 0 {
        tag.set_has_application_header(true);
    }

    if source_stream_port.is_some() {
        tag.set_has_source_stream_port(true);
    }

    let nonce = *packet_number;

    encoder.encode(&tag);

    // encode the credentials being used
    encoder.encode(crypto.credentials());
    encoder.encode(&source_control_port);
    encoder.encode(&source_stream_port);

    encoder.encode(&stream_id);

    encoder.encode(&packet_number);
    if stream_id.is_reliable {
        encoder.encode(&u24::default());
    }
    encoder.encode(&next_expected_control_packet);
    encoder.encode(&stream_offset);

    if let Some(final_offset) = final_offset {
        unsafe {
            assume!(encoder.remaining_capacity() >= 8);
            encoder.encode(&final_offset);
        }
    }

    if *control_data_len > 0 {
        unsafe {
            assume!(encoder.remaining_capacity() >= 8);
            encoder.encode(&control_data_len);
        }
    }

    let payload_len = {
        // TODO compute payload len for the given encoder
        let buffered_len = payload.buffered_len();

        let remaining_payload_capacity = encoder
            .remaining_capacity()
            .saturating_sub(header_len.encoding_size())
            .saturating_sub(*header_len as usize)
            .saturating_sub(*control_data_len as usize)
            .saturating_sub(crypto.tag_len());

        // TODO figure out encoding size for the capacity
        let remaining_payload_capacity = remaining_payload_capacity.saturating_sub(1);

        let payload_len = buffered_len.min(remaining_payload_capacity);

        unsafe {
            assume!(VarInt::try_from(payload_len).is_ok());
            VarInt::try_from(payload_len).unwrap()
        }
    };

    unsafe {
        assume!(encoder.remaining_capacity() >= 8);
        encoder.encode(&payload_len);
    }

    if !header.buffer_is_empty() {
        unsafe {
            assume!(encoder.remaining_capacity() >= 8);
            encoder.encode(&header_len);
        }
        encoder.write_sized(*header_len as usize, |mut dest| {
            header.infallible_copy_into(&mut dest);
        });
    }

    if *control_data_len > 0 {
        encoder.encode(control_data);
    }

    let payload_offset = encoder.len();

    let mut last_chunk = Default::default();
    encoder.write_sized(*payload_len as usize, |mut dest| {
        // the payload result is infallible
        last_chunk = payload.infallible_partial_copy_into(&mut dest);
    });

    let last_chunk = if last_chunk.is_empty() {
        None
    } else {
        Some(&*last_chunk)
    };

    encoder.advance_position(crypto.tag_len());

    let packet_len = encoder.len();

    let slice = encoder.as_mut_slice();

    {
        let (header, payload_and_tag) = unsafe {
            assume!(slice.len() >= payload_offset);
            slice.split_at_mut(payload_offset)
        };

        crypto.encrypt(nonce, header, last_chunk, payload_and_tag);
    }

    if cfg!(debug_assertions) {
        let decoder = s2n_codec::DecoderBufferMut::new(slice);
        let (packet, remaining) =
            super::decoder::Packet::decode(decoder, (), crypto.tag_len()).unwrap();
        assert!(remaining.is_empty());
        assert_eq!(packet.packet_number(), packet_number);
    }

    packet_len
}

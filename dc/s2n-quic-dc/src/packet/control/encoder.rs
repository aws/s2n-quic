// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    crypto::encrypt,
    packet::{
        control::{Tag, NONCE_MASK},
        stream, WireVersion,
    },
};
use s2n_codec::{Encoder, EncoderBuffer, EncoderValue};
use s2n_quic_core::{assume, buffer, varint::VarInt};

#[inline(always)]
pub fn encode<H, CD, C>(
    mut encoder: EncoderBuffer,
    source_control_port: u16,
    stream_id: Option<stream::Id>,
    packet_number: VarInt,
    header_len: VarInt,
    header: &mut H,
    control_data_len: VarInt,
    control_data: &CD,
    crypto: &C,
) -> usize
where
    H: buffer::reader::Storage<Error = core::convert::Infallible>,
    CD: EncoderValue,
    C: encrypt::Key,
{
    debug_assert_ne!(source_control_port, 0);

    let mut tag = Tag::default();
    tag.set_key_phase(crypto.key_phase());
    tag.set_is_stream(stream_id.is_some());
    tag.set_has_application_header(*header_len > 0);
    encoder.encode(&tag);

    // wire version - we only support `0` currently
    encoder.encode(&WireVersion::ZERO);

    let nonce = *packet_number | NONCE_MASK;

    // encode the credentials being used
    encoder.encode(crypto.credentials());
    encoder.encode(&source_control_port);

    encoder.encode(&stream_id);

    encoder.encode(&packet_number);

    unsafe {
        assume!(encoder.remaining_capacity() >= 8);
        encoder.encode(&control_data_len);
    }

    if !header.buffer_is_empty() {
        unsafe {
            assume!(encoder.remaining_capacity() >= 8);
            encoder.encode(&header_len);
        }
        encoder.write_sized(*header_len as usize, |mut dest| {
            let _: Result<(), core::convert::Infallible> = header.copy_into(&mut dest);
        });
    }

    encoder.encode(control_data);

    let payload_offset = encoder.len();

    encoder.advance_position(crypto.tag_len());

    let packet_len = encoder.len();

    let slice = encoder.as_mut_slice();

    {
        let (header, payload_and_tag) = unsafe {
            assume!(slice.len() >= payload_offset);
            slice.split_at_mut(payload_offset)
        };

        crypto.encrypt(nonce, header, None, payload_and_tag);
    }

    if cfg!(debug_assertions) {
        let decoder = s2n_codec::DecoderBufferMut::new(slice);
        let _ = super::decoder::Packet::decode(decoder, (), crypto.tag_len()).unwrap();
    }

    packet_len
}

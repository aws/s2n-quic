// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    credentials::Credentials,
    crypto::{self, KeyPhase},
    packet::{
        stream::{self, RelativeRetransmissionOffset, Tag},
        WireVersion,
    },
};
use s2n_codec::{Encoder, EncoderBuffer, EncoderValue};
use s2n_quic_core::{
    assume,
    buffer::{self, reader::storage::Infallible as _},
    varint::VarInt,
};

// TODO make sure this is accurate
pub const MAX_RETRANSMISSION_HEADER_LEN: usize = MAX_HEADER_LEN + (32 / 8);
pub const MAX_HEADER_LEN: usize = 64;

#[inline(always)]
pub fn encode<H, CD, P, C>(
    mut encoder: EncoderBuffer,
    source_queue_id: Option<VarInt>,
    stream_id: stream::Id,
    packet_number: VarInt,
    next_expected_control_packet: VarInt,
    header_len: VarInt,
    header: &mut H,
    control_data_len: VarInt,
    control_data: &CD,
    payload: &mut P,
    crypto: &C,
    credentials: &Credentials,
) -> usize
where
    H: buffer::reader::Storage<Error = core::convert::Infallible>,
    P: buffer::Reader<Error = core::convert::Infallible>,
    CD: EncoderValue,
    C: crypto::seal::Application,
{
    let packet_space = stream::PacketSpace::Stream;

    let payload_len = encode_header(
        &mut encoder,
        packet_space,
        crypto.key_phase(),
        credentials,
        source_queue_id,
        stream_id,
        packet_number,
        next_expected_control_packet,
        header_len,
        header,
        control_data_len,
        control_data,
        payload,
        crypto.tag_len(),
    );

    let nonce = packet_number.as_u64();

    let payload_offset = encoder.len();

    let mut last_chunk = Default::default();
    encoder.write_sized(payload_len, |mut dest| {
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
        assert_eq!(packet.payload().len(), payload_len);
        assert_eq!(packet.packet_number(), packet_number);
    }

    packet_len
}

#[inline(always)]
pub fn probe<H, CD, P, C>(
    mut encoder: EncoderBuffer,
    source_queue_id: Option<VarInt>,
    stream_id: stream::Id,
    packet_number: VarInt,
    next_expected_control_packet: VarInt,
    header_len: VarInt,
    header: &mut H,
    control_data_len: VarInt,
    control_data: &CD,
    payload: &mut P,
    crypto: &C,
    credentials: &Credentials,
) -> usize
where
    H: buffer::reader::Storage<Error = core::convert::Infallible>,
    P: buffer::Reader<Error = core::convert::Infallible>,
    CD: EncoderValue,
    C: crypto::seal::control::Stream,
{
    let packet_space = stream::PacketSpace::Recovery;

    let payload_len = encode_header(
        &mut encoder,
        packet_space,
        KeyPhase::Zero,
        credentials,
        source_queue_id,
        stream_id,
        packet_number,
        next_expected_control_packet,
        header_len,
        header,
        control_data_len,
        control_data,
        payload,
        crypto.tag_len(),
    );

    debug_assert_eq!(payload_len, 0, "probes should not contain data");

    let tag_offset = encoder.len();

    encoder.advance_position(crypto.tag_len());

    let packet_len = encoder.len();

    let slice = encoder.as_mut_slice();

    {
        let (header, tag) = unsafe {
            assume!(slice.len() >= tag_offset);
            slice.split_at_mut(tag_offset)
        };

        crypto.sign(header, tag);
    }

    if cfg!(debug_assertions) {
        let decoder = s2n_codec::DecoderBufferMut::new(slice);
        let (packet, remaining) =
            super::decoder::Packet::decode(decoder, (), crypto.tag_len()).unwrap();
        assert!(remaining.is_empty());
        assert_eq!(packet.payload().len(), payload_len);
        assert_eq!(packet.packet_number(), packet_number);
    }

    packet_len
}

#[inline(always)]
fn encode_header<H, CD, P>(
    encoder: &mut EncoderBuffer,
    packet_space: stream::PacketSpace,
    key_phase: KeyPhase,
    credentials: &Credentials,
    source_queue_id: Option<VarInt>,
    stream_id: stream::Id,
    packet_number: VarInt,
    next_expected_control_packet: VarInt,
    header_len: VarInt,
    header: &mut H,
    control_data_len: VarInt,
    control_data: &CD,
    payload: &mut P,
    tag_len: usize,
) -> usize
where
    H: buffer::reader::Storage<Error = core::convert::Infallible>,
    P: buffer::Reader<Error = core::convert::Infallible>,
    CD: EncoderValue,
{
    let stream_offset = payload.current_offset();
    let final_offset = payload.final_offset();

    let mut tag = Tag::default();
    tag.set_key_phase(key_phase);
    tag.set_has_control_data(*control_data_len > 0);
    tag.set_has_final_offset(final_offset.is_some());
    tag.set_has_application_header(*header_len > 0);
    tag.set_has_source_queue_id(source_queue_id.is_some());
    tag.set_packet_space(packet_space);
    encoder.encode(&tag);

    // encode the credentials being used
    encoder.encode(credentials);

    // wire version - we only support `0` currently
    encoder.encode(&WireVersion::ZERO);

    // unused space - was source_control_port when we did port migration but that has
    // been replaced with `source_queue_id`, which is more flexible
    encoder.encode(&0u16);

    encoder.encode(&stream_id);
    encoder.encode(&source_queue_id);

    encoder.encode(&packet_number);
    if stream_id.is_reliable {
        encoder.encode(&RelativeRetransmissionOffset::default());
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
            .saturating_sub(tag_len);

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

    *payload_len as usize
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    credentials::Credentials,
    crypto,
    packet::{
        control::{RoutingInfo, Tag},
        stream, WireVersion,
    },
};
use s2n_codec::{Encoder, EncoderBuffer, EncoderValue};
use s2n_quic_core::{assume, varint::VarInt};

/// Estimates the encoded length of a control packet
#[inline]
pub fn estimate_len(
    _packet_number: VarInt,
    source_queue_id: Option<VarInt>,
    binding_id: Option<stream::Id>,
    routing_info: RoutingInfo,
    control_data_len: VarInt,
    crypto_tag_len: usize,
) -> usize {
    let mut encoder = s2n_codec::EncoderLenEstimator::new(usize::MAX);

    // Tag
    let mut tag = Tag::default();
    tag.set_has_source_queue_id(source_queue_id.is_some());
    tag.set_is_stream(binding_id.is_some());
    tag.set_has_routing_info(!matches!(routing_info, RoutingInfo::None));
    encoder.encode(&tag);

    // Credentials
    {
        encoder.write_zerocopy::<crate::credentials::Id, _>(|_| {});
        encoder.write_repeated(8, 0);
    }

    // Wire version
    encoder.encode(&WireVersion::ZERO);

    // Optional binding_id
    encoder.encode(&binding_id);

    // Optional source_queue_id
    encoder.encode(&source_queue_id);

    // Packet number
    encoder.encode(&_packet_number);

    // Control data length
    encoder.encode(&control_data_len);

    // Routing info
    if tag.has_routing_info() {
        encoder.encode(&routing_info);
    }

    // Control data (actual bytes)
    encoder.write_repeated(control_data_len.as_u64() as usize, 0);

    // Crypto tag
    encoder.write_repeated(crypto_tag_len, 0);

    encoder.len()
}

#[inline(always)]
pub fn encode<CD, C>(
    mut encoder: EncoderBuffer,
    source_queue_id: Option<VarInt>,
    binding_id: Option<stream::Id>,
    packet_number: VarInt,
    routing_info: RoutingInfo,
    control_data_len: VarInt,
    control_data: &CD,
    crypto: &C,
    credentials: &Credentials,
) -> usize
where
    CD: EncoderValue,
    C: crypto::seal::control::Stream,
{
    let mut tag = Tag::default();
    tag.set_has_source_queue_id(source_queue_id.is_some());
    tag.set_is_stream(binding_id.is_some());
    tag.set_has_routing_info(!matches!(routing_info, RoutingInfo::None));
    encoder.encode(&tag);

    // encode the credentials being used
    encoder.encode(credentials);

    // wire version - we only support `0` currently
    encoder.encode(&WireVersion::ZERO);

    encoder.encode(&binding_id);
    encoder.encode(&source_queue_id);

    encoder.encode(&packet_number);

    unsafe {
        assume!(encoder.remaining_capacity() >= 8);
        encoder.encode(&control_data_len);
    }

    // Encode routing info if present
    if tag.has_routing_info() {
        encoder.encode(&routing_info);
    }

    encoder.encode(control_data);

    let payload_offset = encoder.len();

    encoder.advance_position(crypto.tag_len());

    let packet_len = encoder.len();

    let slice = encoder.as_mut_slice();

    {
        let (header, tag) = unsafe {
            assume!(slice.len() >= payload_offset);
            slice.split_at_mut(payload_offset)
        };

        crypto.sign(header, tag);
    }

    if cfg!(debug_assertions) {
        let decoder = s2n_codec::DecoderBufferMut::new(slice);
        let _ = super::decoder::Packet::decode(decoder, (), crypto.tag_len()).unwrap();
    }

    packet_len
}

/// Encodes a control packet using an Application sealer (for datagrams)
///
/// TODO: Unify this with the above `encode` function once we remove HMAC-based control packets
#[inline(always)]
pub fn encode_with_application<CD, C>(
    mut encoder: EncoderBuffer,
    source_queue_id: Option<VarInt>,
    binding_id: Option<stream::Id>,
    packet_number: VarInt,
    routing_info: RoutingInfo,
    control_data_len: VarInt,
    control_data: &mut CD,
    crypto: &C,
    credentials: &Credentials,
) -> usize
where
    CD: s2n_quic_core::buffer::reader::Storage<Error = core::convert::Infallible>,
    C: crypto::seal::Application,
{
    let mut tag = Tag::default();
    tag.set_has_source_queue_id(source_queue_id.is_some());
    tag.set_is_stream(binding_id.is_some());
    tag.set_has_routing_info(!matches!(routing_info, RoutingInfo::None));
    encoder.encode(&tag);

    // encode the credentials being used
    encoder.encode(credentials);

    // wire version - we only support `0` currently
    encoder.encode(&WireVersion::ZERO);

    encoder.encode(&binding_id);
    encoder.encode(&source_queue_id);

    encoder.encode(&packet_number);

    unsafe {
        assume!(encoder.remaining_capacity() >= 8);
        encoder.encode(&control_data_len);
    }

    // Encode routing info if present
    if tag.has_routing_info() {
        encoder.encode(&routing_info);
    }

    // Copy control data from reader (similar to how datagram encoder copies payload)
    let control_data_len_usize = control_data_len.as_u64() as usize;
    encoder.write_sized(control_data_len_usize, |mut dest| {
        let _: Result<(), core::convert::Infallible> = control_data.copy_into(&mut dest);
    });

    let payload_offset = encoder.len();

    encoder.advance_position(crypto.tag_len());

    let packet_len = encoder.len();

    let slice = encoder.as_mut_slice();

    {
        let (header, payload_and_tag) = unsafe {
            assume!(slice.len() >= payload_offset);
            slice.split_at_mut(payload_offset)
        };

        // For control packets, we use KeyPhase::Zero and treat it as a MAC-only operation
        // The "payload" is empty - all data is in the authenticated header
        crypto.encrypt(
            packet_number.as_u64(),
            header,
            None, // No extra payload
            payload_and_tag,
        );
    }

    if cfg!(debug_assertions) {
        let decoder = s2n_codec::DecoderBufferMut::new(slice);
        let _ = super::decoder::Packet::decode(decoder, (), crypto.tag_len()).unwrap();
    }

    packet_len
}

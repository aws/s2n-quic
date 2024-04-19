// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{credentials, crypto::encrypt, packet::datagram::Tag};
use s2n_codec::{Encoder, EncoderBuffer, EncoderValue};
use s2n_quic_core::{assume, buffer};

#[inline(always)]
pub fn estimate_len(
    _packet_number: super::PacketNumber,
    next_expected_control_packet: Option<super::PacketNumber>,
    app_header_len: super::HeaderLen,
    payload_len: super::PayloadLen,
    crypto_tag_len: usize,
) -> usize {
    let app_header_len_usize = *app_header_len as usize;
    let payload_len_usize = *payload_len as usize;

    let mut encoder = s2n_codec::EncoderLenEstimator::new(usize::MAX);

    encoder.encode(&Tag::default());
    // credentials
    {
        encoder.write_zerocopy::<credentials::Id, _>(|_| {});
        encoder.write_repeated(8, 0);
    }
    encoder.encode(&0u16); // source control port
    encoder.write_repeated(8, 0); // packet number
    encoder.write_repeated(8, 0); // payload len

    if let Some(_packet_number) = next_expected_control_packet {
        encoder.write_repeated(8, 0); // next expected control packet
        encoder.write_repeated(8, 0); // control_data_len
    }

    if app_header_len_usize > 0 {
        encoder.write_repeated(8, 0); // application header len
        encoder.write_repeated(app_header_len_usize, 0); // application data
    }

    encoder.write_repeated(8, 0);
    encoder.write_repeated(payload_len_usize, 0);

    encoder.write_repeated(crypto_tag_len, 0);

    encoder.len()
}

#[inline(always)]
#[allow(clippy::too_many_arguments)]
pub fn encode<H, CD, P, C>(
    mut encoder: EncoderBuffer,
    tag: Tag,
    source_control_port: u16,
    packet_number: super::PacketNumber,
    next_expected_control_packet: Option<super::PacketNumber>,
    header_len: super::HeaderLen,
    header: &mut H,
    control_data: &CD,
    payload_len: super::PayloadLen,
    payload: &mut P,
    crypto: &C,
) -> usize
where
    H: buffer::reader::Storage<Error = core::convert::Infallible>,
    P: buffer::reader::Storage<Error = core::convert::Infallible>,
    CD: EncoderValue,
    C: encrypt::Key,
{
    debug_assert_eq!(tag.ack_eliciting(), next_expected_control_packet.is_some());

    let header_len_usize = *header_len as usize;
    let payload_len_usize = *payload_len as usize;
    let nonce = *packet_number;

    encoder.encode(&tag);

    // encode the credentials being used
    encoder.encode(crypto.credentials());
    encoder.encode(&source_control_port);

    if tag.is_connected() || tag.ack_eliciting() {
        unsafe {
            assume!(encoder.remaining_capacity() >= 8);
            encoder.encode(&packet_number);
        }
    } else {
        debug_assert_eq!(packet_number, super::PacketNumber::default());
    }

    if tag.has_length() {
        unsafe {
            assume!(encoder.remaining_capacity() >= 8);
            encoder.encode(&payload_len);
        }
    }

    if let Some(packet_number) = next_expected_control_packet {
        unsafe {
            assume!(encoder.remaining_capacity() >= 8);
            encoder.encode(&packet_number);
        }
        // TODO write control data len
    }

    if !header.buffer_is_empty() {
        unsafe {
            assume!(encoder.remaining_capacity() >= 8);
            encoder.encode(&header_len);
        }
        encoder.write_sized(header_len_usize, |mut dest| {
            let _: Result<(), core::convert::Infallible> = header.copy_into(&mut dest);
        });
    }

    if next_expected_control_packet.is_some() {
        encoder.encode(control_data);
    }

    let payload_offset = encoder.len();

    let mut last_chunk = buffer::reader::storage::Chunk::empty();
    encoder.write_sized(payload_len_usize, |mut dest| {
        let result: Result<buffer::reader::storage::Chunk, core::convert::Infallible> =
            payload.partial_copy_into(&mut dest);
        last_chunk = result.expect("copy is infallible");
    });

    let last_chunk = if last_chunk.is_empty() {
        None
    } else {
        Some(&last_chunk[..])
    };

    encoder.advance_position(crypto.tag_len());

    let packet_len = encoder.len();

    let slice = encoder.as_mut_slice();
    let (header, payload_and_tag) = unsafe {
        assume!(slice.len() >= payload_offset);
        slice.split_at_mut(payload_offset)
    };

    crypto.encrypt(nonce, header, last_chunk, payload_and_tag);

    packet_len
}

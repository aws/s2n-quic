// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::crypto::seal;
use s2n_codec::{Encoder, EncoderBuffer};
use s2n_quic_core::assume;

#[inline]
pub fn finish<C>(mut encoder: EncoderBuffer, crypto: &C) -> usize
where
    C: seal::control::Secret,
{
    let header_offset = encoder.len();

    encoder.advance_position(crypto.tag_len());

    let packet_len = encoder.len();

    let slice = encoder.as_mut_slice();
    let (header, tag) = unsafe {
        assume!(slice.len() >= header_offset);
        slice.split_at_mut(header_offset)
    };

    crypto.sign(header, tag);

    packet_len
}

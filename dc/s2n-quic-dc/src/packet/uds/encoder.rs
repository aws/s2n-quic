// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::path::secret::schedule::Ciphersuite;
use s2n_codec::{Encoder, EncoderBuffer, EncoderLenEstimator};
use s2n_quic_core::{dc::ApplicationParams, varint::VarInt};

pub const PACKET_VERSION: u8 = 0;
pub const APP_PARAMS_VERSION: u8 = 0;

#[inline(always)]
pub fn encode(
    mut encoder: EncoderBuffer,
    ciphersuite: &Ciphersuite,
    export_secret: &[u8],
    application_params: &ApplicationParams,
    payload: &[u8],
) -> usize {
    let start_len = encoder.len();

    encoder.encode(&PACKET_VERSION);

    let ciphersuite_byte: u8 = (*ciphersuite).into();
    encoder.encode(&ciphersuite_byte);

    encoder.encode_with_len_prefix::<VarInt, _>(&export_secret);

    encoder.encode(&APP_PARAMS_VERSION);

    encoder.encode(application_params);

    encoder.encode_with_len_prefix::<VarInt, _>(&payload);

    encoder.len() - start_len
}

#[inline]
pub fn encoding_size(
    ciphersuite: &Ciphersuite,
    export_secret: &[u8],
    application_params: &ApplicationParams,
    payload: &[u8],
) -> usize {
    let mut estimator = EncoderLenEstimator::new(usize::MAX);

    estimator.encode(&PACKET_VERSION);

    let ciphersuite_byte: u8 = (*ciphersuite).into();
    estimator.encode(&ciphersuite_byte);

    estimator.encode_with_len_prefix::<VarInt, _>(&export_secret);

    estimator.encode(&APP_PARAMS_VERSION);

    estimator.encode(application_params);

    estimator.encode_with_len_prefix::<VarInt, _>(&payload);

    estimator.len()
}

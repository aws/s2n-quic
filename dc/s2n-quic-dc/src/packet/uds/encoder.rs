// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{encode_address, Protocol};
use crate::path::secret::schedule::Ciphersuite;
use s2n_codec::Encoder;
use s2n_quic_core::{dc::ApplicationParams, inet::SocketAddress, varint::VarInt};

pub const PACKET_VERSION: u8 = 0;
pub const PACKET_VERSION_V1: u8 = 1;
pub const PACKET_VERSION_HANDOFF: u8 = 2;
/// A UDP stream or control packet without stream-creation material.
pub const PACKET_VERSION_FORWARD: u8 = 3;
pub const APP_PARAMS_VERSION: u8 = 0;

#[inline(always)]
pub fn encode<E: Encoder>(
    encoder: &mut E,
    ciphersuite: &Ciphersuite,
    export_secret: &[u8],
    application_params: &ApplicationParams,
    encode_time: u64,
    application_data: Option<&[u8]>,
    payload: &[u8],
) -> usize {
    let start_len = encoder.len();

    let version_tag = if application_data.is_some() {
        PACKET_VERSION_V1
    } else {
        PACKET_VERSION
    };
    encoder.encode(&version_tag);

    let ciphersuite_byte: u8 = (*ciphersuite).into();
    encoder.encode(&ciphersuite_byte);

    encoder.encode_with_len_prefix::<VarInt, _>(&export_secret);

    encoder.encode(&APP_PARAMS_VERSION);

    encoder.encode(application_params);

    encoder.encode(&encode_time);

    if let Some(application_data) = application_data {
        encoder.encode_with_len_prefix::<VarInt, _>(&application_data);
    }

    encoder.encode_with_len_prefix::<VarInt, _>(&payload);

    encoder.len() - start_len
}

/// Encodes a credential handoff with transport and address metadata.
///
/// The application-data field is present even when empty. `local_address` records the initial
/// packet's destination so the receiver can reply from the same IP.
#[inline(always)]
#[allow(clippy::too_many_arguments)]
pub fn encode_handoff<E: Encoder>(
    encoder: &mut E,
    protocol: Protocol,
    ciphersuite: &Ciphersuite,
    export_secret: &[u8],
    application_params: &ApplicationParams,
    encode_time: u64,
    peer_address: &SocketAddress,
    local_address: Option<&SocketAddress>,
    application_data: Option<&[u8]>,
    payload: &[u8],
) -> usize {
    let start_len = encoder.len();

    encoder.encode(&PACKET_VERSION_HANDOFF);

    let ciphersuite_byte: u8 = (*ciphersuite).into();
    encoder.encode(&ciphersuite_byte);

    encoder.encode_with_len_prefix::<VarInt, _>(&export_secret);

    encoder.encode(&APP_PARAMS_VERSION);

    encoder.encode(application_params);

    encoder.encode(&encode_time);

    encoder.encode_with_len_prefix::<VarInt, _>(&application_data.unwrap_or_default());

    encoder.encode_with_len_prefix::<VarInt, _>(&payload);

    let protocol_byte: u8 = protocol.into();
    encoder.encode(&protocol_byte);

    encode_address(encoder, Some(peer_address));
    encode_address(encoder, local_address);

    encoder.len() - start_len
}

/// Encodes a UDP stream or control packet without a credential handoff.
#[inline(always)]
pub fn encode_forward<E: Encoder>(
    encoder: &mut E,
    encode_time: u64,
    peer_address: &SocketAddress,
    payload: &[u8],
) -> usize {
    let start_len = encoder.len();

    encoder.encode(&PACKET_VERSION_FORWARD);
    encoder.encode(&encode_time);
    encode_address(encoder, Some(peer_address));
    encoder.encode_with_len_prefix::<VarInt, _>(&payload);

    encoder.len() - start_len
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::path::secret::schedule::Ciphersuite;
use s2n_codec::{Encoder, EncoderBuffer, EncoderValue};
use s2n_quic_core::{assume, dc::ApplicationParams, varint::VarInt};
use std::sync::atomic::Ordering;

pub const PACKET_VERSION: u8 = 0;
pub const APP_PARAMS_VERSION: u8 = 0;

/// Encode a packet with the format:
///
/// [ version tag: u8 ]
/// [ ciphersuite: u8 ]
/// [ export secret length: VarInt ]
/// [ export secret ] - bytes
/// [ application parameters version: u8 ]
/// [ serialized application parameters ]
/// [ wire packet length: VarInt ]
/// [ wire packet ] - bytes
///
#[inline(always)]
pub fn encode(
    mut encoder: EncoderBuffer,
    ciphersuite: &Ciphersuite,
    export_secret: &[u8],
    export_secret_len: VarInt,
    application_params: &ApplicationParams,
    payload: &[u8],
) -> usize {
    let start_len = encoder.len();

    encoder.encode(&PACKET_VERSION);

    let ciphersuite_byte: u8 = (*ciphersuite).into();
    encoder.encode(&ciphersuite_byte);

    encoder.encode(&export_secret_len);

    for &byte in export_secret {
        encoder.encode(&byte);
    }

    encoder.encode(&APP_PARAMS_VERSION);

    encode_application_params(application_params, &mut encoder);

    let payload_len = {
        let payload_len = payload.len();
        unsafe {
            assume!(VarInt::try_from(payload_len).is_ok());
            VarInt::try_from(payload_len).unwrap()
        }
    };
    encoder.encode(&payload_len);

    for &byte in payload {
        encoder.encode(&byte);
    }

    encoder.len() - start_len
}

#[inline]
pub fn encoding_size(
    export_secret: &[u8],
    export_secret_len: VarInt,
    application_params: &ApplicationParams,
    payload: &[u8],
) -> usize {
    let mut size = 0;

    // Version tag: u8 = 1 byte
    size += 1;

    // Ciphersuite: u8 = 1 byte
    size += 1;

    size += export_secret_len.encoding_size();

    size += export_secret.len();

    // Application parameters version: u8 = 1 byte
    size += 1;

    size += application_params_encoding_size(application_params);

    let payload_len = {
        let payload_len = payload.len();
        unsafe {
            assume!(VarInt::try_from(payload_len).is_ok());
            VarInt::try_from(payload_len).unwrap()
        }
    };
    size += payload_len.encoding_size();

    size += payload.len();

    size
}

pub fn encode_application_params(
    application_params: &ApplicationParams,
    encoder: &mut EncoderBuffer,
) {
    let max_datagram_size = application_params.max_datagram_size.load(Ordering::Relaxed);
    encoder.encode(&max_datagram_size);

    encoder.encode(&application_params.remote_max_data);

    encoder.encode(&application_params.local_send_max_data);

    encoder.encode(&application_params.local_recv_max_data);

    match application_params.max_idle_timeout {
        Some(timeout) => {
            encoder.encode(&1u8); // presence flag
            encoder.encode(&timeout.get()); // u32 value
        }
        None => {
            encoder.encode(&0u8); // absence flag
        }
    }
}

pub fn application_params_encoding_size(application_params: &ApplicationParams) -> usize {
    let mut size = 0;

    // max_datagram_size: u16 = 2 bytes
    size += 2;

    size += application_params.remote_max_data.encoding_size();
    size += application_params.local_send_max_data.encoding_size();
    size += application_params.local_recv_max_data.encoding_size();

    // max_idle_timeout: 1 byte flag + optional 4 bytes
    size += 1; // presence flag
    if application_params.max_idle_timeout.is_some() {
        size += 4; // u32
    }

    size
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#![forbid(unsafe_code)]
#![cfg_attr(not(test), no_std)]

mod ciphersuite;
#[macro_use]
mod negotiated;
#[macro_use]
mod header_key;

pub use ring::{
    self,
    aead::{Algorithm, MAX_TAG_LEN},
    hkdf::Prk,
};

#[derive(Clone)]
pub struct SecretPair {
    pub server: Prk,
    pub client: Prk,
}

pub mod handshake;
pub mod initial;
pub mod one_rtt;
pub mod retry;
pub mod zero_rtt;

#[derive(Clone, Copy, Debug, Default)]
pub struct RingCryptoSuite;

impl s2n_quic_core::crypto::CryptoSuite for RingCryptoSuite {
    type HandshakeKey = handshake::RingHandshakeKey;
    type HandshakeHeaderKey = handshake::RingHandshakeHeaderKey;
    type InitialKey = initial::RingInitialKey;
    type InitialHeaderKey = initial::RingInitialHeaderKey;
    type OneRttKey = one_rtt::RingOneRttKey;
    type OneRttHeaderKey = one_rtt::RingOneRttHeaderKey;
    type ZeroRttKey = zero_rtt::RingZeroRttKey;
    type ZeroRttHeaderKey = zero_rtt::RingZeroRttHeaderKey;
    type RetryKey = retry::RingRetryKey;
}

#[cfg(test)]
mod tests {
    use crate::one_rtt::RingOneRttKey;
    use ring::{aead, hkdf};
    use s2n_codec::DecoderBufferMut;
    use s2n_quic_core::{
        connection::id::ConnectionInfo,
        endpoint,
        inet::SocketAddress,
        packet::{number::PacketNumberSpace, short::ProtectedShort},
        varint::VarInt,
    };
    use std::num::ParseIntError;

    fn decode_hex(s: &str) -> Vec<u8> {
        let data: Result<Vec<u8>, ParseIntError> = (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16))
            .collect();
        data.unwrap()
    }

    #[test]
    // Test packet_header unprotect against real traffic sample.
    fn packet_header_unprotect() {
        let client_secret =
            decode_hex("2b3b2f344cecc66f59ad61009f9937d886cb87368ab74f8e2d805595f72481a3");
        let server_secret =
            decode_hex("d7710a1cf3efea3625c57b08e08afb83d7201ea3423487cd8b7f7b76bc7e3d89");

        let aead_algo = &aead::AES_128_GCM;
        let pair = crate::SecretPair {
            server: hkdf::Prk::new_less_safe(hkdf::HKDF_SHA256, &server_secret),
            client: hkdf::Prk::new_less_safe(hkdf::HKDF_SHA256, &client_secret),
        };
        let (_key, header_key) =
            RingOneRttKey::new(endpoint::Type::Client, aead_algo, pair).expect("invalid cipher");

        let data = vec![
            (0, "5fc7fb38fb1fbe14179d36d64ee94088b443d2d71c2edeb8b09be30dc1b3ceb10a82c07b02863665004cc2","c7fb38fb1fbe14179d36d64ee94088b4"),
            (1, "5dc7fb38fb1fbe14179d36d64ee94088b47e9c97be63edacdace07ef6c37f497fe7c2cc9a1a5c04429144e", "c7fb38fb1fbe14179d36d64ee94088b4"),
            (2, "55c6fe4188993d55de984803d2e1a4f1e8d3e9b89fe71e825929e0f06ebc710cda38033de4ab7ae197a40a0560", "c6fe4188993d55de984803d2e1a4f1e8"),
            (3, "57c6fe4188993d55de984803d2e1a4f1e875cdb14dbda56048fca37f287d65ad85391744cfc250bd9285044949c9bf647060eb64", "c6fe4188993d55de984803d2e1a4f1e8"),
            (4, "40c6fe4188993d55de984803d2e1a4f1e8c7d8ce79237858cbe533219fda2f87c650bfe064068dbaeb46646fed", "c6fe4188993d55de984803d2e1a4f1e8"),
            (5, "4cc6fe4188993d55de984803d2e1a4f1e8aba0db5eb2ca5ae13f3c49aa8fbb2733afc1cdc58fe3dde302395718", "c6fe4188993d55de984803d2e1a4f1e8"),
            (6, "50c6fe4188993d55de984803d2e1a4f1e8d82df33d6d499d5871a48d8befe362074ab80a68e40721688ae2f517", "c6fe4188993d55de984803d2e1a4f1e8")
        ];

        let remote_address = SocketAddress::default();
        let connection_info = ConnectionInfo::new(&remote_address);

        let cid_len = 16;
        for (expected_pn, data, expected_cid) in data {
            let mut data = decode_hex(data);
            let decoder_buffer = DecoderBufferMut::new(&mut data);
            let (encoded_packet, _remaining) =
                ProtectedShort::decode(0, decoder_buffer, &connection_info, &cid_len).unwrap();

            let encrypted_packet = encoded_packet
                .unprotect(
                    &header_key,
                    PacketNumberSpace::ApplicationData.new_packet_number(VarInt::from_u8(0)),
                )
                .unwrap();

            assert_eq!(
                encrypted_packet.destination_connection_id(),
                decode_hex(expected_cid),
                "destination_connection_id doesnt match expected value {}",
                expected_pn
            );

            assert_eq!(
                encrypted_packet.packet_number,
                PacketNumberSpace::ApplicationData
                    .new_packet_number(VarInt::new(expected_pn as u64).unwrap()),
                "packet_number doesnt match {}",
                expected_pn
            );
        }
    }
}

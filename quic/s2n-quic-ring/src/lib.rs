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
        crypto::testing::HeaderKey as TestHeaderKey,
        endpoint,
        inet::SocketAddress,
        packet::{number::PacketNumberSpace, short::ProtectedShort},
        varint::VarInt,
    };
    use std::{net::SocketAddr, num::ParseIntError};

    fn decode_hex(s: &str) -> Vec<u8> {
        let data: Result<Vec<u8>, ParseIntError> = (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16))
            .collect();
        data.unwrap()
    }

    #[test]
    fn unprotect_packet() {
        // CLIENT_HANDSHAKE_TRAFFIC_SECRET 62d3809fa7c0173384dcc795b62588616cd714f581144ba44f7097b282d125da
        // SERVER_HANDSHAKE_TRAFFIC_SECRET 920427784d609939c1e6b46cab13afc185da1b80788d6637c2430609fd9f3fcc
        // CLIENT_TRAFFIC_SECRET_0 2b3b2f344cecc66f59ad61009f9937d886cb87368ab74f8e2d805595f72481a3
        // SERVER_TRAFFIC_SECRET_0 d7710a1cf3efea3625c57b08e08afb83d7201ea3423487cd8b7f7b76bc7e3d89
        // EXPORTER_SECRET 9e57c02d66dece871572fc69cb874008b47e8da5c0c9669057e7d56b8710d162
        // handshake keys
        // let client_secret =
        //     decode_hex("62d3809fa7c0173384dcc795b62588616cd714f581144ba44f7097b282d125da").unwrap();
        // let server_secret =
        //     decode_hex("920427784d609939c1e6b46cab13afc185da1b80788d6637c2430609fd9f3fcc").unwrap();
        // traffic keys
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
            RingOneRttKey::new(endpoint::Type::Client, aead_algo, pair.clone())
                .expect("invalid cipher");

        // let (_key, server_header_key) =
        //     RingOneRttKey::new(endpoint::Type::Server, aead_algo, pair).expect("invalid cipher");

        // 0
        let data = "5fc7fb38fb1fbe14179d36d64ee94088b443d2d71c2edeb8b09be30dc1b3ceb10a82c07b02863665004cc2";
        // dcid c7fb38fb1fbe14179d36d64ee94088b4
        // 1
        let data = "57c6fe4188993d55de984803d2e1a4f1e875cdb14dbda56048fca37f287d65ad85391744cfc250bd928504";
        // 2
        // let data = "55c6fe4188993d55de984803d2e1a4f1e8d3e9b89fe71e825929e0f06ebc710cda38033de4ab7ae197a40a0560";
        // 3
        // let data = "57c6fe4188993d55de984803d2e1a4f1e875cdb14dbda56048fca37f287d65ad85391744cfc250bd9285044949c9bf647060eb64";
        // 4
        // let data = "40c6fe4188993d55de984803d2e1a4f1e8c7d8ce79237858cbe533219fda2f87c650bfe064068dbaeb46646fed";
        // 5
        // let data = "4cc6fe4188993d55de984803d2e1a4f1e8aba0db5eb2ca5ae13f3c49aa8fbb2733afc1cdc58fe3dde302395718";
        // 6
        // let data = "50c6fe4188993d55de984803d2e1a4f1e8d82df33d6d499d5871a48d8befe362074ab80a68e40721688ae2f517";

        let remote_address = SocketAddress::default();
        let connection_info = ConnectionInfo::new(&remote_address);

        // for i in 1..=20 {
        let i = 16;
        let mut data = decode_hex(data);
        let decoder_buffer = DecoderBufferMut::new(&mut data);
        let (encoded_packet, _remaining) =
            ProtectedShort::decode(0, decoder_buffer, &connection_info, &i).unwrap();

        let encrypted_packet = encoded_packet
            .unprotect(
                &header_key,
                PacketNumberSpace::ApplicationData.new_packet_number(VarInt::from_u8(0)),
            )
            .unwrap();

        let dcid_matched = encrypted_packet.destination_connection_id()
            == decode_hex("c6fe4188993d55de984803d2e1a4f1e8");
        println!(
            "{} {} | {:?}: {:x?}",
            i,
            dcid_matched,
            encrypted_packet.packet_number,
            encrypted_packet.destination_connection_id(),
        );

        assert_eq!(
            encrypted_packet.destination_connection_id(),
            &decode_hex("c6fe4188993d55de984803d2e1a4f1e8"),
            "DESTINATION_CONNECTION_ID DOESNT MATCH EXPECTED VALUE"
        );
        //
        // eventually try deryption
        let p = encrypted_packet.decrypt(&_key).unwrap();
        println!(
            "{:?}: {:x?}",
            p.packet_number,
            p.destination_connection_id()
        );
        // }

        assert!(false);
    }
}

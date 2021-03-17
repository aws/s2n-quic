// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{ciphersuite::TLS_AES_128_GCM_SHA256 as Ciphersuite, header_key::HeaderKeyPair};
use ring::hkdf;
use s2n_quic_core::{
    crypto::{
        label::{CLIENT_IN, SERVER_IN},
        CryptoError, InitialHeaderKey, InitialKey, Key, INITIAL_SALT,
    },
    endpoint,
};

header_key!(RingInitialHeaderKey);

impl InitialHeaderKey for RingInitialHeaderKey {}

#[derive(Debug)]
pub struct RingInitialKey {
    sealer: Ciphersuite,
    opener: Ciphersuite,
}

lazy_static::lazy_static! {
    /// Compute the Initial salt once, as the seed is constant
    static ref INITIAL_SIGNING_KEY: hkdf::Salt = hkdf::Salt::new(hkdf::HKDF_SHA256, &INITIAL_SALT);
}

impl RingInitialKey {
    fn new(endpoint: endpoint::Type, connection_id: &[u8]) -> (Self, RingInitialHeaderKey) {
        let initial_secret = INITIAL_SIGNING_KEY.extract(connection_id);
        let digest = INITIAL_SIGNING_KEY.algorithm();

        let client_secret = initial_secret
            .expand(&[&CLIENT_IN], digest)
            .expect("label size verified")
            .into();

        let server_secret = initial_secret
            .expand(&[&SERVER_IN], digest)
            .expect("label size verified")
            .into();

        let (sealer, opener) = match endpoint {
            endpoint::Type::Client => (
                Ciphersuite::new(client_secret),
                Ciphersuite::new(server_secret),
            ),
            endpoint::Type::Server => (
                Ciphersuite::new(server_secret),
                Ciphersuite::new(client_secret),
            ),
        };

        let (key_sealer, header_sealer) = sealer;
        let (key_opener, header_opener) = opener;
        let key = Self {
            sealer: key_sealer,
            opener: key_opener,
        };
        let header_key = RingInitialHeaderKey(HeaderKeyPair {
            sealer: header_sealer,
            opener: header_opener,
        });

        (key, header_key)
    }
}

impl InitialKey for RingInitialKey {
    type HeaderKey = RingInitialHeaderKey;

    fn new_server(connection_id: &[u8]) -> (Self, Self::HeaderKey) {
        Self::new(endpoint::Type::Server, connection_id)
    }

    fn new_client(connection_id: &[u8]) -> (Self, Self::HeaderKey) {
        Self::new(endpoint::Type::Client, connection_id)
    }
}

impl Key for RingInitialKey {
    fn decrypt(
        &self,
        packet_number: u64,
        header: &[u8],
        payload: &mut [u8],
    ) -> Result<(), CryptoError> {
        self.opener.decrypt(packet_number, header, payload)
    }

    fn encrypt(
        &self,
        packet_number: u64,
        header: &[u8],
        payload: &mut [u8],
    ) -> Result<(), CryptoError> {
        self.sealer.encrypt(packet_number, header, payload)
    }

    fn tag_len(&self) -> usize {
        self.sealer.tag_len()
    }

    fn aead_confidentiality_limit(&self) -> u64 {
        self.sealer.aead_confidentiality_limit()
    }

    fn aead_integrity_limit(&self) -> u64 {
        self.opener.aead_integrity_limit()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use s2n_codec::{DecoderBufferMut, EncoderBuffer};
    use s2n_quic_core::{
        connection::id::ConnectionInfo,
        crypto::initial::{
            EXAMPLE_CLIENT_INITIAL_PAYLOAD, EXAMPLE_CLIENT_INITIAL_PROTECTED_PACKET, EXAMPLE_DCID,
            EXAMPLE_SERVER_INITIAL_PAYLOAD, EXAMPLE_SERVER_INITIAL_PROTECTED_PACKET,
        },
        inet::SocketAddress,
        packet::{encoding::PacketEncoder, initial::CleartextInitial, ProtectedPacket},
    };

    #[test]
    fn rfc_example_server_test() {
        test_round_trip(
            &RingInitialKey::new_client(&EXAMPLE_DCID),
            &RingInitialKey::new_server(&EXAMPLE_DCID),
            &EXAMPLE_CLIENT_INITIAL_PROTECTED_PACKET,
            &EXAMPLE_CLIENT_INITIAL_PAYLOAD,
        );
    }

    #[test]
    fn rfc_example_client_test() {
        test_round_trip(
            &RingInitialKey::new_server(&EXAMPLE_DCID),
            &RingInitialKey::new_client(&EXAMPLE_DCID),
            &EXAMPLE_SERVER_INITIAL_PROTECTED_PACKET,
            &EXAMPLE_SERVER_INITIAL_PAYLOAD,
        );
    }

    fn test_round_trip(
        sealer: &(RingInitialKey, RingInitialHeaderKey),
        opener: &(RingInitialKey, RingInitialHeaderKey),
        protected_packet: &[u8],
        cleartext_payload: &[u8],
    ) {
        let (sealer_key, sealer_header_key) = sealer;
        let (opener_key, opener_header_key) = opener;
        let (version, dcid, scid, token, sealed_packet) = decrypt(
            opener_key,
            opener_header_key,
            protected_packet.to_vec(),
            cleartext_payload,
            |packet| {
                let version = packet.version;
                let dcid = packet.destination_connection_id.to_vec();
                let scid = packet.source_connection_id.to_vec();
                let token = packet.token.to_vec();

                let mut output_buffer = vec![0; protected_packet.len()];
                packet
                    .encode_packet(
                        sealer_key,
                        sealer_header_key,
                        Default::default(),
                        None,
                        EncoderBuffer::new(&mut output_buffer),
                    )
                    .unwrap();

                (version, dcid, scid, token, output_buffer)
            },
        );

        // We have to decrypt instead of assert_eq on the sealed_packet
        // because of potential encoding differences. But the resulting
        // packets should be equal
        decrypt(
            opener_key,
            opener_header_key,
            sealed_packet,
            cleartext_payload,
            |packet| {
                assert_eq!(packet.version, version);
                assert_eq!(packet.destination_connection_id, &dcid[..]);
                assert_eq!(packet.source_connection_id, &scid[..]);
                assert_eq!(packet.token, &token[..]);
            },
        );
    }

    fn decrypt<F: FnOnce(CleartextInitial) -> O, O>(
        opener_key: &RingInitialKey,
        opener_header_key: &RingInitialHeaderKey,
        mut protected_packet: Vec<u8>,
        cleartext_payload: &[u8],
        on_decrypt: F,
    ) -> O {
        let decoder = DecoderBufferMut::new(&mut protected_packet);
        let remote_address = SocketAddress::default();
        let connection_info = ConnectionInfo::new(&remote_address);
        let (packet, _) = ProtectedPacket::decode(decoder, &connection_info, &20).unwrap();

        let packet = match packet {
            ProtectedPacket::Initial(initial) => initial,
            _ => panic!("expected initial packet type"),
        };

        let packet = packet
            .unprotect(opener_header_key, Default::default())
            .unwrap();
        let packet = packet.decrypt(opener_key).unwrap();

        // trim any padding off of the end
        let actual_payload = &packet.payload.as_less_safe_slice()[..cleartext_payload.len()];
        assert_eq!(cleartext_payload, actual_payload);

        on_decrypt(packet)
    }
}

use crate::ciphersuite::TLS_AES_128_GCM_SHA256 as Ciphersuite;
use ring::hkdf;
use s2n_quic_core::{
    crypto::{
        label::{CLIENT_IN, SERVER_IN},
        CryptoError, HeaderCrypto, HeaderProtectionMask, InitialCrypto, Key, INITIAL_SALT,
    },
    endpoint::EndpointType,
};

#[derive(Debug)]
pub struct RingInitialCrypto {
    sealer: Ciphersuite,
    opener: Ciphersuite,
}

lazy_static::lazy_static! {
    /// Compute the Initial salt once, as the seed is constant
    static ref INITIAL_SIGNING_KEY: hkdf::Salt = hkdf::Salt::new(hkdf::HKDF_SHA256, &INITIAL_SALT);
}

impl RingInitialCrypto {
    fn new(endpoint: EndpointType, connection_id: &[u8]) -> Self {
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
            EndpointType::Client => (
                Ciphersuite::new(client_secret),
                Ciphersuite::new(server_secret),
            ),
            EndpointType::Server => (
                Ciphersuite::new(server_secret),
                Ciphersuite::new(client_secret),
            ),
        };

        Self { sealer, opener }
    }
}

impl InitialCrypto for RingInitialCrypto {
    fn new_server(connection_id: &[u8]) -> Self {
        Self::new(EndpointType::Server, connection_id)
    }

    fn new_client(connection_id: &[u8]) -> Self {
        Self::new(EndpointType::Client, connection_id)
    }
}

impl Key for RingInitialCrypto {
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
}

impl HeaderCrypto for RingInitialCrypto {
    fn opening_header_protection_mask(&self, sample: &[u8]) -> HeaderProtectionMask {
        self.opener.opening_header_protection_mask(sample)
    }

    fn opening_sample_len(&self) -> usize {
        self.opener.opening_sample_len()
    }

    fn sealing_header_protection_mask(&self, sample: &[u8]) -> HeaderProtectionMask {
        self.sealer.sealing_header_protection_mask(sample)
    }

    fn sealing_sample_len(&self) -> usize {
        self.sealer.sealing_sample_len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use s2n_codec::{DecoderBufferMut, EncoderBuffer};
    use s2n_quic_core::{
        crypto::initial::{
            EXAMPLE_CLIENT_INITIAL_PAYLOAD, EXAMPLE_CLIENT_INITIAL_PROTECTED_PACKET, EXAMPLE_DCID,
            EXAMPLE_SERVER_INITIAL_PAYLOAD, EXAMPLE_SERVER_INITIAL_PROTECTED_PACKET,
        },
        packet::{encoding::PacketEncoder, initial::CleartextInitial, ProtectedPacket},
    };

    #[test]
    fn rfc_example_server_test() {
        test_round_trip(
            &RingInitialCrypto::new_client(&EXAMPLE_DCID),
            &RingInitialCrypto::new_server(&EXAMPLE_DCID),
            &EXAMPLE_CLIENT_INITIAL_PROTECTED_PACKET,
            &EXAMPLE_CLIENT_INITIAL_PAYLOAD,
        );
    }

    #[test]
    fn rfc_example_client_test() {
        test_round_trip(
            &RingInitialCrypto::new_server(&EXAMPLE_DCID),
            &RingInitialCrypto::new_client(&EXAMPLE_DCID),
            &EXAMPLE_SERVER_INITIAL_PROTECTED_PACKET,
            &EXAMPLE_SERVER_INITIAL_PAYLOAD,
        );
    }

    fn test_round_trip(
        sealer: &RingInitialCrypto,
        opener: &RingInitialCrypto,
        protected_packet: &[u8],
        cleartext_payload: &[u8],
    ) {
        let (version, dcid, scid, token, sealed_packet) = decrypt(
            opener,
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
                        sealer,
                        Default::default(),
                        EncoderBuffer::new(&mut output_buffer),
                    )
                    .unwrap();

                (version, dcid, scid, token, output_buffer)
            },
        );

        // We have to decrypt instead of assert_eq on the sealed_packet
        // because of potential encoding differences. But the resulting
        // packets should be equal
        decrypt(opener, sealed_packet, cleartext_payload, |packet| {
            assert_eq!(packet.version, version);
            assert_eq!(packet.destination_connection_id, &dcid[..]);
            assert_eq!(packet.source_connection_id, &scid[..]);
            assert_eq!(packet.token, &token[..]);
        });
    }

    fn decrypt<F: FnOnce(CleartextInitial) -> O, O>(
        opener: &RingInitialCrypto,
        mut protected_packet: Vec<u8>,
        cleartext_payload: &[u8],
        on_decrypt: F,
    ) -> O {
        let decoder = DecoderBufferMut::new(&mut protected_packet);
        let (packet, _) = ProtectedPacket::decode(decoder, &20).unwrap();

        let packet = match packet {
            ProtectedPacket::Initial(initial) => initial,
            _ => panic!("expected initial packet type"),
        };

        let packet = packet.unprotect(opener, Default::default()).unwrap();
        let packet = packet.decrypt(opener).unwrap();

        // trim any padding off of the end
        let actual_payload = &packet.payload.as_less_safe_slice()[..cleartext_payload.len()];
        assert_eq!(cleartext_payload, actual_payload);

        on_decrypt(packet)
    }
}

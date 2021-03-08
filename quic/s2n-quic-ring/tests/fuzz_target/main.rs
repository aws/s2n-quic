// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use bolero::{check, generator::*};
use ring::{
    aead::{AES_128_GCM, AES_256_GCM, CHACHA20_POLY1305},
    hkdf,
    hkdf::KeyType,
};
use s2n_quic_core::crypto::{
    initial::InitialKey, key::Key, CryptoError, HeaderKey, InitialHeaderKey,
};
use s2n_quic_ring::{
    handshake::{RingHandshakeHeaderKey, RingHandshakeKey},
    initial::{RingInitialHeaderKey, RingInitialKey},
    one_rtt::{RingOneRttHeaderKey, RingOneRttKey},
    zero_rtt::{RingZeroRttHeaderKey, RingZeroRttKey},
    Algorithm, Prk, SecretPair,
};

fn main() {
    check!()
        .with_generator((
            gen_crypto(),
            gen(),
            gen_unique_bytes(0..20),
            gen_unique_bytes(0..20),
        ))
        .for_each(|(crypto, packet_number, header, payload)| {
            let packet_number = *packet_number;
            match crypto {
                CryptoTest::Initial {
                    ref server_key,
                    ref client_key,
                    ref server_header_key,
                    ref client_header_key,
                } => {
                    assert!(test_round_trip(
                        server_key,
                        client_key,
                        server_header_key,
                        client_header_key,
                        packet_number,
                        &header,
                        &payload
                    )
                    .is_ok());
                    assert!(test_round_trip(
                        client_key,
                        server_key,
                        client_header_key,
                        server_header_key,
                        packet_number,
                        &header,
                        &payload
                    )
                    .is_ok());
                }
                CryptoTest::Handshake {
                    ref server_key,
                    ref client_key,
                    ref server_header_key,
                    ref client_header_key,
                } => {
                    assert!(test_round_trip(
                        server_key,
                        client_key,
                        server_header_key,
                        client_header_key,
                        packet_number,
                        &header,
                        &payload
                    )
                    .is_ok());
                    assert!(test_round_trip(
                        client_key,
                        server_key,
                        client_header_key,
                        server_header_key,
                        packet_number,
                        &header,
                        &payload
                    )
                    .is_ok());
                    let server_key = server_key.update();
                    assert!(test_round_trip(
                        &server_key,
                        client_key,
                        server_header_key,
                        client_header_key,
                        packet_number,
                        &header,
                        &payload
                    )
                    .is_err());
                    assert!(test_round_trip(
                        client_key,
                        &server_key,
                        client_header_key,
                        server_header_key,
                        packet_number,
                        &header,
                        &payload
                    )
                    .is_err());
                    let client_key = client_key.update();
                    assert!(test_round_trip(
                        &server_key,
                        &client_key,
                        server_header_key,
                        client_header_key,
                        packet_number,
                        &header,
                        &payload
                    )
                    .is_ok());
                    assert!(test_round_trip(
                        &client_key,
                        &server_key,
                        client_header_key,
                        server_header_key,
                        packet_number,
                        &header,
                        &payload
                    )
                    .is_ok());
                }
                CryptoTest::OneRTT {
                    ref server_key,
                    ref client_key,
                    ref server_header_key,
                    ref client_header_key,
                } => {
                    assert!(test_round_trip(
                        server_key,
                        client_key,
                        server_header_key,
                        client_header_key,
                        packet_number,
                        &header,
                        &payload
                    )
                    .is_ok());
                    assert!(test_round_trip(
                        client_key,
                        server_key,
                        client_header_key,
                        server_header_key,
                        packet_number,
                        &header,
                        &payload
                    )
                    .is_ok());
                    let server_key = server_key.update();
                    assert!(test_round_trip(
                        &server_key,
                        client_key,
                        server_header_key,
                        client_header_key,
                        packet_number,
                        &header,
                        &payload
                    )
                    .is_err());
                    assert!(test_round_trip(
                        client_key,
                        &server_key,
                        client_header_key,
                        server_header_key,
                        packet_number,
                        &header,
                        &payload
                    )
                    .is_err());
                    let client_key = client_key.update();
                    assert!(test_round_trip(
                        &server_key,
                        &client_key,
                        server_header_key,
                        client_header_key,
                        packet_number,
                        &header,
                        &payload
                    )
                    .is_ok());
                    assert!(test_round_trip(
                        &client_key,
                        &server_key,
                        client_header_key,
                        server_header_key,
                        packet_number,
                        &header,
                        &payload
                    )
                    .is_ok());
                }
                CryptoTest::ZeroRTT {
                    ref key,
                    ref header_key,
                } => {
                    assert!(test_round_trip(
                        key,
                        key,
                        header_key,
                        header_key,
                        packet_number,
                        &header,
                        &payload
                    )
                    .is_ok());
                }
            }
        });
}

fn test_round_trip<K: Key, H: HeaderKey>(
    sealer_key: &K,
    opener: &K,
    sealer_header_key: &H,
    opener_header_key: &H,
    packet_number: u64,
    header: &[u8],
    orig_payload: &[u8],
) -> Result<(), CryptoError> {
    let mut payload = orig_payload.to_vec();
    payload.resize(orig_payload.len() + sealer_key.tag_len(), 0);

    sealer_key
        .encrypt(packet_number, header, &mut payload)
        .expect("encryption should always work");

    assert_ne!(orig_payload, &payload[..], "payload should be encrypted");

    opener.decrypt(packet_number, header, &mut payload)?;

    assert_eq!(
        orig_payload[..],
        payload[..orig_payload.len()],
        "payload should be decrypted"
    );

    let sample_len = opener_header_key.opening_sample_len();
    assert_eq!(
        sealer_header_key.sealing_header_protection_mask(&payload[..sample_len]),
        opener_header_key.opening_header_protection_mask(&payload[..sample_len])
    );

    Ok(())
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug)]
enum CryptoTest {
    Initial {
        server_key: RingInitialKey,
        client_key: RingInitialKey,
        server_header_key: RingInitialHeaderKey,
        client_header_key: RingInitialHeaderKey,
    },
    Handshake {
        server_key: RingHandshakeKey,
        client_key: RingHandshakeKey,
        server_header_key: RingHandshakeHeaderKey,
        client_header_key: RingHandshakeHeaderKey,
    },
    OneRTT {
        server_key: RingOneRttKey,
        client_key: RingOneRttKey,
        server_header_key: RingOneRttHeaderKey,
        client_header_key: RingOneRttHeaderKey,
    },
    ZeroRTT {
        key: RingZeroRttKey,
        header_key: RingZeroRttHeaderKey,
    },
}

fn gen_crypto() -> impl ValueGenerator<Output = CryptoTest> {
    one_of((
        gen_initial(),
        gen_handshake(),
        gen_one_rtt(),
        gen_zero_rtt(),
    ))
}

fn gen_initial() -> impl ValueGenerator<Output = CryptoTest> {
    gen_dcid().map(|dcid| {
        let server_key = RingInitialKey::new_server(&dcid);
        let client_key = RingInitialKey::new_client(&dcid);
        let server_header_key = RingInitialHeaderKey::new_server(&dcid);
        let client_header_key = RingInitialHeaderKey::new_client(&dcid);
        CryptoTest::Initial {
            server_key,
            client_key,
            server_header_key,
            client_header_key,
        }
    })
}

fn gen_dcid() -> impl ValueGenerator<Output = Vec<u8>> {
    gen_unique_bytes(0..=20)
}

fn gen_handshake() -> impl ValueGenerator<Output = CryptoTest> {
    gen_negotiated_secrets().map(|(algo, secrets)| {
        let (server_key, server_header_key) =
            RingHandshakeKey::new_server(&algo, secrets.clone()).unwrap();
        let (client_key, client_header_key) = RingHandshakeKey::new_client(&algo, secrets).unwrap();
        CryptoTest::Handshake {
            server_key,
            client_key,
            server_header_key,
            client_header_key,
        }
    })
}

fn gen_one_rtt() -> impl ValueGenerator<Output = CryptoTest> {
    gen_negotiated_secrets().map(|(algo, secrets)| {
        let (server_key, server_header_key) =
            RingOneRttKey::new_server(&algo, secrets.clone()).unwrap();
        let (client_key, client_header_key) = RingOneRttKey::new_client(&algo, secrets).unwrap();
        CryptoTest::OneRTT {
            server_key,
            client_key,
            server_header_key,
            client_header_key,
        }
    })
}

fn gen_zero_rtt() -> impl ValueGenerator<Output = CryptoTest> {
    gen_secret(hkdf::HKDF_SHA256).map(|secret| {
        let (key, header_key) = RingZeroRttKey::new(secret);
        CryptoTest::ZeroRTT { key, header_key }
    })
}

fn gen_negotiated_secrets() -> impl ValueGenerator<Output = (&'static Algorithm, SecretPair)> {
    (0u8..3).and_then_gen(|i| {
        let (algo, hkdf) = match i {
            0 => (&AES_128_GCM, hkdf::HKDF_SHA256),
            1 => (&AES_256_GCM, hkdf::HKDF_SHA384),
            2 => (&CHACHA20_POLY1305, hkdf::HKDF_SHA256),
            _ => unreachable!(),
        };

        gen_secrets(hkdf).map_gen(move |secrets| (algo, secrets))
    })
}

fn gen_secrets(algo: hkdf::Algorithm) -> impl ValueGenerator<Output = SecretPair> {
    (gen_secret(algo), gen_secret(algo))
        .map_gen(move |(client, server)| SecretPair { client, server })
}

fn gen_secret(algo: hkdf::Algorithm) -> impl ValueGenerator<Output = Prk> {
    gen_unique_bytes(algo.len()).map(move |secret| Prk::new_less_safe(algo, &secret))
}

fn gen_unique_bytes<L: ValueGenerator<Output = usize>>(
    len: L,
) -> impl ValueGenerator<Output = Vec<u8>> {
    len.map(|len| {
        use core::sync::atomic::{AtomicUsize, Ordering::*};

        if len == 0 {
            return vec![];
        }

        static NUM: AtomicUsize = AtomicUsize::new(0);

        let mut bytes = NUM.fetch_add(1, SeqCst).to_le_bytes().to_vec();
        bytes.resize(len, 0);
        bytes
    })
}

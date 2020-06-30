use bolero::{fuzz, generator::*};
use ring::{
    aead::{AES_128_GCM, AES_256_GCM, CHACHA20_POLY1305},
    hkdf,
    hkdf::KeyType,
};
use s2n_quic_core::crypto::{initial::InitialCrypto, key::Key, CryptoError, HeaderCrypto};
use s2n_quic_ring::{
    handshake::RingHandshakeCrypto, initial::RingInitialCrypto, one_rtt::RingOneRTTCrypto,
    zero_rtt::RingZeroRTTCrypto, Algorithm, Prk, SecretPair,
};

fn main() {
    fuzz!()
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
                    ref server,
                    ref client,
                } => {
                    assert!(
                        test_round_trip(server, client, packet_number, &header, &payload).is_ok()
                    );
                    assert!(
                        test_round_trip(client, server, packet_number, &header, &payload).is_ok()
                    );
                }
                CryptoTest::Handshake {
                    ref server,
                    ref client,
                } => {
                    assert!(
                        test_round_trip(server, client, packet_number, &header, &payload).is_ok()
                    );
                    assert!(
                        test_round_trip(client, server, packet_number, &header, &payload).is_ok()
                    );
                    let server = server.update();
                    assert!(
                        test_round_trip(&server, client, packet_number, &header, &payload).is_err()
                    );
                    assert!(
                        test_round_trip(client, &server, packet_number, &header, &payload).is_err()
                    );
                    let client = client.update();
                    assert!(
                        test_round_trip(&server, &client, packet_number, &header, &payload).is_ok()
                    );
                    assert!(
                        test_round_trip(&client, &server, packet_number, &header, &payload).is_ok()
                    );
                }
                CryptoTest::OneRTT {
                    ref server,
                    ref client,
                } => {
                    assert!(
                        test_round_trip(server, client, packet_number, &header, &payload).is_ok()
                    );
                    assert!(
                        test_round_trip(client, server, packet_number, &header, &payload).is_ok()
                    );
                    let server = server.update();
                    assert!(
                        test_round_trip(&server, client, packet_number, &header, &payload).is_err()
                    );
                    assert!(
                        test_round_trip(client, &server, packet_number, &header, &payload).is_err()
                    );
                    let client = client.update();
                    assert!(
                        test_round_trip(&server, &client, packet_number, &header, &payload).is_ok()
                    );
                    assert!(
                        test_round_trip(&client, &server, packet_number, &header, &payload).is_ok()
                    );
                }
                CryptoTest::ZeroRTT { ref crypto } => {
                    assert!(
                        test_round_trip(crypto, crypto, packet_number, &header, &payload).is_ok()
                    );
                }
            }
        });
}

fn test_round_trip<C: Key + HeaderCrypto>(
    sealer: &C,
    opener: &C,
    packet_number: u64,
    header: &[u8],
    orig_payload: &[u8],
) -> Result<(), CryptoError> {
    let mut payload = orig_payload.to_vec();
    payload.resize(orig_payload.len() + sealer.tag_len(), 0);

    sealer
        .encrypt(packet_number, header, &mut payload)
        .expect("encryption should always work");

    assert_ne!(orig_payload, &payload[..], "payload should be encrypted");

    opener.decrypt(packet_number, header, &mut payload)?;

    assert_eq!(
        orig_payload[..],
        payload[..orig_payload.len()],
        "payload should be decrypted"
    );

    let sample_len = opener.opening_sample_len();
    assert_eq!(
        sealer.sealing_header_protection_mask(&payload[..sample_len]),
        opener.opening_header_protection_mask(&payload[..sample_len])
    );

    Ok(())
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug)]
enum CryptoTest {
    Initial {
        server: RingInitialCrypto,
        client: RingInitialCrypto,
    },
    Handshake {
        server: RingHandshakeCrypto,
        client: RingHandshakeCrypto,
    },
    OneRTT {
        server: RingOneRTTCrypto,
        client: RingOneRTTCrypto,
    },
    ZeroRTT {
        crypto: RingZeroRTTCrypto,
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
        let server = RingInitialCrypto::new_server(&dcid);
        let client = RingInitialCrypto::new_client(&dcid);
        CryptoTest::Initial { server, client }
    })
}

fn gen_dcid() -> impl ValueGenerator<Output = Vec<u8>> {
    gen_unique_bytes(0..=20)
}

fn gen_handshake() -> impl ValueGenerator<Output = CryptoTest> {
    gen_negotiated_secrets().map(|(algo, secrets)| {
        let server = RingHandshakeCrypto::new_server(&algo, secrets.clone()).unwrap();
        let client = RingHandshakeCrypto::new_client(&algo, secrets).unwrap();
        CryptoTest::Handshake { server, client }
    })
}

fn gen_one_rtt() -> impl ValueGenerator<Output = CryptoTest> {
    gen_negotiated_secrets().map(|(algo, secrets)| {
        let server = RingOneRTTCrypto::new_server(&algo, secrets.clone()).unwrap();
        let client = RingOneRTTCrypto::new_client(&algo, secrets).unwrap();
        CryptoTest::OneRTT { server, client }
    })
}

fn gen_zero_rtt() -> impl ValueGenerator<Output = CryptoTest> {
    gen_secret(hkdf::HKDF_SHA256).map(|secret| {
        let crypto = RingZeroRTTCrypto::new(secret);
        CryptoTest::ZeroRTT { crypto }
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

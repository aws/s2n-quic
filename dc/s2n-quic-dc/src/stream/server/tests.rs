// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::Authenticator;
use crate::{
    credentials::Credentials,
    crypto::{awslc, open::Application as _},
    packet::stream::{self, decoder, encoder},
    path::secret::{self, map::Bidirectional},
    stream::TransportFeatures,
};
use s2n_codec::{DecoderBufferMut, EncoderBuffer};
use s2n_quic_core::{buffer::reader::incremental::Incremental, varint::VarInt};
use std::net::SocketAddr;

const CLIENT_ADDR: &str = "127.0.0.1:1111";
const SERVER_ADDR: &str = "127.0.0.1:2222";
const PAYLOAD: &[u8] = b"hello";

/// A client and server that share a path secret, as they would after a handshake.
struct Pair {
    client: Bidirectional,
    server_map: secret::Map,
}

impl Pair {
    fn new() -> Self {
        let client_addr: SocketAddr = CLIENT_ADDR.parse().unwrap();
        let server_addr: SocketAddr = SERVER_ADDR.parse().unwrap();

        let client_map = secret::map::testing::new(16);
        let server_map = secret::map::testing::new(16);
        client_map.test_insert_pair(client_addr, None, &server_map, server_addr, None);

        let peer = client_map.get_tracked(server_addr).unwrap();
        let (client, _params) = peer.pair(&TransportFeatures::UDP);

        Self { client, server_map }
    }

    /// Encodes a stream packet sealed with the client's application key.
    fn encode(&self) -> Vec<u8> {
        let stream_id = stream::Id::normal(VarInt::ZERO).unwrap();

        let mut payload = PAYLOAD;
        let mut incremental = Incremental::new(VarInt::ZERO);
        let mut reader = incremental.with_storage(&mut payload, true).unwrap();

        let mut buffer = vec![0u8; 1024];
        let len = encoder::encode(
            EncoderBuffer::new(&mut buffer),
            None,
            stream_id,
            VarInt::ZERO,
            VarInt::ZERO,
            VarInt::ZERO,
            &mut &[][..],
            VarInt::ZERO,
            &(),
            &mut reader,
            &self.client.application.sealer,
            &self.client.credentials,
        );
        buffer.truncate(len);
        buffer
    }

    /// Encodes a recovery-space probe, which is signed with the control key and has no payload.
    fn probe(&self) -> Vec<u8> {
        let control = self.client.control.as_ref().unwrap();
        let stream_id = stream::Id::normal(VarInt::ZERO).unwrap();

        let mut payload = &b""[..];
        let mut incremental = Incremental::new(VarInt::ZERO);
        let mut reader = incremental.with_storage(&mut payload, false).unwrap();

        let mut buffer = vec![0u8; 1024];
        let len = encoder::probe(
            EncoderBuffer::new(&mut buffer),
            None,
            stream_id,
            VarInt::ZERO,
            VarInt::ZERO,
            VarInt::ZERO,
            &mut &[][..],
            VarInt::ZERO,
            &(),
            &mut reader,
            &control.sealer,
            &self.client.credentials,
        );
        buffer.truncate(len);
        buffer
    }

    /// Re-encodes `packet` as a retransmission, the way the client's send path does when a packet
    /// goes unacknowledged.
    fn retransmit(&self, packet: &mut [u8], packet_number: VarInt) {
        let control = self.client.control.as_ref().unwrap();
        decoder::Packet::retransmit(
            DecoderBufferMut::new(packet),
            stream::PacketSpace::Stream,
            packet_number,
            &control.sealer,
        )
        .unwrap();
    }

    /// Derives the server's keys for `credentials`, as the acceptor does via
    /// `endpoint::derive_stream_credentials`.
    fn server_crypto(&self, credentials: &Credentials) -> Bidirectional {
        let mut control_out = vec![];
        let (crypto, _params, _application_data) = self
            .server_map
            .pair_for_credentials(credentials, None, &TransportFeatures::UDP, &mut control_out)
            .expect("server holds the path secret");
        assert!(control_out.is_empty());
        crypto
    }

    fn credentials(&self) -> Credentials {
        self.client.credentials
    }
}

/// Decrypts `packet` the way the stream reader does with the keys the acceptor derived.
///
/// Only the decrypt result is returned; a packet that fails to *decode* means the test itself is
/// broken, so that case panics rather than being folded into the error type under test.
#[allow(
    clippy::panic_in_result_fn,
    clippy::unwrap_in_result,
    clippy::unwrap_used,
    reason = "test helper: a packet that doesn't decode is a broken test, not a decrypt failure"
)]
fn read(crypto: &Bidirectional, packet: &mut [u8]) -> Result<Vec<u8>, crate::crypto::open::Error> {
    let tag_len = crypto.application.opener.tag_len();
    let control = crypto.control.as_ref().unwrap();

    let decoder = DecoderBufferMut::new(packet);
    let (crate::packet::Packet::Stream(mut packet), _remaining) =
        decoder.decode_parameterized(tag_len).unwrap()
    else {
        panic!("not a stream packet");
    };

    packet.decrypt_in_place(&crypto.application.opener, &control.opener)?;

    Ok(packet.payload().to_vec())
}

#[test]
fn authentic_packet_is_accepted() {
    let pair = Pair::new();
    let packet = pair.encode();
    let crypto = pair.server_crypto(&pair.credentials());

    let mut authenticator = Authenticator::default();
    assert!(authenticator.authenticate_first_packet(&crypto, &packet));
}

/// The acceptor hands the untouched segment to the stream, which decrypts it with the *same* keys.
/// That second decrypt has to succeed: the replay check is memoized on the `Bidirectional`, so
/// verifying at accept time must not consume the `key_id`.
#[test]
fn verifying_does_not_consume_the_key_id() {
    let pair = Pair::new();
    let mut packet = pair.encode();
    let crypto = pair.server_crypto(&pair.credentials());

    let mut authenticator = Authenticator::default();
    assert!(authenticator.authenticate_first_packet(&crypto, &packet));

    let payload = read(&crypto, &mut packet).expect("the stream reader decrypts the same bytes");
    assert_eq!(payload, PAYLOAD);
}

/// Deriving a *second* key pair to verify with would mark the `key_id` twice, so the stream's own
/// decrypt would reject the peer's packet as a replay. This pins that hazard down, since it is the
/// reason the acceptor derives once and shares the result.
#[test]
fn verifying_with_a_second_key_pair_is_rejected_as_a_replay() {
    let pair = Pair::new();
    let mut packet = pair.encode();

    let verify_crypto = pair.server_crypto(&pair.credentials());
    let mut authenticator = Authenticator::default();
    assert!(authenticator.authenticate_first_packet(&verify_crypto, &packet));

    let stream_crypto = pair.server_crypto(&pair.credentials());
    let error = read(&stream_crypto, &mut packet).expect_err("second pair sees its own replay");
    assert!(
        matches!(error, crate::crypto::open::Error::ReplayDefinitelyDetected),
        "unexpected error: {error:?}"
    );
}

/// A retransmission is what the server sees when its answer to the original was lost, so it has to
/// authenticate. `decrypt` rewrites the auth tag, the recovery bit and the retransmission offset in
/// place for these, which is why the check runs on a copy - this asserts the caller's bytes come
/// back untouched and still decrypt afterwards.
#[test]
fn retransmitted_packet_is_accepted_without_mutating_the_caller_s_bytes() {
    let pair = Pair::new();
    let mut packet = pair.encode();
    pair.retransmit(&mut packet, VarInt::from_u8(1));

    let original = packet.clone();
    let crypto = pair.server_crypto(&pair.credentials());

    let mut authenticator = Authenticator::default();
    assert!(authenticator.authenticate_first_packet(&crypto, &packet));
    assert_eq!(original, packet, "the segment must not be modified");

    let payload = read(&crypto, &mut packet).expect("the stream reader decrypts the same bytes");
    assert_eq!(payload, PAYLOAD);
}

/// The forged case: a syntactically valid packet naming credentials the server knows, sealed with a
/// key it does not have.
#[test]
fn packet_sealed_with_an_unrelated_key_is_rejected() {
    let pair = Pair::new();
    let credentials = pair.credentials();

    let sealer =
        awslc::seal::Application::new(b"not-the-real-key", [0x11; 12], &awslc::AES_128_GCM);
    let stream_id = stream::Id::normal(VarInt::ZERO).unwrap();

    let mut payload = PAYLOAD;
    let mut incremental = Incremental::new(VarInt::ZERO);
    let mut reader = incremental.with_storage(&mut payload, true).unwrap();

    let mut packet = vec![0u8; 1024];
    let len = encoder::encode(
        EncoderBuffer::new(&mut packet),
        None,
        stream_id,
        VarInt::ZERO,
        VarInt::ZERO,
        VarInt::ZERO,
        &mut &[][..],
        VarInt::ZERO,
        &(),
        &mut reader,
        &sealer,
        &credentials,
    );
    packet.truncate(len);

    let crypto = pair.server_crypto(&credentials);
    let mut authenticator = Authenticator::default();
    assert!(!authenticator.authenticate_first_packet(&crypto, &packet));
}

#[test]
fn tampered_payload_is_rejected() {
    let pair = Pair::new();
    let mut packet = pair.encode();
    let crypto = pair.server_crypto(&pair.credentials());

    // flip a bit in the last payload byte, before the auth tag
    let tag_len = crypto.application.opener.tag_len();
    let idx = packet.len() - tag_len - 1;
    packet[idx] ^= 1;

    let mut authenticator = Authenticator::default();
    assert!(!authenticator.authenticate_first_packet(&crypto, &packet));
}

/// The header is AAD, not ciphertext, which is what lets the acceptor act on the credentials and
/// peer address once the packet verifies.
#[test]
fn tampered_header_is_rejected() {
    let pair = Pair::new();
    let crypto = pair.server_crypto(&pair.credentials());
    let mut authenticator = Authenticator::default();

    // byte 1 starts the credential id: flipping inside it leaves the packet parseable
    for idx in [1, 5, 16] {
        let mut packet = pair.encode();
        packet[idx] ^= 1;
        assert!(
            !authenticator.authenticate_first_packet(&crypto, &packet),
            "a flipped bit at {idx} must not authenticate"
        );
    }
}

/// Every prefix fails to decode, so this covers the decode path and the degenerate scratch split
/// rather than the AEAD check.
#[test]
fn truncated_packet_is_rejected() {
    let pair = Pair::new();
    let packet = pair.encode();
    let crypto = pair.server_crypto(&pair.credentials());

    let mut authenticator = Authenticator::default();
    for len in 0..packet.len() {
        assert!(
            !authenticator.authenticate_first_packet(&crypto, &packet[..len]),
            "a {len} byte prefix must not authenticate"
        );
    }
}

/// The scratch buffer is reused across calls, so reuse - including a shorter packet after a longer
/// one - must not change the verdict.
#[test]
fn scratch_buffer_is_reused_across_calls() {
    let pair = Pair::new();
    let authentic = pair.encode();
    let crypto = pair.server_crypto(&pair.credentials());

    let mut tampered = authentic.clone();
    let last = tampered.len() - 1;
    tampered[last] ^= 1;

    let mut authenticator = Authenticator::default();
    for _ in 0..4 {
        assert!(!authenticator.authenticate_first_packet(&crypto, &tampered));
        assert!(!authenticator.authenticate_first_packet(&crypto, &authentic[..8]));
        assert!(authenticator.authenticate_first_packet(&crypto, &authentic));
    }
}

/// Only a stream packet can claim a stream, so the other tags must be turned away. Note that a
/// zero first byte is a *stream* tag ([`stream::Tag::MIN`]), not a stand-in for "not a stream".
#[test]
fn non_stream_packet_is_rejected() {
    let pair = Pair::new();
    let crypto = pair.server_crypto(&pair.credentials());
    let mut authenticator = Authenticator::default();

    for (name, tag) in [
        ("datagram", 0b0100_0000u8),
        ("control", 0b0101_0000u8),
        ("reserved", 0b0111_0000u8),
        ("long", 0b1000_0000u8),
    ] {
        let mut packet = vec![0u8; 64];
        packet[0] = tag;
        assert!(
            !authenticator.authenticate_first_packet(&crypto, &packet),
            "a {name} packet must not authenticate"
        );
    }
}

/// A recovery-space packet carries no payload and is authenticated by the control key alone, so it
/// takes the other branch of `decrypt`.
#[test]
fn recovery_packet_is_authenticated_by_the_control_key() {
    let pair = Pair::new();
    let crypto = pair.server_crypto(&pair.credentials());
    let mut authenticator = Authenticator::default();

    let authentic = pair.probe();
    assert!(authenticator.authenticate_first_packet(&crypto, &authentic));

    let mut tampered = authentic.clone();
    let last = tampered.len() - 1;
    tampered[last] ^= 1;
    assert!(!authenticator.authenticate_first_packet(&crypto, &tampered));
}

/// A zero-length payload makes the plaintext half of the scratch buffer degenerate, so check that
/// the split still lines up.
#[test]
fn empty_payload_packet_is_accepted() {
    let pair = Pair::new();

    let stream_id = stream::Id::normal(VarInt::ZERO).unwrap();
    let mut payload = &b""[..];
    let mut incremental = Incremental::new(VarInt::ZERO);
    let mut reader = incremental.with_storage(&mut payload, true).unwrap();

    let mut packet = vec![0u8; 1024];
    let len = encoder::encode(
        EncoderBuffer::new(&mut packet),
        None,
        stream_id,
        VarInt::ZERO,
        VarInt::ZERO,
        VarInt::ZERO,
        &mut &[][..],
        VarInt::ZERO,
        &(),
        &mut reader,
        &pair.client.application.sealer,
        &pair.client.credentials,
    );
    packet.truncate(len);

    let crypto = pair.server_crypto(&pair.credentials());
    let mut authenticator = Authenticator::default();
    assert!(authenticator.authenticate_first_packet(&crypto, &packet));
}

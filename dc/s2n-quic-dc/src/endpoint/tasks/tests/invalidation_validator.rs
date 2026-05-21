// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::helpers::{TestReceiver, TestReceiverExt as _};
use crate::{
    credentials::Id,
    endpoint::{id::SendWorkerId, tasks},
    intrusive::Entry,
    packet::{secret_control, WireVersion},
    path::secret::{map::Map, schedule, stateless_reset},
    socket::{channel::intrusive::unsync, pool::Pool},
    testing::{ext::*, sim},
};
use s2n_codec::EncoderBuffer;
use s2n_quic_core::{dc, endpoint::Type, time::NoopClock};
use std::net::SocketAddr;

const SIGNER_SECRET: &[u8] = b"invalidation-validator-test-signer";
const DETERMINISTIC_SECRET: [u8; 32] = [42u8; 32];

fn validator_counters() -> tasks::ValidatorInvalidationCounters {
    let registry = crate::counter::Registry::default();
    tasks::ValidatorInvalidationCounters {
        unknown_path_secret_validated: registry.register("test.invalidation.validator.ups"),
        stale_key_validated: registry.register("test.invalidation.validator.stale"),
        replay_detected_validated: registry.register("test.invalidation.validator.replay"),
    }
}

fn setup_map_with_entry(peer: SocketAddr) -> (Map, Id) {
    let map = Map::new(
        stateless_reset::Signer::new(SIGNER_SECRET),
        16,
        true,
        NoopClock,
        crate::event::tracing::Subscriber::default(),
    );
    map.test_stop_cleaner();
    map.test_insert_deterministic(peer, Type::Client);

    let secret = schedule::Secret::new(
        schedule::Ciphersuite::AES_GCM_128_SHA256,
        dc::SUPPORTED_VERSIONS[0],
        Type::Client,
        &DETERMINISTIC_SECRET,
    );
    (map, *secret.id())
}

fn encode_unknown_path_secret(
    credential_id: Id,
    stateless_reset: [u8; secret_control::TAG_LEN],
) -> Vec<u8> {
    let mut out = [0u8; secret_control::MAX_PACKET_SIZE];
    let len = secret_control::UnknownPathSecret {
        wire_version: WireVersion::ZERO,
        credential_id,
        queue_id: None,
    }
    .encode(EncoderBuffer::new(&mut out), &stateless_reset);
    out[..len].to_vec()
}

fn encode_stale_key(
    credential_id: Id,
    sender_id: s2n_quic_core::varint::VarInt,
    min_key_id: s2n_quic_core::varint::VarInt,
    control_sealer: crate::crypto::awslc::seal::control::Secret,
) -> Vec<u8> {
    let mut out = [0u8; secret_control::MAX_PACKET_SIZE];
    let len = secret_control::StaleKey {
        wire_version: WireVersion::ZERO,
        credential_id,
        sender_id: Some(sender_id),
        min_key_id,
    }
    .encode(EncoderBuffer::new(&mut out), &control_sealer);
    out[..len].to_vec()
}

fn encode_replay_detected(
    credential_id: Id,
    sender_id: s2n_quic_core::varint::VarInt,
    rejected_key_id: s2n_quic_core::varint::VarInt,
    control_sealer: crate::crypto::awslc::seal::control::Secret,
) -> Vec<u8> {
    let mut out = [0u8; secret_control::MAX_PACKET_SIZE];
    let len = secret_control::ReplayDetected {
        wire_version: WireVersion::ZERO,
        credential_id,
        rejected_key_id,
        sender_id: Some(sender_id),
    }
    .encode(EncoderBuffer::new(&mut out), &control_sealer);
    out[..len].to_vec()
}

fn packet_entry(
    payload: &[u8],
    peer: SocketAddr,
) -> Entry<crate::socket::pool::descriptor::Filled> {
    let unfilled = Pool::new(1200)
        .alloc()
        .expect("packet allocation should succeed");
    let segments = unfilled
        .fill_with(|addr, _cmsg, mut buffer| {
            addr.set(peer.into());
            buffer[..payload.len()].copy_from_slice(payload);
            Ok::<usize, core::convert::Infallible>(payload.len())
        })
        .expect("packet fill should succeed");
    Entry::new(segments.take_filled())
}

#[test]
fn unknown_path_secret_packet_broadcasts_validated_id() {
    sim(|| {
        let peer: SocketAddr = "127.0.0.1:4444".parse().unwrap();
        let (map, local_id) = setup_map_with_entry(peer);

        let wire_id = local_id.for_peer();
        let stateless_reset = stateless_reset::Signer::new(SIGNER_SECRET).sign(&local_id);
        let payload = encode_unknown_path_secret(wire_id, stateless_reset);
        let input = TestReceiver::new([packet_entry(&payload, peer)]);

        let (tx_a, mut rx_a) = unsync::new::<tasks::Invalidation>();
        let (tx_b, mut rx_b) = unsync::new::<tasks::Invalidation>();
        let mut rx = tasks::invalidation_validator(
            input,
            map,
            vec![tx_a].into(),
            vec![tx_b].into(),
            vec![SendWorkerId::new(0)].into(),
            validator_counters(),
        );

        async move {
            assert!(rx.recv().await.is_some());
            drop(rx);

            assert_eq!(
                *rx_a.recv().await.expect("first output should receive id"),
                tasks::Invalidation::UnknownPathSecret {
                    credential_id: local_id
                }
            );
            assert_eq!(
                *rx_b.recv().await.expect("second output should receive id"),
                tasks::Invalidation::UnknownPathSecret {
                    credential_id: local_id
                }
            );
            assert!(rx_a.recv().await.is_none());
            assert!(rx_b.recv().await.is_none());
        }
        .primary()
        .spawn();
    });
}

#[test]
fn malformed_packet_is_ignored() {
    sim(|| {
        let peer: SocketAddr = "127.0.0.1:5555".parse().unwrap();
        let (map, _local_id) = setup_map_with_entry(peer);

        let input = TestReceiver::new([packet_entry(&[0x12, 0x34, 0x56], peer)]);
        let (send_tx, mut output_rx) = unsync::new::<tasks::Invalidation>();
        let mut rx = tasks::invalidation_validator(
            input,
            map,
            vec![send_tx].into(),
            vec![].into(),
            vec![SendWorkerId::new(0)].into(),
            validator_counters(),
        );

        async move {
            assert!(rx.recv().await.is_some());
            drop(rx);
            assert!(
                output_rx.recv().await.is_none(),
                "invalid packet should be ignored"
            );
        }
        .primary()
        .spawn();
    });
}

#[test]
fn stale_key_packet_broadcasts_validated_sender_target() {
    sim(|| {
        let peer: SocketAddr = "127.0.0.1:6666".parse().unwrap();
        let (map, local_id) = setup_map_with_entry(peer);
        let peer_secret = schedule::Secret::new(
            schedule::Ciphersuite::AES_GCM_128_SHA256,
            dc::SUPPORTED_VERSIONS[0],
            Type::Server,
            &DETERMINISTIC_SECRET,
        );

        let wire_id = local_id.for_peer();
        let sender_id = s2n_quic_core::varint::VarInt::from_u8(7);
        let payload = encode_stale_key(
            wire_id,
            sender_id,
            s2n_quic_core::varint::VarInt::from_u8(3),
            peer_secret.control_sealer(),
        );
        let input = TestReceiver::new([packet_entry(&payload, peer)]);
        let (tx_a, mut rx_a) = unsync::new::<tasks::Invalidation>();
        let (tx_b, mut rx_b) = unsync::new::<tasks::Invalidation>();
        let mut sender_id_to_worker = vec![SendWorkerId::new(0); 8];
        sender_id_to_worker[7] = SendWorkerId::new(1);
        let mut rx = tasks::invalidation_validator(
            input,
            map,
            vec![tx_a, tx_b].into(),
            vec![].into(),
            sender_id_to_worker.into(),
            validator_counters(),
        );

        async move {
            assert!(rx.recv().await.is_some());
            drop(rx);
            assert_eq!(
                *rx_b.recv().await.expect("stale key should be propagated"),
                tasks::Invalidation::StaleKey {
                    credential_id: local_id,
                    sender_id: crate::endpoint::id::LocalSenderId::new(sender_id),
                    rejected_key_id: s2n_quic_core::varint::VarInt::from_u8(3),
                }
            );
            assert!(rx_a.recv().await.is_none());
            assert!(rx_b.recv().await.is_none());
        }
        .primary()
        .spawn();
    });
}

#[test]
fn replay_detected_packet_broadcasts_validated_sender_target() {
    sim(|| {
        let peer: SocketAddr = "127.0.0.1:7777".parse().unwrap();
        let (map, local_id) = setup_map_with_entry(peer);
        let peer_secret = schedule::Secret::new(
            schedule::Ciphersuite::AES_GCM_128_SHA256,
            dc::SUPPORTED_VERSIONS[0],
            Type::Server,
            &DETERMINISTIC_SECRET,
        );

        let wire_id = local_id.for_peer();
        let sender_id = s2n_quic_core::varint::VarInt::from_u8(9);
        let payload = encode_replay_detected(
            wire_id,
            sender_id,
            s2n_quic_core::varint::VarInt::from_u8(2),
            peer_secret.control_sealer(),
        );
        let input = TestReceiver::new([packet_entry(&payload, peer)]);
        let (tx_a, mut rx_a) = unsync::new::<tasks::Invalidation>();
        let (tx_b, mut rx_b) = unsync::new::<tasks::Invalidation>();
        let mut sender_id_to_worker = vec![SendWorkerId::new(0); 10];
        sender_id_to_worker[9] = SendWorkerId::new(1);
        let mut rx = tasks::invalidation_validator(
            input,
            map,
            vec![tx_a, tx_b].into(),
            vec![].into(),
            sender_id_to_worker.into(),
            validator_counters(),
        );

        async move {
            assert!(rx.recv().await.is_some());
            drop(rx);
            assert_eq!(
                *rx_b
                    .recv()
                    .await
                    .expect("replay detected should be propagated"),
                tasks::Invalidation::StaleKey {
                    credential_id: local_id,
                    sender_id: crate::endpoint::id::LocalSenderId::new(sender_id),
                    rejected_key_id: s2n_quic_core::varint::VarInt::from_u8(2),
                }
            );
            assert!(rx_a.recv().await.is_none());
            assert!(rx_b.recv().await.is_none());
        }
        .primary()
        .spawn();
    });
}

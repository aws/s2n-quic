// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::{
    crypto::{self, open::Application as _, seal::Application as _},
    path::secret::receiver,
};
use s2n_codec::DecoderBufferMut;
use std::{io, net::UdpSocket, sync::Barrier, time::Duration};

const HEADER: &[u8] = b"authenticated initial header";
const PAYLOAD: &[u8] = b"initial payload";

struct Fixture {
    client: Map,
    server: Map,
    client_socket: UdpSocket,
    server_addr: SocketAddr,
    id: Id,
}

impl Fixture {
    fn new() -> Self {
        let new_map = || {
            let map = Map::new(
                stateless_reset::Signer::random(),
                10,
                false,
                time::NoopClock,
                crate::testing::NoopSubscriber {},
            );
            map.test_stop_cleaner();
            map
        };
        let client_socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        client_socket
            .set_read_timeout(Some(Duration::from_millis(20)))
            .unwrap();
        let client_addr = client_socket.local_addr().unwrap();
        let server_addr = "127.0.0.1:4567".parse().unwrap();
        let client = new_map();
        let server = new_map();
        let id = client.test_insert_pair(client_addr, None, &server, server_addr, None);
        Self {
            client,
            server,
            client_socket,
            server_addr,
            id,
        }
    }

    fn packet(&self) -> (Credentials, Vec<u8>) {
        let peer = self.client.get_tracked(self.server_addr).unwrap();
        let (keys, _) = peer.pair(&TransportFeatures::UDP).unwrap();
        let mut ciphertext = vec![0; PAYLOAD.len() + keys.application.sealer.tag_len()];
        ciphertext[..PAYLOAD.len()].copy_from_slice(PAYLOAD);
        keys.application
            .sealer
            .encrypt(0, HEADER, None, &mut ciphertext);
        (keys.credentials, ciphertext)
    }

    fn handoff(&self, credentials: &Credentials) -> Option<Bidirectional> {
        let mut control = vec![];
        let keys = self
            .server
            .secret_for_handoff(credentials, None, &mut control)
            .map(|(_, _, keys, _, _)| keys);
        assert!(control.is_empty());
        keys
    }

    fn assert_no_control(&self) {
        let mut buffer = [0; control::MAX_PACKET_SIZE];
        let error = self.client_socket.recv_from(&mut buffer).unwrap_err();
        assert!(
            matches!(
                error.kind(),
                io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
            ),
            "{error:?}"
        );
    }

    fn receive_control(&self) -> Vec<u8> {
        let mut buffer = [0; control::MAX_PACKET_SIZE];
        let (length, _) = self.client_socket.recv_from(&mut buffer).unwrap();
        buffer[..length].to_vec()
    }
}

fn decrypt(keys: &Bidirectional, ciphertext: &[u8]) -> crypto::open::Result {
    let (payload, tag) = ciphertext.split_at(PAYLOAD.len());
    let mut plaintext = vec![0; PAYLOAD.len()];
    keys.application.opener.decrypt(
        crypto::KeyPhase::Zero,
        0,
        HEADER,
        payload,
        tag,
        crypto::UninitSlice::new(&mut plaintext),
    )
}

#[test]
fn failed_authentication_does_not_consume_handoff() {
    let fixture = Fixture::new();
    let (credentials, ciphertext) = fixture.packet();
    let keys = fixture.handoff(&credentials).unwrap();
    let mut invalid = ciphertext.clone();
    *invalid.last_mut().unwrap() ^= 1;
    assert_eq!(
        decrypt(&keys, &invalid),
        Err(crypto::open::Error::InvalidTag)
    );
    fixture.assert_no_control();

    let keys = fixture.handoff(&credentials).unwrap();
    assert_eq!(decrypt(&keys, &ciphertext), Ok(()));
    assert!(fixture.handoff(&credentials).is_none());
    fixture.assert_no_control();
}

#[test]
fn concurrent_handoffs_have_one_authenticated_winner() {
    let fixture = Fixture::new();
    let (credentials, ciphertext) = fixture.packet();
    let contenders: Vec<_> = (0..8)
        .map(|_| fixture.handoff(&credentials).unwrap())
        .collect();
    let barrier = Barrier::new(contenders.len());

    let results = std::thread::scope(|scope| {
        let threads: Vec<_> = contenders
            .into_iter()
            .map(|keys| {
                let barrier = &barrier;
                let ciphertext = &ciphertext;
                scope.spawn(move || {
                    barrier.wait();
                    decrypt(&keys, ciphertext)
                })
            })
            .collect();
        threads
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .collect::<Vec<_>>()
    });

    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert!(results.iter().all(|result| {
        matches!(
            result,
            Ok(()) | Err(crypto::open::Error::ReplayDefinitelyDetected)
        )
    }));
    assert!(fixture.handoff(&credentials).is_none());
    fixture.assert_no_control();
}

#[test]
fn handoff_that_becomes_too_old_is_silent() {
    let fixture = Fixture::new();
    let (old_credentials, old_ciphertext) = fixture.packet();
    let old_candidate = fixture.handoff(&old_credentials).unwrap();

    let entry = fixture
        .client
        .store
        .get_by_id_untracked(&fixture.id)
        .unwrap();
    entry.sender().update_for_stale_key(2_000u16.into());
    let (new_credentials, new_ciphertext) = fixture.packet();
    let new_candidate = fixture.handoff(&new_credentials).unwrap();
    assert_eq!(decrypt(&new_candidate, &new_ciphertext), Ok(()));

    assert!(matches!(
        decrypt(&old_candidate, &old_ciphertext),
        Err(crypto::open::Error::ReplayPotentiallyDetected { .. })
    ));
    assert!(fixture.handoff(&old_credentials).is_none());
    fixture.assert_no_control();
}

#[test]
fn handoff_rejects_maximum_key_without_control_response() {
    let fixture = Fixture::new();
    assert!(fixture
        .handoff(&Credentials {
            id: fixture.id,
            key_id: VarInt::MAX,
        })
        .is_none());
    fixture.assert_no_control();
}

#[test]
fn missing_handoff_secret_requests_unknown_secret_recovery() {
    let fixture = Fixture::new();
    let mut response = vec![];
    assert!(fixture
        .server
        .secret_for_handoff(
            &Credentials {
                id: Id::default(),
                key_id: VarInt::ZERO,
            },
            None,
            &mut response,
        )
        .is_none());
    let (packet, remaining) =
        control::Packet::decode(DecoderBufferMut::new(&mut response)).unwrap();
    assert!(matches!(packet, control::Packet::UnknownPathSecret(_)));
    assert!(remaining.is_empty());
    fixture.assert_no_control();
}

#[test]
fn ordinary_dedup_still_sends_replay_and_stale_responses() {
    let fixture = Fixture::new();
    let (credentials, ciphertext) = fixture.packet();
    let mut control_out = vec![];
    let (_, _, first, _, _) = fixture
        .server
        .secret_for_credentials(
            &credentials,
            None,
            &TransportFeatures::UDP,
            &mut control_out,
        )
        .unwrap();
    assert_eq!(decrypt(&first, &ciphertext), Ok(()));

    let (_, _, duplicate, _, _) = fixture
        .server
        .secret_for_credentials(
            &credentials,
            None,
            &TransportFeatures::UDP,
            &mut control_out,
        )
        .unwrap();
    assert_eq!(
        decrypt(&duplicate, &ciphertext),
        Err(crypto::open::Error::ReplayDefinitelyDetected)
    );
    let mut response = fixture.receive_control();
    let (packet, _) = control::Packet::decode(DecoderBufferMut::new(&mut response)).unwrap();
    assert!(matches!(packet, control::Packet::ReplayDetected(_)));

    let server_entry = fixture
        .server
        .store
        .get_by_id_untracked(&fixture.id)
        .unwrap();
    server_entry
        .receiver()
        .post_authentication(&Credentials {
            id: fixture.id,
            key_id: 2_000u16.into(),
        })
        .unwrap();
    let (_, _, old, _, _) = fixture
        .server
        .secret_for_credentials(
            &credentials,
            None,
            &TransportFeatures::UDP,
            &mut control_out,
        )
        .unwrap();
    assert!(matches!(
        decrypt(&old, &ciphertext),
        Err(crypto::open::Error::ReplayPotentiallyDetected { .. })
    ));
    let mut response = fixture.receive_control();
    let (packet, _) = control::Packet::decode(DecoderBufferMut::new(&mut response)).unwrap();
    assert!(matches!(packet, control::Packet::StaleKey(_)));
}

#[test]
fn handoff_precheck_does_not_claim_unseen_keys() {
    let state = receiver::State::new();
    let credentials = Credentials {
        id: Id::default(),
        key_id: 10u8.into(),
    };
    assert_eq!(state.pre_authentication_for_handoff(&credentials), Ok(()));
    assert_eq!(state.pre_authentication_for_handoff(&credentials), Ok(()));
    assert_eq!(state.minimum_unseen_key_id(), VarInt::ZERO);
    assert_eq!(state.post_authentication(&credentials), Ok(()));
    assert_eq!(
        state.pre_authentication_for_handoff(&credentials),
        Err(receiver::Error::AlreadyExists)
    );

    let earlier = Credentials {
        key_id: 9u8.into(),
        ..credentials
    };
    assert_eq!(state.pre_authentication_for_handoff(&earlier), Ok(()));
    assert_eq!(state.post_authentication(&earlier), Ok(()));
}

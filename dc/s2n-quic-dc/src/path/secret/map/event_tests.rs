// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    event,
    packet::{secret_control as control, WireVersion},
    path::secret::{stateless_reset, Map},
};
use s2n_codec::{DecoderBufferMut, EncoderBuffer};
use s2n_quic_core::time::NoopClock;
use std::sync::Arc;

type Subscriber = (
    event::testing::Subscriber,
    event::metrics::aggregate::Subscriber<(
        event::metrics::aggregate::probe::Registry,
        event::metrics::aggregate::probe::dynamic::Registry,
    )>,
);

#[track_caller]
fn sub() -> Arc<Subscriber> {
    crate::testing::init_tracing();

    Arc::new((event::testing::Subscriber::snapshot(), Default::default()))
}

#[track_caller]
fn map(capacity: usize) -> Map {
    map_sub(capacity, sub())
}

fn map_sub(capacity: usize, sub: Arc<Subscriber>) -> Map {
    let signer = stateless_reset::Signer::random();

    let map = Map::new(signer, capacity, NoopClock, sub);

    // sleep so the cleaner has time to emit events
    std::thread::sleep(core::time::Duration::from_millis(100));

    map
}

#[test]
fn init_uninit() {
    let _ = map(10);
}

#[test]
fn insert_one() {
    let map = map(10);
    map.test_insert("127.0.0.1:4567".parse().unwrap());
}

#[test]
fn control_packets() {
    let sub = sub();
    let client = map_sub(10, sub.clone());
    let server = map_sub(10, sub);

    let client_addr = "127.0.0.1:1234".parse().unwrap();
    let server_addr = "127.0.0.1:5678".parse().unwrap();

    let id = client.test_insert_pair(client_addr, &server, server_addr);

    let mut out = [0; 128];

    macro_rules! packet {
        ($expr:expr, $crypto:expr) => {
            let v = $expr;
            let len = v.encode(EncoderBuffer::new(&mut out), $crypto);
            let buf = &mut out[..len];
            let (pkt, _) = control::Packet::decode(DecoderBufferMut::new(buf)).unwrap();
            client.handle_control_packet(&pkt, &server_addr);
        };
    }

    let server_entry = server.store.get_by_id_untracked(&id).unwrap().clone();
    let client_entry = client.store.get_by_id_untracked(&id).unwrap().clone();

    let fake_secret =
        crate::path::secret::seal::control::Secret::new(&[0; 32], &aws_lc_rs::hmac::HMAC_SHA256);
    let fake_srt = [0; 16];
    let fake_id = [0; 16].into();

    // make sure control packets can't be reflected back
    let client_secret = client_entry.control_sealer();
    let client_srt = client_entry.sender().stateless_reset;

    let real_secret = server_entry.control_sealer();
    let real_id = *server_entry.id();
    let real_srt = server_entry.sender().stateless_reset;

    // try to send a series of packet types from the server to the client
    let attempts = [
        (&fake_secret, fake_srt, fake_id),
        (&fake_secret, fake_srt, real_id),
        (&client_secret, client_srt, real_id),
        (&real_secret, real_srt, real_id),
    ];

    for (secret, stateless_reset, credential_id) in attempts {
        packet!(
            control::UnknownPathSecret {
                wire_version: WireVersion::ZERO,
                credential_id,
            },
            &stateless_reset
        );

        packet!(
            control::StaleKey {
                wire_version: WireVersion::ZERO,
                credential_id,
                min_key_id: 123u16.into(),
            },
            secret
        );

        packet!(
            control::ReplayDetected {
                wire_version: WireVersion::ZERO,
                credential_id,
                rejected_key_id: 123u16.into(),
            },
            secret
        );
    }
}

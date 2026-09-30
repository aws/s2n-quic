// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{stateless_reset, ApplicationData, ApplicationDataError, Map};
use s2n_quic_core::time::NoopClock;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

fn test_map() -> Map {
    let map = Map::new(
        stateless_reset::Signer::random(),
        10,
        false,
        NoopClock,
        crate::event::tracing::Subscriber::default(),
    );
    // These tests don't exercise the background cleaner; stop it to keep them deterministic.
    map.test_stop_cleaner();
    map
}

#[test]
fn serialize_application_data_returns_ok_none_when_unregistered() {
    let map = test_map();

    let data: ApplicationData = Arc::new(42u64);

    assert!(matches!(map.serialize_application_data(&data), Ok(None)));
}

#[test]
fn serialize_application_data_invokes_registered_callback() {
    let map = test_map();

    let calls = Arc::new(AtomicUsize::new(0));
    let calls_in_cb = calls.clone();
    map.register_application_data_serializer(Box::new(move |data| {
        calls_in_cb.fetch_add(1, Ordering::Relaxed);
        let value = data
            .downcast_ref::<u64>()
            .copied()
            .expect("the test always registers a u64");
        Ok(Some(value.to_be_bytes().to_vec()))
    }));

    let data: ApplicationData = Arc::new(42u64);
    let blob = map
        .serialize_application_data(&data)
        .expect("a registered callback that succeeds yields Ok");

    assert_eq!(calls.load(Ordering::Relaxed), 1);
    assert_eq!(blob, Some(42u64.to_be_bytes().to_vec()));
}

/// The map passes a callback error through unchanged: it has no publisher of its own, so the
/// caller (the forwarding worker) owns both the event and the log line.
#[test]
fn serialize_application_data_returns_err_from_callback() {
    let map = test_map();

    map.register_application_data_serializer(Box::new(|_data| {
        Err(ApplicationDataError {
            msg: "serialization failed",
            inner: "serialization failed".into(),
        })
    }));

    let data: ApplicationData = Arc::new(42u64);

    let err = map
        .serialize_application_data(&data)
        .expect_err("a failing callback yields Err");
    assert_eq!(err.msg, "serialization failed");
}

/// Key ID exhaustion is terminal for a path secret, so the only way to recover is to replace it.
/// Every path that derives a *sending* key asks for a background re-handshake when it hits that.
#[test]
fn key_id_exhaustion_requests_rehandshake() {
    use crate::stream::TransportFeatures;
    use s2n_quic_core::varint::VarInt;
    use std::net::SocketAddr;

    let map = test_map();
    let peer_addr: SocketAddr = "127.0.0.1:1234".parse().expect("valid address literal");
    map.test_insert(peer_addr);

    let requests = Arc::new(AtomicUsize::new(0));
    let requests_in_cb = requests.clone();
    map.register_request_handshake(Box::new(move |_peer, _reason| {
        requests_in_cb.fetch_add(1, Ordering::Relaxed);
        None
    }));

    let peer = map.get_tracked(peer_addr).expect("just inserted");

    // A healthy entry hands out key IDs without asking for anything.
    assert!(peer.pair(&TransportFeatures::UDP).is_some());
    assert!(peer.seal_once().is_some());
    assert_eq!(requests.load(Ordering::Relaxed), 0);

    // Drive the counter to the top of the range, exactly as an authenticated StaleKey packet from
    // the peer would.
    let entry = map
        .store
        .get_by_addr_untracked(&peer_addr)
        .expect("just inserted");
    entry.sender().update_for_stale_key(VarInt::MAX);

    // Each of the three paths that derive a sending key now fails and requests a re-handshake.
    assert!(peer.pair(&TransportFeatures::UDP).is_none());
    assert_eq!(requests.load(Ordering::Relaxed), 1);

    assert!(peer.seal_once().is_none());
    assert_eq!(requests.load(Ordering::Relaxed), 2);

    assert!(map.seal_once_id(*entry.id()).is_none());
    assert_eq!(requests.load(Ordering::Relaxed), 3);
}

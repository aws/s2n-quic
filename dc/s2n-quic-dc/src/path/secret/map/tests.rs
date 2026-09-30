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

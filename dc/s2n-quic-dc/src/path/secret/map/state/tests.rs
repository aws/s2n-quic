// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::{
    event::{testing, tracing},
    path::secret::{schedule, sender},
};
use s2n_quic_core::{dc, time::NoopClock as Clock};
use std::{
    collections::HashSet,
    fmt,
    net::{Ipv4Addr, SocketAddrV4},
};

fn fake_entry(generation: u64) -> Arc<Entry> {
    Entry::builder((Ipv4Addr::LOCALHOST, 1).into())
        .generation(generation)
        .build()
}

#[test]
fn cleans_after_delay() {
    let signer = stateless_reset::Signer::new(b"secret");
    let map = State::builder()
        .with_signer(signer)
        .with_capacity(50)
        .with_clock(Clock)
        .with_subscriber(tracing::Subscriber::default())
        .build_state()
        .unwrap();

    // Stop background processing. We expect to manually invoke clean, and a background worker
    // might interfere with our state.
    map.cleaner.stop();

    let first = fake_entry(1);
    let second = fake_entry(2);
    let third = fake_entry(3);
    map.test_insert(first.clone());
    map.test_insert(second.clone());

    assert!(map.ids.contains_key(first.id()));
    assert!(map.ids.contains_key(second.id()));

    map.cleaner.clean(&map, 1);
    map.cleaner.clean(&map, 1);

    map.test_insert(third.clone());

    assert!(!map.ids.contains_key(first.id()));
    assert!(map.ids.contains_key(second.id()));
    assert!(map.ids.contains_key(third.id()));
}

#[test]
fn thread_shutdown() {
    let signer = stateless_reset::Signer::new(b"secret");
    let map = State::builder()
        .with_signer(signer)
        .with_capacity(10)
        .with_clock(Clock)
        .with_subscriber((
            tracing::Subscriber::default(),
            testing::Subscriber::snapshot(),
        ))
        .build_state()
        .unwrap();
    let state = Arc::downgrade(&map);
    drop(map);

    let iterations = 10;
    let max_time = core::time::Duration::from_secs(2);

    for _ in 0..iterations {
        // Nothing is holding on to the state, so the thread should shutdown (mpsc disconnects or on
        // next loop around if that fails for some reason).
        if state.strong_count() == 0 {
            return;
        }
        std::thread::sleep(max_time / iterations);
    }

    panic!("thread did not shut down after {max_time:?}");
}

#[derive(Debug, Default)]
struct Model {
    invariants: HashSet<Invariant>,
}

#[derive(bolero::TypeGenerator, Debug, Copy, Clone)]
enum Operation {
    Insert { ip: u8, path_secret_id: TestId },
    AdvanceTime,
    ReceiveUnknown { path_secret_id: TestId },
}

#[derive(bolero::TypeGenerator, PartialEq, Eq, Hash, Copy, Clone)]
struct TestId(u8);

impl fmt::Debug for TestId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("TestId")
            .field(&self.0)
            .field(&self.id())
            .finish()
    }
}

impl TestId {
    fn secret(self) -> schedule::Secret {
        let mut export_secret = [0; 32];
        export_secret[0] = self.0;
        schedule::Secret::new(
            schedule::Ciphersuite::AES_GCM_128_SHA256,
            dc::SUPPORTED_VERSIONS[0],
            s2n_quic_core::endpoint::Type::Client,
            &export_secret,
        )
    }

    fn id(self) -> Id {
        *self.secret().id()
    }
}

#[derive(Debug, PartialEq, Eq, Hash, Copy, Clone)]
enum Invariant {
    ContainsIp(SocketAddr),
    ContainsId(Id),
    IdRemoved(Id),
}

impl Model {
    fn perform(&mut self, operation: Operation, state: &State<Clock, tracing::Subscriber>) {
        match operation {
            Operation::Insert { ip, path_secret_id } => {
                let ip = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::from([0, 0, 0, ip]), 0));
                let secret = path_secret_id.secret();
                let id = *secret.id();

                // In reality, sender.stateless_reset = peer.signer.sign(peer.local_id)
                // = peer.signer.sign(our.peer_id). With identical signers this is
                // signer.sign(id.for_peer()).
                let stateless_reset = state.signer().sign(&id.for_peer());
                state.test_insert(Arc::new(Entry::new(
                    ip,
                    secret,
                    sender::State::new(stateless_reset),
                    receiver::State::new(),
                    dc::testing::TEST_APPLICATION_PARAMS,
                    crate::time::DefaultClock::default().now().into(),
                    None,
                )));

                self.invariants.insert(Invariant::ContainsIp(ip));
                self.invariants.insert(Invariant::ContainsId(id));
            }
            Operation::AdvanceTime => {
                let mut invalidated = Vec::new();
                self.invariants.retain(|invariant| {
                    if let Invariant::ContainsId(id) = invariant {
                        if state
                            .get_by_id_untracked(id)
                            .is_none_or(|v| v.retired_at().is_some())
                        {
                            invalidated.push(*id);
                            return false;
                        }
                    }

                    true
                });
                for id in invalidated {
                    assert!(self.invariants.insert(Invariant::IdRemoved(id)), "{id:?}");
                }

                // Evict all stale records *now*.
                state.cleaner.clean(state, 0);
            }
            Operation::ReceiveUnknown { path_secret_id } => {
                let local_id = path_secret_id.id();
                // The control packet contains our peer_id (what we sent on the wire).
                let wire_id = local_id.for_peer();
                // This is signing with the "wrong" signer, but currently all of the signers used
                // in this test are keyed the same way so it doesn't matter.
                let stateless_reset = state.signer.sign(&wire_id);
                let packet =
                    crate::packet::secret_control::unknown_path_secret::Packet::new_for_test(
                        wire_id,
                        &stateless_reset,
                    );

                state
                    .handle_unknown_path_secret_packet(&packet, &"127.0.0.1:1234".parse().unwrap());

                if state.should_evict_on_unknown_path_secret()
                    && self.invariants.contains(&Invariant::ContainsId(local_id))
                {
                    self.invariants.retain(|invariant| {
                        if let Invariant::ContainsId(prev_id) = invariant {
                            if prev_id == &local_id {
                                return false;
                            }
                        }

                        true
                    });

                    self.invariants.insert(Invariant::IdRemoved(local_id));
                }
            }
        }
    }

    fn check_invariants(&self, state: &State<Clock, tracing::Subscriber>) {
        for invariant in self.invariants.iter() {
            // We avoid assertions for contains() if we're running the small capacity test, since
            // they are likely broken -- we semi-randomly evict peers in that case.
            match invariant {
                Invariant::ContainsIp(ip) => {
                    if state.max_capacity != 5 {
                        assert!(
                            state.client_peers.contains_key(ip)
                                || state.server_peers.contains_key(ip),
                            "{ip:?}"
                        );
                    }
                }
                Invariant::ContainsId(id) => {
                    if state.max_capacity != 5 {
                        assert!(state.ids.contains_key(id), "{id:?}");
                    }
                }
                Invariant::IdRemoved(id) => {
                    assert!(!state.ids.contains_key(id), "{:?}", state.ids.get(*id));
                }
            }
        }

        // All entries in the peer set should also be in the `ids` set (which is actively garbage
        // collected).
        // FIXME: this requires a clean() call which may have not happened yet.
        // state.peers.iter(|_, entry| {
        //     assert!(
        //         state.ids.contains_key(entry.secret.id()),
        //         "{:?} not present in IDs",
        //         entry.secret.id()
        //     );
        // });
    }
}

fn has_duplicate_pids(ops: &[Operation]) -> bool {
    let mut ids = HashSet::new();
    for op in ops.iter() {
        match op {
            Operation::Insert {
                ip: _,
                path_secret_id,
            } => {
                if !ids.insert(path_secret_id) {
                    return true;
                }
            }
            Operation::AdvanceTime => {}
            Operation::ReceiveUnknown { path_secret_id: _ } => {
                // no-op, we're fine receiving unknown pids.
            }
        }
    }

    false
}

fn check_invariants_inner(should_evict_on_unknown_path_secret: bool) {
    bolero::check!()
        .with_type::<Vec<Operation>>()
        .with_iterations(10_000)
        .for_each(|input: &Vec<Operation>| {
            if has_duplicate_pids(input) {
                // Ignore this attempt.
                return;
            }

            let mut model = Model::default();
            let signer = stateless_reset::Signer::new(b"secret");
            let mut map = State::builder()
                .with_signer(signer)
                .with_capacity(10_000)
                .with_evict_on_unknown_path_secret(should_evict_on_unknown_path_secret)
                .with_clock(Clock)
                .with_subscriber(tracing::Subscriber::default())
                .build_state()
                .unwrap();

            // Avoid background work interfering with testing.
            map.cleaner.stop();

            Arc::<State<Clock, tracing::Subscriber>>::get_mut(&mut map)
                .unwrap()
                .set_max_capacity(5);

            model.check_invariants(&map);

            for op in input {
                model.perform(*op, &map);
                model.check_invariants(&map);
            }
        })
}

#[test]
fn check_invariants() {
    check_invariants_inner(false);
}

#[test]
fn check_invariants_evict_unknown_pid() {
    check_invariants_inner(true);
}

#[test]
#[ignore = "fixed size maps currently break overflow assumptions, too small bucket size"]
fn check_invariants_no_overflow() {
    bolero::check!()
        .with_type::<Vec<Operation>>()
        .with_iterations(10_000)
        .for_each(|input: &Vec<Operation>| {
            if has_duplicate_pids(input) {
                // Ignore this attempt.
                return;
            }

            let mut model = Model::default();
            let signer = stateless_reset::Signer::new(b"secret");
            let map = State::builder()
                .with_signer(signer)
                .with_capacity(10_000)
                .with_clock(Clock)
                .with_subscriber(tracing::Subscriber::default())
                .build_state()
                .unwrap();

            // Avoid background work interfering with testing.
            map.cleaner.stop();

            model.check_invariants(&map);

            for op in input {
                model.perform(*op, &map);
                model.check_invariants(&map);
            }
        })
}

// Unfortunately actually checking memory usage is probably too flaky, but if this did end up
// growing at all on a per-entry basis we'd quickly overflow available memory (this is 153GB of
// peer entries at minimum).
//
// For now ignored but run locally to confirm this works.
#[test]
#[ignore = "memory growth takes a long time to run"]
fn no_memory_growth() {
    let signer = stateless_reset::Signer::new(b"secret");
    let map = State::builder()
        .with_signer(signer)
        .with_capacity(100_000)
        .with_clock(Clock)
        .with_subscriber(tracing::Subscriber::default())
        .build_state()
        .unwrap();
    map.cleaner.stop();

    for idx in 0..500_000 {
        // FIXME: this ends up 2**16 peers in the `peers` map
        map.test_insert(fake_entry(idx as u64));
    }
}

#[test]
fn unknown_path_secret_evicts() {
    let signer = stateless_reset::Signer::new(b"secret");
    let map = State::builder()
        .with_signer(signer)
        .with_capacity(5)
        .with_evict_on_unknown_path_secret(true)
        .with_clock(Clock)
        .with_subscriber(tracing::Subscriber::default())
        .build_state()
        .unwrap();

    let entry = fake_entry(0);
    map.test_insert(entry.clone());

    // Simulate receiving UnknownPathSecret from peer. The credential_id on the wire
    // is our peer_id (what we sent them), and the tag is the sender's stateless_reset.
    let wire_id = entry.id().for_peer();
    let packet = crate::packet::secret_control::unknown_path_secret::Packet::new_for_test(
        wire_id,
        &entry.sender().stateless_reset,
    );

    assert!(map.ids.contains_key(entry.id()), "{:?}", map.ids);
    assert!(map.client_peers.contains_key(entry.peer()));

    map.handle_unknown_path_secret_packet(&packet, &"127.0.0.1:1234".parse().unwrap());

    assert!(!map.ids.contains_key(entry.id()), "{:?}", map.ids);
    assert!(!map.client_peers.contains_key(entry.peer()));
}

// ─── application_data / ApplicationDataRequest flow ───────────────────────────

/// Minimal `TlsSession` stub for exercising the `make_application_data`
/// callback without driving a full TLS handshake. The callback under test only
/// consumes `request.peer_info`, so these methods are never meaningfully called.
struct StubTlsSession;

impl s2n_quic_core::crypto::tls::TlsSession for StubTlsSession {
    fn tls_exporter(
        &self,
        _label: &[u8],
        _context: &[u8],
        _output: &mut [u8],
    ) -> Result<(), s2n_quic_core::crypto::tls::TlsExportError> {
        Ok(())
    }

    fn cipher_suite(&self) -> s2n_quic_core::crypto::tls::CipherSuite {
        s2n_quic_core::crypto::tls::CipherSuite::TLS_AES_128_GCM_SHA256
    }

    fn peer_cert_chain_der(&self) -> Result<Vec<Vec<u8>>, s2n_quic_core::crypto::tls::ChainError> {
        Ok(vec![])
    }
}

fn test_state() -> Arc<State<Clock, tracing::Subscriber>> {
    let map = State::builder()
        .with_signer(stateless_reset::Signer::new(b"secret"))
        .with_capacity(10)
        .with_clock(Clock)
        .with_subscriber(tracing::Subscriber::default())
        .build_state()
        .unwrap();
    map.cleaner.stop();
    map
}

/// With no `make_application_data` registered, the store yields `None`.
#[test]
fn application_data_none_without_callback() {
    let map = test_state();
    let request = super::super::ApplicationDataRequest {
        tls: &StubTlsSession,
        peer_info: None,
    };
    let data = Store::application_data(&*map, request).unwrap();
    assert!(data.is_none());
}

/// The registered callback receives the request's `peer_info` and its returned
/// `ApplicationData` is what the store yields. This is the hook Membrain uses to
/// compute negotiation at handshake time and stash the result on the `Entry`.
#[test]
fn application_data_callback_receives_peer_info_and_returns_data() {
    let map = test_state();

    // The callback echoes the peer_info length as its application data, so the
    // assertion proves it actually saw the bytes we passed in the request.
    Store::register_make_application_data(
        &*map,
        Box::new(|request: super::super::ApplicationDataRequest| {
            let len = request.peer_info.map_or(0, |b| b.len());
            let data: super::super::ApplicationData = Arc::new(len);
            Ok(Some(data))
        }),
    );

    let peer_info = bytes::Bytes::from_static(b"hello-peer-info");
    let request = super::super::ApplicationDataRequest {
        tls: &StubTlsSession,
        peer_info: Some(&peer_info),
    };
    let data = Store::application_data(&*map, request)
        .unwrap()
        .expect("callback returns Some");
    let observed_len = data.downcast::<usize>().expect("usize application data");
    assert_eq!(*observed_len, peer_info.len());
}

/// A callback that returns `Err` surfaces the error (the handshake path turns
/// this into an APPLICATION_ERROR, aborting the connection).
#[test]
fn application_data_callback_error_propagates() {
    let map = test_state();
    Store::register_make_application_data(
        &*map,
        Box::new(|_request: super::super::ApplicationDataRequest| {
            Err(super::super::ApplicationDataError {
                msg: "boom",
                inner: "negotiation failed".into(),
            })
        }),
    );

    let request = super::super::ApplicationDataRequest {
        tls: &StubTlsSession,
        peer_info: None,
    };
    let err = Store::application_data(&*map, request).unwrap_err();
    assert_eq!(err.msg, "boom");
}

/// The `Entry` carries the `application_data` it was constructed with — the
/// value the handshake path moves in (produced by the `make_application_data`
/// callback at handshake time).
#[test]
fn entry_carries_application_data_from_constructor() {
    let app_data: super::super::ApplicationData = Arc::new(7u32);

    let entry = Entry::builder((Ipv4Addr::LOCALHOST, 1).into())
        .application_data(Some(app_data))
        .build();

    let stored = entry
        .application_data()
        .as_ref()
        .expect("application data present");
    assert_eq!(*stored.clone().downcast::<u32>().unwrap(), 7);
}

/// An `Entry` built without negotiation carries no application data.
#[test]
fn entry_without_negotiation_has_no_application_data() {
    let entry = Entry::builder((Ipv4Addr::LOCALHOST, 1).into()).build();
    assert!(entry.application_data().is_none());
}

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

fn fake_entry(port: u16) -> Arc<Entry> {
    Entry::fake((Ipv4Addr::LOCALHOST, port).into(), None)
}

#[test]
fn cleans_after_delay() {
    let signer = stateless_reset::Signer::new(b"secret");
    let map = State::new(signer, 50, false, Clock, tracing::Subscriber::default());

    // Stop background processing. We expect to manually invoke clean, and a background worker
    // might interfere with our state.
    map.cleaner.stop();

    let first = fake_entry(1);
    let second = fake_entry(1);
    let third = fake_entry(1);
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
    let map = State::new(
        signer,
        10,
        false,
        Clock,
        (
            tracing::Subscriber::default(),
            testing::Subscriber::snapshot(),
        ),
    );
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

                let stateless_reset = state.signer().sign(&id);
                state.test_insert(Arc::new(Entry::new(
                    ip,
                    secret,
                    sender::State::new(stateless_reset),
                    receiver::State::new(),
                    dc::testing::TEST_APPLICATION_PARAMS,
                    dc::testing::TEST_REHANDSHAKE_PERIOD,
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
                let id = path_secret_id.id();
                // This is signing with the "wrong" signer, but currently all of the signers used
                // in this test are keyed the same way so it doesn't matter.
                let stateless_reset = state.signer.sign(&id);
                let packet =
                    crate::packet::secret_control::unknown_path_secret::Packet::new_for_test(
                        id,
                        &stateless_reset,
                    );

                if state.ups_eviction_policy() {
                    assert!(self.invariants.insert(Invariant::IdRemoved(id)), "{id:?}");
                }

                state
                    .handle_unknown_path_secret_packet(&packet, &"127.0.0.1:1234".parse().unwrap());
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
                        assert!(state.peers.contains_key(ip), "{ip:?}");
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

fn has_duplicate_pids(ops: &[Operation], evict_on_ups: bool) -> bool {
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
            Operation::ReceiveUnknown { path_secret_id } => {
                if evict_on_ups {
                    if !ids.insert(path_secret_id) {
                        return true;
                    }
                }
            }
        }
    }

    false
}

fn check_invariants_inner(ups_eviction_policy: bool) {
    bolero::check!()
        .with_type::<Vec<Operation>>()
        .with_iterations(10_000)
        .for_each(|input: &Vec<Operation>| {
            if has_duplicate_pids(input, ups_eviction_policy) {
                // Ignore this attempt.
                return;
            }

            let mut model = Model::default();
            let signer = stateless_reset::Signer::new(b"secret");
            let mut map = State::new(
                signer,
                10_000,
                ups_eviction_policy,
                Clock,
                tracing::Subscriber::default(),
            );

            // Avoid background work interfering with testing.
            map.cleaner.stop();

            Arc::get_mut(&mut map).unwrap().set_max_capacity(5);

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
            if has_duplicate_pids(input, false) {
                // Ignore this attempt.
                return;
            }

            let mut model = Model::default();
            let signer = stateless_reset::Signer::new(b"secret");
            let map = State::new(signer, 10_000, false, Clock, tracing::Subscriber::default());

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
    let map = State::new(
        signer,
        100_000,
        false,
        Clock,
        tracing::Subscriber::default(),
    );
    map.cleaner.stop();

    for idx in 0..500_000 {
        // FIXME: this ends up 2**16 peers in the `peers` map
        map.test_insert(fake_entry(idx as u16));
    }
}

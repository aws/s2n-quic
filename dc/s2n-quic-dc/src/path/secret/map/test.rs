// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{receiver, sender};
use std::{
    collections::HashSet,
    net::{Ipv4Addr, SocketAddrV4},
};

use super::*;

const VERSION: dc::Version = dc::SUPPORTED_VERSIONS[0];

fn fake_entry(peer: u16) -> Arc<Entry> {
    let mut secret = [0; 32];
    aws_lc_rs::rand::fill(&mut secret).unwrap();
    Arc::new(Entry::new(
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, peer)),
        schedule::Secret::new(
            schedule::Ciphersuite::AES_GCM_128_SHA256,
            VERSION,
            s2n_quic_core::endpoint::Type::Client,
            &secret,
        ),
        sender::State::new([0; control::TAG_LEN]),
        receiver::State::without_shared(),
        dc::testing::TEST_APPLICATION_PARAMS,
        dc::testing::TEST_REHANDSHAKE_PERIOD,
    ))
}

#[test]
fn cleans_after_delay() {
    let signer = stateless_reset::Signer::new(b"secret");
    let map = Map::new(signer);

    // Stop background processing. We expect to manually invoke clean, and a background worker
    // might interfere with our state.
    map.state.cleaner.stop();

    let first = fake_entry(1);
    let second = fake_entry(1);
    let third = fake_entry(1);
    map.insert(first.clone());
    map.insert(second.clone());

    assert!(map.state.ids.contains_key(first.secret.id()));
    assert!(map.state.ids.contains_key(second.secret.id()));

    map.state.cleaner.clean(&map.state, 1);
    map.state.cleaner.clean(&map.state, 1);

    map.insert(third.clone());

    assert!(!map.state.ids.contains_key(first.secret.id()));
    assert!(map.state.ids.contains_key(second.secret.id()));
    assert!(map.state.ids.contains_key(third.secret.id()));
}

#[test]
fn thread_shutdown() {
    let signer = stateless_reset::Signer::new(b"secret");
    let map = Map::new(signer);
    let state = Arc::downgrade(&map.state);
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
            VERSION,
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
    fn perform(&mut self, operation: Operation, state: &Map) {
        match operation {
            Operation::Insert { ip, path_secret_id } => {
                let ip = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::from([0, 0, 0, ip]), 0));
                let secret = path_secret_id.secret();
                let id = *secret.id();

                let stateless_reset = state.state.signer.sign(&id);
                state.insert(Arc::new(Entry::new(
                    ip,
                    secret,
                    sender::State::new(stateless_reset),
                    state.state.receiver_shared.clone().new_receiver(),
                    dc::testing::TEST_APPLICATION_PARAMS,
                    dc::testing::TEST_REHANDSHAKE_PERIOD,
                )));

                self.invariants.insert(Invariant::ContainsIp(ip));
                self.invariants.insert(Invariant::ContainsId(id));
            }
            Operation::AdvanceTime => {
                let mut invalidated = Vec::new();
                self.invariants.retain(|invariant| {
                    if let Invariant::ContainsId(id) = invariant {
                        if state
                            .state
                            .ids
                            .get_by_key(id)
                            .map_or(true, |v| v.retired.retired())
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
                state.state.cleaner.clean(&state.state, 0);
            }
            Operation::ReceiveUnknown { path_secret_id } => {
                let id = path_secret_id.id();
                // This is signing with the "wrong" signer, but currently all of the signers used
                // in this test are keyed the same way so it doesn't matter.
                let stateless_reset = state.state.signer.sign(&id);
                let packet =
                    crate::packet::secret_control::unknown_path_secret::Packet::new_for_test(
                        id,
                        &stateless_reset,
                    );
                state.handle_unknown_secret_packet(&packet);

                // ReceiveUnknown does not cause any action with respect to our invariants, no
                // updates required.
            }
        }
    }

    fn check_invariants(&self, state: &State) {
        for invariant in self.invariants.iter() {
            // We avoid assertions for contains() if we're running the small capacity test, since
            // they are likely broken -- we semi-randomly evict peers in that case.
            match invariant {
                Invariant::ContainsIp(ip) => {
                    if state.max_capacity != 5 {
                        assert!(state.peers.contains_key(ip), "{:?}", ip);
                    }
                }
                Invariant::ContainsId(id) => {
                    if state.max_capacity != 5 {
                        assert!(state.ids.contains_key(id), "{:?}", id);
                    }
                }
                Invariant::IdRemoved(id) => {
                    assert!(
                        !state.ids.contains_key(id),
                        "{:?}",
                        state.ids.get_by_key(id)
                    );
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

#[test]
fn check_invariants() {
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
            let mut map = Map::new(signer);

            // Avoid background work interfering with testing.
            map.state.cleaner.stop();

            Arc::get_mut(&mut map.state).unwrap().set_max_capacity(5);

            model.check_invariants(&map.state);

            for op in input {
                model.perform(*op, &map);
                model.check_invariants(&map.state);
            }
        })
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
            let map = Map::new(signer);

            // Avoid background work interfering with testing.
            map.state.cleaner.stop();

            model.check_invariants(&map.state);

            for op in input {
                model.perform(*op, &map);
                model.check_invariants(&map.state);
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
    let map = Map::new(signer);
    for idx in 0..500_000_000 {
        map.insert(fake_entry(idx as u16));
    }
}

#[test]
#[cfg(all(target_pointer_width = "64", target_os = "linux"))]
fn entry_size() {
    // This gates to running only on specific GHA to reduce false positives.
    if std::env::var("S2N_QUIC_RUN_VERSION_SPECIFIC_TESTS").is_ok() {
        assert_eq!(fake_entry(0).size(), 270);
    }
}

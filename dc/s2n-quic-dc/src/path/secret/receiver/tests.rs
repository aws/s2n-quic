// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use bolero::check;
use rand::{seq::SliceRandom, Rng, SeedableRng};
use std::collections::{binary_heap::PeekMut, BinaryHeap, HashSet};

#[test]
fn check() {
    check!().with_type::<Vec<KeyId>>().for_each(|ops| {
        let mut oracle = std::collections::HashSet::new();
        let subject = State::with_shared(Shared::new());
        let id = Id::from([0; 16]);
        for op in ops {
            let expected = oracle.insert(*op);
            let actual = subject
                .post_authentication(&Credentials { id, key_id: *op })
                .is_ok();
            // If we did expect this to be a new value, it may have already been marked as
            // "seen" by the set. However, we should never return a false OK (i.e., claim that
            // the value was not seen when it actually was).
            if !expected {
                assert!(!actual);
            }
        }
    });
}

#[test]
fn check_ordered() {
    check!().with_type::<Vec<KeyId>>().for_each(|ops| {
        let mut ops = ops.clone();
        ops.sort();
        let mut oracle = std::collections::HashSet::new();
        let subject = State::with_shared(Shared::new());
        let id = Id::from([0; 16]);
        for op in ops {
            let expected = oracle.insert(op);
            let actual = subject
                .post_authentication(&Credentials { id, key_id: op })
                .is_ok();
            assert_eq!(actual, expected);
        }
    });
}

#[test]
fn check_u16() {
    check!().with_type::<Vec<u16>>().for_each(|ops| {
        let mut oracle = std::collections::HashSet::new();
        let subject = State::with_shared(Shared::new());
        for op in ops {
            let op = KeyId::new(*op as u64).unwrap();
            let expected = oracle.insert(op);
            let id = Id::from([0; 16]);
            let actual = subject
                .post_authentication(&Credentials { id, key_id: op })
                .is_ok();
            // If we did expect this to be a new value, it may have already been marked as
            // "seen" by the set. However, we should never return a false OK (i.e., claim that
            // the value was not seen when it actually was).
            //
            // Note that despite the u16::MAX < SHARED_ENTRIES, this is still not able to be
            // 100% reliable because not all evicted entries from the local set are put into
            // the backing allocation.
            if !expected {
                assert!(!actual);
            }
        }
    });
}

#[test]
fn check_ordered_u16() {
    check!().with_type::<Vec<u16>>().for_each(|ops| {
        let mut ops = ops.clone();
        ops.sort();
        let mut oracle = std::collections::HashSet::new();
        let subject = State::with_shared(Shared::new());
        let id = Id::from([0; 16]);
        for op in ops {
            let op = KeyId::new(op as u64).unwrap();
            let expected = oracle.insert(op);
            let actual = subject
                .post_authentication(&Credentials { id, key_id: op })
                .is_ok();
            assert_eq!(actual, expected);
        }
    });
}

#[test]
fn shared() {
    let subject = Shared::new();
    let id1 = Id::from([0; 16]);
    let mut id2 = Id::from([0; 16]);
    // This is a part of the key ID not used for hashing.
    id2[10] = 1;
    let key1 = KeyId::new(0).unwrap();
    let key2 = KeyId::new(1).unwrap();
    subject.insert(&Credentials {
        id: id1,
        key_id: key1,
    });
    assert_eq!(
        subject.remove(&Credentials {
            id: id1,
            key_id: key1,
        }),
        Ok(())
    );
    assert_eq!(
        subject.remove(&Credentials {
            id: id1,
            key_id: key1,
        }),
        Err(Error::AlreadyExists)
    );
    subject.insert(&Credentials {
        id: id2,
        key_id: key1,
    });
    assert_eq!(
        subject.remove(&Credentials {
            id: id1,
            key_id: key1,
        }),
        Err(Error::Unknown)
    );
    assert_eq!(
        subject.remove(&Credentials {
            id: id1,
            key_id: key2,
        }),
        Err(Error::Unknown)
    );
    // Removal never taints an entry, so this is still fine.
    assert_eq!(
        subject.remove(&Credentials {
            id: id2,
            key_id: key1,
        }),
        Ok(())
    );
}

// This test is not particularly interesting, it's mostly just the same as the random tests above
// which insert ordered and unordered values. Mostly it tests that we continue to allow 129 IDs of
// arbitrary reordering.
#[test]
fn check_shuffled_chunks() {
    check!()
        .with_type::<(u64, u8)>()
        .for_each(|&(seed, chunk_size)| {
            check_shuffled_chunks_inner(seed, chunk_size);
        });
}

#[test]
fn check_shuffled_chunks_specific() {
    check_shuffled_chunks_inner(0xf323243, 10);
    check_shuffled_chunks_inner(0xf323243, 63);
    check_shuffled_chunks_inner(0xf323243, 129);
}

fn check_shuffled_chunks_inner(seed: u64, chunk_size: u8) {
    eprintln!("======== starting test run ({seed} {chunk_size}) ==========");
    if chunk_size == 0 || chunk_size >= 129 {
        // Needs at least 1 in the chunk.
        //
        // Chunk sizes that are larger than the local set are not guaranteed to pass, since they
        // may skip entirely over the 129-element window which then isn't inserted at all into our
        // backup/shared set.
        return;
    }
    let mut model = Model::default();
    let mut rng = rand::rngs::SmallRng::seed_from_u64(seed);
    let mut deltas = (-(chunk_size as i32 / 2)..(chunk_size as i32 / 2)).collect::<Vec<_>>();
    for initial in (128u32..100_000u32).step_by(chunk_size as usize) {
        deltas.shuffle(&mut rng);
        for delta in deltas.iter() {
            model.insert(initial.checked_add_signed(*delta).unwrap() as u64);
        }
    }
}

// This represents the commonly seen behavior in production where a small percentage of inserted
// keys are potentially significantly delayed. Currently our percentage is fixed, but the delay is
// not; it's minimum is set by our test here and the maximum is always at most SHARED_ENTRIES.
//
// This ensures that in the common case we see in production our receiver map, presuming no
// contention in the shared map, is reliably able to return accurate results.
#[test]
fn check_delayed() {
    check!()
        .with_type::<(u64, u16)>()
        .for_each(|&(seed, delay)| {
            check_delayed_inner(seed, delay);
        });
}

#[test]
fn check_delayed_specific() {
    check_delayed_inner(0xf323243, 10);
    check_delayed_inner(0xf323243, 63);
    check_delayed_inner(0xf323243, 129);
}

// delay represents the *minimum* delay a delayed entry sees. The maximum is up to SHARED_ENTRIES.
fn check_delayed_inner(seed: u64, delay: u16) {
    // We expect that the shared map is always big enough to absorb our delay.
    // (This is statically true; u16::MAX < SHARED_ENTRIES).
    assert!((delay as usize) < SHARED_ENTRIES);
    let delay = delay as u64;
    eprintln!("======== starting test run ({seed} {delay}) ==========");
    let mut model = Model::default();
    let mut rng = rand::rngs::SmallRng::seed_from_u64(seed);
    // reverse the first element (insert_before) to ensure we pop smallest pending ID first.
    // max on the second element (id_to_insert) to ensure that we go in least-favorable order if
    // there are multiple elements to insert, inserting most recent first and only afterwards older
    // entries.
    let mut buffered: BinaryHeap<(std::cmp::Reverse<u64>, u64)> = BinaryHeap::new();
    for id in 0..(SHARED_ENTRIES as u64 * 3) {
        while let Some(peeked) = buffered.peek_mut() {
            // min-heap means that if the first entry isn't the one we want, then there's no entry
            // that we want.
            if (peeked.0).0 == id {
                model.insert(peeked.1);
                PeekMut::pop(peeked);
            } else {
                break;
            }
        }
        // Every 128th ID gets put in immediately, the rest are delayed by a random amount.
        // This ensures that we always evict all the gaps as we move forward into the backing set.
        // In production, this roughly means that at least 1/128 = 0.7% of packets arrive in relative order
        // to each other. (That's an approximation, it's not obvious how to really derive a simple
        // explanation for what guarantees we're actually trying to provide here).
        if id % 128 != 0 {
            // ...until some random interval no more than SHARED_ENTRIES away.
            let insert_before = rng.gen_range(id + 1 + delay..id + SHARED_ENTRIES as u64);
            buffered.push((std::cmp::Reverse(insert_before), id));
        } else {
            model.insert(id);
        }
    }
}

struct Model {
    insert_order: Vec<u64>,
    oracle: HashSet<u64>,
    subject: State,
}

impl Default for Model {
    fn default() -> Self {
        Self {
            oracle: Default::default(),
            insert_order: Vec::new(),
            subject: State::with_shared(Shared::new()),
        }
    }
}

impl Model {
    fn insert(&mut self, op: u64) {
        let pid = Id::from([0; 16]);
        let id = KeyId::new(op).unwrap();
        let expected = self.oracle.insert(op);
        if expected {
            self.insert_order.push(op);
        }
        let actual = self.subject.post_authentication(&Credentials {
            id: pid,
            key_id: id,
        });
        if actual.is_ok() != expected {
            let mut oracle = self.oracle.iter().collect::<Vec<_>>();
            oracle.sort_unstable();
            panic!(
                "Inserting {:?} failed, in oracle: {}, in subject: {:?}, inserted: {:?}",
                op, expected, actual, self.insert_order
            );
        }
    }
}

#[test]
fn shared_no_collisions() {
    let mut seen = HashSet::new();
    let shared = Shared::new();
    for key_id in 0..SHARED_ENTRIES as u64 {
        let index = shared.index(&Credentials {
            id: Id::from([0; 16]),
            key_id: KeyId::new(key_id).unwrap(),
        });
        assert!(seen.insert(index));
    }

    // The next entry should collide, since we will wrap around.
    let index = shared.index(&Credentials {
        id: Id::from([0; 16]),
        key_id: KeyId::new(SHARED_ENTRIES as u64 + 1).unwrap(),
    });
    assert!(!seen.insert(index));
}

#[test]
fn shared_id_pair_no_collisions() {
    let shared = Shared::new();

    // Two random IDs. Exact constants shouldn't matter much, we're mainly aiming to test overall
    // quality of our mapping from Id + KeyId.
    let id1 = Id::from(u128::to_ne_bytes(0x25add729cce683cd0cda41d35436bdc6));
    let id2 = Id::from(u128::to_ne_bytes(0x2862115d0691fe180f2aeb26af3c2e5e));

    for key_id in 0..SHARED_ENTRIES as u64 {
        let index1 = shared.index(&Credentials {
            id: id1,
            key_id: KeyId::new(key_id).unwrap(),
        });
        let index2 = shared.index(&Credentials {
            id: id2,
            key_id: KeyId::new(key_id).unwrap(),
        });

        // Our path secret IDs are sufficiently different that we expect that for any given index
        // we map to a different slot. This test is not *really* saying much since it's highly
        // dependent on the exact values of the path secret IDs, but it prevents simple bugs like
        // ignoring the IDs entirely.
        assert_ne!(index1, index2);
    }
}

// Confirms that we start out without any entries present in the map.
#[test]
fn shared_no_entries() {
    let shared = Shared::new();
    // We have to check all slots to be sure. The index used for lookup is going to be shuffled due
    // to the hashing in of the secret. We need to use an all-zero path secret ID since the entries
    // in the map start out zero-initialized today.
    for key_id in 0..SHARED_ENTRIES as u64 {
        assert_eq!(
            shared.remove(&Credentials {
                id: Id::from([0; 16]),
                key_id: KeyId::new(key_id).unwrap(),
            }),
            Err(Error::Unknown)
        );
    }
}

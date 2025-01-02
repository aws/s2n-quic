// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn slot_insert_and_get() {
    let slot = Slot::new();
    assert!(slot.get_by_key(&3).is_none());
    assert_eq!(slot.put(3, "key 1"), (None, None));
    // still same slot, but new generation
    assert_eq!(slot.put(3, "key 2"), (Some("key 1"), None));
    // still same slot, but new generation
    assert_eq!(slot.put(3, "key 3"), (Some("key 2"), None));

    // new slot
    assert_eq!(slot.put(5, "key 4"), (None, None));
    assert_eq!(slot.put(6, "key 4"), (None, None));
}

#[test]
fn slot_clear() {
    let slot = Slot::new();
    assert_eq!(slot.put(3, "key 1"), (None, None));
    // still same slot, but new generation
    assert_eq!(slot.put(3, "key 2"), (Some("key 1"), None));
    // still same slot, but new generation
    assert_eq!(slot.put(3, "key 3"), (Some("key 2"), None));

    slot.clear();

    assert_eq!(slot.len(), 0);
}

#[test]
fn capacity_size() {
    let map: Map<u32, ()> = Map::with_capacity(500_000, Default::default());
    for idx in 0..500_000 {
        let _ = map.insert(idx, ());
    }
    assert!(map.count() >= 400_000, "{}", map.count());
}

#[test]
fn insert_new_key() {
    let slot = Slot::new();
    // inserting the same key should never overflow
    for _ in 0..SLOT_CAPACITY * 2 {
        assert!(slot.insert_new_key(3, ()).is_none());
    }
    assert!(slot.get_by_key(&3).is_some());
}

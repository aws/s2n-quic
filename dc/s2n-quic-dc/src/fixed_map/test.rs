// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn slot_insert_and_get() {
    let slot = Slot::new();
    assert!(slot.get_by_key(&3).is_none());
    assert_eq!(slot.put(3, "key 1"), None);
    // still same slot, but new generation
    assert_eq!(slot.put(3, "key 2"), Some("key 1"));
    // still same slot, but new generation
    assert_eq!(slot.put(3, "key 3"), Some("key 2"));

    // new slot
    assert_eq!(slot.put(5, "key 4"), None);
    assert_eq!(slot.put(6, "key 4"), None);
}

#[test]
fn slot_clear() {
    let slot = Slot::new();
    assert_eq!(slot.put(3, "key 1"), None);
    // still same slot, but new generation
    assert_eq!(slot.put(3, "key 2"), Some("key 1"));
    // still same slot, but new generation
    assert_eq!(slot.put(3, "key 3"), Some("key 2"));

    slot.clear();

    assert_eq!(slot.len(), 0);
}

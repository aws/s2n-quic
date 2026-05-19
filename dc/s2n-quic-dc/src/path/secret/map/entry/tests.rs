// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::{
    packet::secret_control as control,
    path::secret::{receiver, schedule, sender},
};
use s2n_quic_core::time::Timestamp;
use std::time::Duration;

fn test_entry_with_senders(sender_count: usize) -> Entry {
    let peer = (std::net::Ipv4Addr::LOCALHOST, 4433).into();
    let secret = schedule::Secret::new(
        schedule::Ciphersuite::AES_GCM_128_SHA256,
        s2n_quic_core::dc::SUPPORTED_VERSIONS[0],
        s2n_quic_core::endpoint::Type::Client,
        &[7u8; 32],
    );

    Entry::new_with_socket_senders(
        peer,
        secret,
        sender::State::new([0; control::TAG_LEN]),
        receiver::State::new(),
        s2n_quic_core::dc::testing::TEST_APPLICATION_PARAMS,
        crate::time::now(),
        None,
        sender_count,
    )
}

#[test]
fn entry_size() {
    let mut should_check = true;

    should_check &= cfg!(target_pointer_width = "64");
    should_check &= cfg!(target_os = "linux");
    should_check &= std::env::var("S2N_QUIC_RUN_VERSION_SPECIFIC_TESTS").is_ok();

    // This gates to running only on specific GHA to reduce false positives.
    if should_check {
        assert_eq!(
            Entry::fake((std::net::Ipv4Addr::LOCALHOST, 0).into(), None).size(),
            // Includes per-entry sender scheduling storage metadata (Box<[AtomicU64]>).
            323
        );
    }
}

#[test]
fn allocates_sender_schedule_slots() {
    let entry = test_entry_with_senders(4);
    assert_eq!(entry.socket_sender_count(), 4);
}

#[test]
fn empty_sender_schedule_is_supported() {
    let entry = test_entry_with_senders(0);
    assert_eq!(entry.socket_sender_count(), 0);
    assert_eq!(entry.sender_load_score(0), 0);
}

#[test]
fn sender_with_more_queued_bytes_has_higher_load_score() {
    let entry = test_entry_with_senders(2);
    let now = unsafe { Timestamp::from_duration(Duration::from_micros(10)) };

    entry.update_sender_load_score(
        0,
        now,
        4_000,
        s2n_quic_core::recovery::bandwidth::Bandwidth::new(1_000, Duration::from_millis(1)),
    );
    entry.update_sender_load_score(
        1,
        now,
        2_000,
        s2n_quic_core::recovery::bandwidth::Bandwidth::new(1_000, Duration::from_millis(1)),
    );

    let score0 = entry.sender_load_score(0);
    let score1 = entry.sender_load_score(1);
    assert!(
        score0 > score1,
        "sender 0 has more bytes queued so should have a higher load score"
    );
}

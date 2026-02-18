// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use std::{
    net::SocketAddr,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

#[test]
fn test_limits_concurrency() {
    // Use a short rehandshake period so we schedule multiple handshakes per minute
    let mut state = RehandshakeState::new_with_paused_time(Duration::from_secs(60));

    // Add 10 peers
    for i in 0..10 {
        state.push(SocketAddr::from(([127, 0, 0, 1], 4000 + i)));
    }
    state.adjust_post_refill();

    let concurrent = Arc::new(AtomicUsize::new(0));
    let max_concurrent = Arc::new(AtomicUsize::new(0));
    let handle = state.runtime.handle().clone();

    state.next_rehandshake_batch(10, |_addr| {
        let concurrent = concurrent.clone();
        let max_concurrent = max_concurrent.clone();

        Some(handle.spawn(async move {
            let current = concurrent.fetch_add(1, Ordering::Relaxed) + 1;
            max_concurrent.fetch_max(current, Ordering::Relaxed);

            // Simulate handshake taking 5 seconds
            tokio::time::sleep(Duration::from_secs(5)).await;

            concurrent.fetch_sub(1, Ordering::Relaxed);
        }))
    });

    // Semaphore is set to 2, so max concurrent should be 2, despite pacing.
    assert_eq!(max_concurrent.load(Ordering::Relaxed), 2);
}

#[test]
fn test_limits_concurrency_for_fast_handshakes() {
    // Use a short rehandshake period so we schedule multiple handshakes per minute
    let mut state = RehandshakeState::new_with_paused_time(Duration::from_secs(60));

    // Add 10 peers
    for i in 0..10 {
        state.push(SocketAddr::from(([127, 0, 0, 1], 4000 + i)));
    }
    state.adjust_post_refill();

    let concurrent = Arc::new(AtomicUsize::new(0));
    let max_concurrent = Arc::new(AtomicUsize::new(0));
    let handle = state.runtime.handle().clone();

    state.next_rehandshake_batch(10, |_addr| {
        let concurrent = concurrent.clone();
        let max_concurrent = max_concurrent.clone();

        Some(handle.spawn(async move {
            let current = concurrent.fetch_add(1, Ordering::Relaxed) + 1;
            max_concurrent.fetch_max(current, Ordering::Relaxed);

            // Simulate handshake taking 1ms
            tokio::time::sleep(Duration::from_millis(1)).await;

            concurrent.fetch_sub(1, Ordering::Relaxed);
        }))
    });

    // Semaphore is set to 2. However we only expect one handshake at a time because pacing is
    // configured at 10ms, and all handshakes complete fast enough that it's respected.
    assert_eq!(max_concurrent.load(Ordering::Relaxed), 1);
}

#[test]
fn test_waits_for_completion() {
    // Use a short rehandshake period so we schedule multiple handshakes per minute
    let mut state = RehandshakeState::new_with_paused_time(Duration::from_secs(60));

    for i in 0..5 {
        state.push(SocketAddr::from(([127, 0, 0, 1], 4000 + i)));
    }
    state.adjust_post_refill();

    let completed = Arc::new(AtomicUsize::new(0));
    let handle = state.runtime.handle().clone();

    state.next_rehandshake_batch(5, |_addr| {
        let completed = completed.clone();

        Some(handle.spawn(async move {
            tokio::time::sleep(Duration::from_secs(3)).await;
            completed.fetch_add(1, Ordering::Relaxed);
        }))
    });

    // All handshakes should be complete
    assert_eq!(completed.load(Ordering::Relaxed), 5);
}

#[test]
fn test_respects_60_second_deadline() {
    // Use a short rehandshake period so we try to schedule many handshakes
    let mut state = RehandshakeState::new_with_paused_time(Duration::from_secs(120));

    // Add many peers
    for i in 0..100 {
        state.push(SocketAddr::from(([127, 0, 0, 1], 4000 + i)));
    }
    state.adjust_post_refill();

    let scheduled = Arc::new(AtomicUsize::new(0));
    let handle = state.runtime.handle().clone();

    state.next_rehandshake_batch(100, |_addr| {
        let scheduled = scheduled.clone();

        scheduled.fetch_add(1, Ordering::Relaxed);

        Some(handle.spawn(async move {
            // Each handshake takes 10 seconds
            tokio::time::sleep(Duration::from_secs(10)).await;
        }))
    });

    // With 2 concurrent slots and 10s per handshake, we can complete at most
    // 60s / 10s * 2 = 12 handshakes before hitting the deadline.
    // Allow for 13 due to timing granularity.
    let count = scheduled.load(Ordering::Relaxed);
    assert!(count <= 13, "scheduled {count} handshakes, expected <= 13");

    // Should have items left in queue
    assert!(!state.queue.is_empty());
}

#[test]
fn test_keeps_unscheduled_in_queue() {
    // Use a short rehandshake period
    let mut state = RehandshakeState::new_with_paused_time(Duration::from_secs(60));

    for i in 0..20 {
        state.push(SocketAddr::from(([127, 0, 0, 1], 4000 + i)));
    }
    state.adjust_post_refill();

    let initial_count = state.queue.len();
    let handle = state.runtime.handle().clone();

    state.next_rehandshake_batch(20, |_addr| {
        Some(handle.spawn(async move {
            // Each handshake takes 30 seconds
            tokio::time::sleep(Duration::from_secs(30)).await;
        }))
    });

    let scheduled = initial_count - state.queue.len();
    assert!(
        scheduled < initial_count,
        "should not schedule all handshakes"
    );
    assert!(
        !state.queue.is_empty(),
        "should keep unscheduled items in queue"
    );
}

#[test]
fn test_tail_handshake_scheduling() {
    // Use a long rehandshake period so tail handshake logic is used
    let mut state = RehandshakeState::new_with_paused_time(Duration::from_secs(3600));

    // Single peer - should use tail handshake logic
    state.push(SocketAddr::from(([127, 0, 0, 1], 4000)));

    let scheduled = Arc::new(AtomicUsize::new(0));
    let handle = state.runtime.handle().clone();

    // Call batch enough times that we should go through the full hour. If we scheduled in the last
    // period, we'll need to include one more period since it'll slip into the next hour.
    for _ in 0..61 {
        state.next_rehandshake_batch(1, |_addr| {
            scheduled.fetch_add(1, Ordering::Relaxed);
            Some(handle.spawn(async move {}))
        });

        // Advance time by 60 seconds (one batch period)
        state.runtime.block_on(async {
            tokio::time::sleep(Duration::from_secs(60)).await;
        });
    }

    // Should have scheduled at least one handshake across all calls
    assert!(
        scheduled.load(Ordering::Relaxed) > 0,
        "should schedule at least one tail handshake"
    );
}

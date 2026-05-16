// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Contract tests for the `waker_drain` task.
//!
//! The waker drain task offloads `Waker::wake()` calls from hot dispatch threads. Producers
//! push wakers into per-slot queues via `Sink`; the drain task polls all slots round-robin
//! and invokes each waker. These tests verify that wakers are reliably fired regardless of
//! how they arrive (single, batched, multi-slot) and that the task shuts down cleanly when
//! all producers are gone.

use super::helpers::test_waker;
use crate::{
    socket::channel::{ReceiverExt as _, UnboundedSender as _},
    stream::endpoint::{tasks, waker},
    testing::ext::*,
};

/// A single waker pushed to one slot is invoked exactly once.
#[test]
fn single_waker_fires() {
    crate::testing::sim(|| {
        let (mut sinks, mut drains) = waker::new(1, 1);
        let drain = drains.pop().unwrap();

        let rx = tasks::waker_drain(drain);
        async move { rx.drain_budgeted(Some(32)).await }
            .primary()
            .spawn();

        async move {
            let (waker, did_wake) = test_waker();
            sinks[0].send(waker).unwrap();
            1.ms().sleep().await;
            assert_eq!(did_wake.count(), 1);
        }
        .primary()
        .spawn();
    });
}

/// Multiple wakers pushed to the same slot in a burst are all invoked.
/// Verifies the drain doesn't stop after the first waker per slot.
#[test]
fn batched_wakers_same_slot() {
    crate::testing::sim(|| {
        let (mut sinks, mut drains) = waker::new(1, 1);
        let drain = drains.pop().unwrap();

        let rx = tasks::waker_drain(drain);
        async move { rx.drain_budgeted(Some(32)).await }
            .primary()
            .spawn();

        async move {
            let (waker, did_wake) = test_waker();
            for _ in 0..5 {
                sinks[0].send(waker.clone()).unwrap();
            }
            1.ms().sleep().await;
            assert_eq!(did_wake.count(), 5);
        }
        .primary()
        .spawn();
    });
}

/// Wakers distributed across multiple producer slots are all drained.
/// Exercises the round-robin polling across slots.
#[test]
fn multiple_slots_all_fire() {
    crate::testing::sim(|| {
        let (mut sinks, mut drains) = waker::new(3, 1);
        let drain = drains.pop().unwrap();

        let rx = tasks::waker_drain(drain);
        async move { rx.drain_budgeted(Some(32)).await }
            .primary()
            .spawn();

        async move {
            let (waker, did_wake) = test_waker();
            for sink in sinks.iter_mut() {
                sink.send(waker.clone()).unwrap();
            }
            1.ms().sleep().await;
            assert_eq!(did_wake.count(), 3);
        }
        .primary()
        .spawn();
    });
}

/// The task continues draining after an initial batch completes.
/// Producers can push new wakers at any time and they will eventually fire.
#[test]
fn wakers_pushed_after_initial_drain_still_fire() {
    crate::testing::sim(|| {
        let (mut sinks, mut drains) = waker::new(1, 1);
        let drain = drains.pop().unwrap();

        let rx = tasks::waker_drain(drain);
        async move { rx.drain_budgeted(Some(32)).await }
            .primary()
            .spawn();

        async move {
            let (waker, did_wake) = test_waker();

            sinks[0].send(waker.clone()).unwrap();
            1.ms().sleep().await;
            assert_eq!(did_wake.count(), 1);

            // Push more after the first batch was drained
            sinks[0].send(waker.clone()).unwrap();
            sinks[0].send(waker.clone()).unwrap();
            1.ms().sleep().await;
            assert_eq!(did_wake.count(), 3);
        }
        .primary()
        .spawn();
    });
}

/// When all Sink handles are dropped, the drain task terminates cleanly.
/// This ensures graceful endpoint shutdown — no leaked tasks.
#[test]
fn shutdown_after_all_sinks_dropped() {
    crate::testing::sim(|| {
        let (mut sinks, mut drains) = waker::new(2, 1);
        let drain = drains.pop().unwrap();

        let rx = tasks::waker_drain(drain);
        async move { rx.drain_budgeted(Some(32)).await }
            .primary()
            .spawn();

        async move {
            let (waker, did_wake) = test_waker();
            sinks[0].send(waker.clone()).unwrap();
            1.ms().sleep().await;
            assert_eq!(did_wake.count(), 1);

            // Drop all sinks — drain task should shut down
            drop(sinks);
        }
        .primary()
        .spawn();
    });
}

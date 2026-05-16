// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::helpers::test_waker;
use crate::{
    socket::channel::{ReceiverExt as _, UnboundedSender as _},
    stream::endpoint::{tasks, waker},
    testing::ext::*,
};

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

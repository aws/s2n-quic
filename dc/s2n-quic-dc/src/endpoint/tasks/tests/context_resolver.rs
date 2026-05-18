// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Contract tests for the `context_resolver` task.
//!
//! The context resolver receives FrameBatches from the dispatch pipeline, looks up (or
//! creates) the corresponding send::Context for the destination peer, pushes frames into
//! the context's pending queues, and dispatches the context to the appropriate timing
//! wheels based on WheelInterest. These tests verify the lookup, push, and dispatch
//! behavior.

use super::helpers::{test_batch, test_entry, TestReceiver, TestReceiverExt as _};
use crate::{
    endpoint::{combinator::FrameBatch, send, tasks},
    socket::channel::{intrusive::unsync, ReceiverExt as _, UnboundedSender as _},
    testing::{ext::*, sim},
    time::bach::Clock,
};
use std::{cell::RefCell, rc::Rc};

type TxWheelRx = unsync::Receiver<send::TxWheelAdapter>;
type BatchTx = unsync::Sender<crate::intrusive::EntryAdapter<FrameBatch>>;

struct Harness {
    batch_tx: BatchTx,
    tx_wheel_rx: TxWheelRx,
    send_caches: Vec<Rc<RefCell<send::Cache>>>,
}

/// Spawns the context_resolver pipeline and returns a harness for driving it.
fn setup() -> Harness {
    let registry = crate::counter::Registry::default();
    let send_caches = vec![Rc::new(RefCell::new(send::Cache::new(&registry, 0)))];
    let sender_idx_to_local = vec![0];

    let (tx_wheel_tx, tx_wheel_rx) = unsync::new_with_adapter::<send::TxWheelAdapter>();
    let (pto_wheel_tx, _) = unsync::new_with_adapter::<send::PtoWheelAdapter>();
    let (idle_wheel_tx, _) = unsync::new_with_adapter::<send::IdleWheelAdapter>();

    let (batch_tx, batch_rx) = unsync::new::<FrameBatch>();

    let rx = tasks::context_resolver(
        batch_rx,
        send_caches.clone(),
        sender_idx_to_local,
        1,
        Clock::default(),
        tx_wheel_tx,
        pto_wheel_tx,
        idle_wheel_tx,
    );
    async move { rx.drain_budgeted(Some(32)).await }
        .primary()
        .spawn();

    Harness {
        batch_tx,
        tx_wheel_rx,
        send_caches,
    }
}

/// A batch for a known peer resolves the context and dispatches to the TX wheel.
#[test]
fn resolves_context_and_dispatches_to_tx_wheel() {
    sim(|| {
        let Harness {
            mut batch_tx,
            mut tx_wheel_rx,
            ..
        } = setup();

        let pse = test_entry();
        let _ = batch_tx.send(test_batch(&pse));
        drop(batch_tx);

        async move {
            let ctx = tx_wheel_rx.recv().await;
            assert!(ctx.is_some(), "expected context dispatched to TX wheel");
        }
        .primary()
        .spawn();
    });
}

/// Multiple batches for the same peer reuse the cached context (no duplicate creation).
#[test]
fn same_peer_reuses_context() {
    sim(|| {
        let Harness {
            mut batch_tx,
            mut tx_wheel_rx,
            send_caches,
        } = setup();

        let pse = test_entry();
        let _ = batch_tx.send(test_batch(&pse));
        let _ = batch_tx.send(test_batch(&pse));
        drop(batch_tx);

        let cache_ref = send_caches[0].clone();
        async move {
            let _ctx = tx_wheel_rx.recv().await.unwrap();
            assert_eq!(cache_ref.borrow().context_count(), 1);
        }
        .primary()
        .spawn();
    });
}

/// Dropping the input shuts down the resolver cleanly.
#[test]
fn input_close_shuts_down() {
    sim(|| {
        let Harness {
            batch_tx,
            mut tx_wheel_rx,
            ..
        } = setup();

        drop(batch_tx);

        async move {
            let result = tx_wheel_rx.recv().await;
            assert!(result.is_none(), "expected TX wheel channel to close");
        }
        .primary()
        .spawn();
    });
}

#[test]
fn wheel_router_routes_all_interest_combinations() {
    sim(|| {
        let registry = crate::counter::Registry::default();
        let make_context = || {
            let entry = test_entry();
            let ctx = send::Context::new(
                &entry,
                registry.register_queue_gauge("test.inflight"),
                registry.register_queue_gauge("test.ack"),
                registry.register_queue_gauge("test.pending"),
                0,
                &Clock::default(),
            )
            .unwrap();
            Rc::new(RefCell::new(ctx))
        };

        let interests = [
            send::WheelInterest {
                transmission: false,
                pto: false,
                idle_timeout: false,
            },
            send::WheelInterest {
                transmission: true,
                pto: false,
                idle_timeout: false,
            },
            send::WheelInterest {
                transmission: false,
                pto: true,
                idle_timeout: false,
            },
            send::WheelInterest {
                transmission: false,
                pto: false,
                idle_timeout: true,
            },
            send::WheelInterest {
                transmission: true,
                pto: true,
                idle_timeout: false,
            },
            send::WheelInterest {
                transmission: true,
                pto: false,
                idle_timeout: true,
            },
            send::WheelInterest {
                transmission: false,
                pto: true,
                idle_timeout: true,
            },
            send::WheelInterest {
                transmission: true,
                pto: true,
                idle_timeout: true,
            },
        ];
        let input = TestReceiver::new(interests.into_iter().map(|i| (make_context(), i)));
        let (tx_sender, mut tx_items) = unsync::new_with_adapter::<send::TxWheelAdapter>();
        let (pto_sender, mut pto_items) = unsync::new_with_adapter::<send::PtoWheelAdapter>();
        let (idle_sender, mut idle_items) = unsync::new_with_adapter::<send::IdleWheelAdapter>();
        let mut router = send::WheelRouter::new(input, tx_sender, pto_sender, idle_sender);

        async move {
            while router.recv().await.is_some() {}
            drop(router);
            let mut tx = 0usize;
            while tx_items.recv().await.is_some() {
                tx += 1;
            }
            let mut pto = 0usize;
            while pto_items.recv().await.is_some() {
                pto += 1;
            }
            let mut idle = 0usize;
            while idle_items.recv().await.is_some() {
                idle += 1;
            }
            assert_eq!(tx, 4);
            assert_eq!(pto, 4);
            assert_eq!(idle, 4);
        }
        .primary()
        .spawn();
    });
}

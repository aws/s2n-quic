// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Contract tests for extracted send-worker pipeline helpers.

use super::helpers::{test_batch, test_entry, TestReceiver, TestReceiverExt as _};
use crate::{
    endpoint::{frame, msg, send, tasks},
    socket::channel::{intrusive::unsync, ReceiverExt as _, UnboundedSender as _},
    testing::{ext::*, sim},
    time::bach::Clock,
    xorshift::Rng,
};
use bytes::BytesMut;
use core::time::Duration;
use s2n_quic_core::varint::VarInt;
use std::{cell::RefCell, rc::Rc};

#[test]
fn send_ack_processor_ignores_invalid_sender_id() {
    let _guard = crate::testing::without_snapshots();
    sim(|| {
        let registry = crate::counter::Registry::default();
        let send_caches = vec![Rc::new(RefCell::new(send::Cache::new(&registry, 0)))];
        let sender_idx_to_local = vec![0usize];

        let (mut ack_tx, ack_rx) = unsync::new::<msg::Sender>();
        let (tx_wheel_tx, mut tx_wheel_rx) = unsync::new_with_adapter::<send::TxWheelAdapter>();
        let (pto_wheel_tx, mut pto_wheel_rx) = unsync::new_with_adapter::<send::PtoWheelAdapter>();
        let (idle_wheel_tx, mut idle_wheel_rx) = unsync::new_with_adapter::<send::IdleWheelAdapter>();
        let (completed_tx, mut completed_rx) = unsync::new::<frame::Frame>();
        let (cancelled_tx, mut cancelled_rx) = unsync::new::<frame::Frame>();
        let (frame_tx, _frame_rx) = frame::submission_channel(1);
        let counters = crate::endpoint::counters::Send::new(&registry);

        let rx = tasks::send_ack_processor(
            ack_rx,
            send_caches,
            sender_idx_to_local,
            1,
            Clock::default(),
            Rng::new(),
            frame_tx,
            completed_tx,
            cancelled_tx,
            counters,
            tx_wheel_tx,
            pto_wheel_tx,
            idle_wheel_tx,
        );

        async move { rx.drain_budgeted(Some(32)).await }
            .primary()
            .spawn();

        async move {
            let entry = test_entry();
            let _ = ack_tx.send(crate::intrusive::Entry::new(msg::Sender::ReceivedAck {
                local_sender_id: VarInt::from_u8(3),
                path_secret_entry: entry,
                payload: BytesMut::new(),
                ack_delay: Duration::ZERO,
            }));
            drop(ack_tx);
        }
        .spawn();

        async move {
            assert!(
                tx_wheel_rx.recv().await.is_none(),
                "invalid sender id should not schedule tx wheel work"
            );
            assert!(
                pto_wheel_rx.recv().await.is_none(),
                "invalid sender id should not schedule pto wheel work"
            );
            assert!(
                idle_wheel_rx.recv().await.is_none(),
                "invalid sender id should not schedule idle wheel work"
            );
            assert!(
                completed_rx.recv().await.is_none(),
                "invalid sender id should not emit completions"
            );
            assert!(
                cancelled_rx.recv().await.is_none(),
                "invalid sender id should not emit cancellations"
            );
        }
        .primary()
        .spawn();
    });
}

#[test]
fn send_pto_timeout_routes_pending_context_to_tx_wheel() {
    let _guard = crate::testing::without_snapshots();
    sim(|| {
        let registry = crate::counter::Registry::default();
        let clock = Clock::default();
        let entry = test_entry();
        let mut ctx = send::Context::new(
            &entry,
            registry.register_queue_gauge("test.inflight"),
            registry.register_queue_gauge("test.ack"),
            registry.register_queue_gauge("test.pending"),
            0,
            &clock,
        )
        .expect("test context should be constructible");
        let _ = ctx.push_batch(test_batch(&entry).into_inner(), &clock);
        let input = TestReceiver::new([Rc::new(RefCell::new(ctx))]);

        let (tx_wheel_tx, mut tx_wheel_rx) = unsync::new_with_adapter::<send::TxWheelAdapter>();
        let (pto_wheel_tx, mut pto_wheel_rx) = unsync::new_with_adapter::<send::PtoWheelAdapter>();
        let (idle_wheel_tx, mut idle_wheel_rx) = unsync::new_with_adapter::<send::IdleWheelAdapter>();

        let rx = tasks::send_pto_timeout(
            input,
            clock,
            tx_wheel_tx,
            pto_wheel_tx,
            idle_wheel_tx,
            registry.register("tx.pto_check"),
            registry.register("tx.pto_requested"),
        );

        async move { rx.drain_budgeted(Some(32)).await }
            .primary()
            .spawn();

        async move {
            assert!(
                tx_wheel_rx.recv().await.is_some(),
                "pto timeout pipeline should route pending context to tx wheel"
            );
            assert!(
                pto_wheel_rx.recv().await.is_none(),
                "no inflight probe state should avoid pto re-scheduling in this scenario"
            );
            assert!(
                idle_wheel_rx.recv().await.is_some(),
                "pto timeout pipeline should preserve idle scheduling for active context"
            );
        }
        .primary()
        .spawn();
    });
}

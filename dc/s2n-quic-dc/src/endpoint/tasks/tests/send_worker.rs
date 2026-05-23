// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Contract tests for extracted send-worker pipeline helpers.
use super::helpers::{test_batch, test_entry, TestReceiver, TestReceiverExt as _};
use crate::{
    endpoint::{frame, id::Id, msg, send, tasks},
    socket::channel::{intrusive::unsync, ReceiverExt as _, UnboundedSender as _},
    testing::{ext::*, sim},
    time::{bach::Clock, precision},
    tracing::*,
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
        let send_caches: crate::endpoint::id::IdMap<crate::endpoint::id::LocalSendSocketId, _> =
            vec![Rc::new(RefCell::new(send::Cache::new(
                &registry,
                crate::endpoint::id::LocalSenderId::from_index(0),
            )))]
            .into();
        let sender_idx_to_local = crate::endpoint::id::IdMap::<
            crate::endpoint::id::LocalSenderId,
            crate::endpoint::id::LocalSendSocketId,
        >::new(1, crate::endpoint::id::LocalSendSocketId::new(0));

        let (mut ack_tx, ack_rx) = unsync::new::<msg::Sender>();
        let (tx_wheel_tx, mut tx_wheel_rx) = unsync::new_with_adapter::<send::TxWheelAdapter>();
        let (pto_wheel_tx, mut pto_wheel_rx) = unsync::new_with_adapter::<send::PtoWheelAdapter>();
        let (idle_wheel_tx, mut idle_wheel_rx) =
            unsync::new_with_adapter::<send::IdleWheelAdapter>();
        let (completed_tx, mut completed_rx) = unsync::new::<frame::Frame>();
        let (cancelled_tx, mut cancelled_rx) = unsync::new::<frame::Frame>();
        let (frame_tx, _frame_rx) = frame::submission_channel(1);

        let (immediate_tx_raw, _immediate_rx) =
            unsync::new_with_adapter::<send::TxImmediateAdapter>();
        let socket_immediate_txs: crate::endpoint::id::IdMap<
            crate::endpoint::id::LocalSendSocketId,
            _,
        > = core::iter::once((
            crate::endpoint::id::LocalSendSocketId::new(0),
            immediate_tx_raw,
        ))
        .collect();
        let mut sender_idx_to_local_imm =
            crate::endpoint::id::IdMap::<
                crate::endpoint::id::LocalSenderId,
                crate::endpoint::id::LocalSendSocketId,
            >::new(1, crate::endpoint::id::LocalSendSocketId::new(0));
        sender_idx_to_local_imm[crate::endpoint::id::LocalSenderId::from_index(0)] =
            crate::endpoint::id::LocalSendSocketId::new(0);
        let immediate_tx =
            send::ImmediateSender::new(socket_immediate_txs, sender_idx_to_local_imm);

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
            registry.register("!send.invalid_sender_idx"),
            immediate_tx,
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
                local_sender_id: crate::endpoint::id::LocalSenderId::new(VarInt::from_u8(3)),
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
            crate::endpoint::id::LocalSenderId::from_index(0),
            &clock,
        )
        .expect("test context should be constructible");
        let _ = ctx.push_batch(test_batch(&entry).into_inner(), &clock);
        let input = TestReceiver::new([Rc::new(RefCell::new(ctx))]);

        let (tx_wheel_tx, mut tx_wheel_rx) = unsync::new_with_adapter::<send::TxWheelAdapter>();
        let (pto_wheel_tx, mut pto_wheel_rx) = unsync::new_with_adapter::<send::PtoWheelAdapter>();
        let (idle_wheel_tx, mut idle_wheel_rx) =
            unsync::new_with_adapter::<send::IdleWheelAdapter>();

        let (pto_immediate_tx_raw, _pto_immediate_rx) =
            unsync::new_with_adapter::<send::TxImmediateAdapter>();
        let pto_socket_immediate_txs: crate::endpoint::id::IdMap<
            crate::endpoint::id::LocalSendSocketId,
            _,
        > = core::iter::once((
            crate::endpoint::id::LocalSendSocketId::new(0),
            pto_immediate_tx_raw,
        ))
        .collect();
        let mut pto_sender_idx_to_local =
            crate::endpoint::id::IdMap::<
                crate::endpoint::id::LocalSenderId,
                crate::endpoint::id::LocalSendSocketId,
            >::new(1, crate::endpoint::id::LocalSendSocketId::new(0));
        pto_sender_idx_to_local[crate::endpoint::id::LocalSenderId::from_index(0)] =
            crate::endpoint::id::LocalSendSocketId::new(0);
        let pto_immediate_tx =
            send::ImmediateSender::new(pto_socket_immediate_txs, pto_sender_idx_to_local);

        let rx = tasks::send_pto_timeout(
            input,
            clock,
            pto_immediate_tx,
            tx_wheel_tx,
            pto_wheel_tx,
            idle_wheel_tx,
            registry.register("tx.pto_check"),
            registry.register("tx.pto_requested"),
            crate::endpoint::id::IdMap::<crate::endpoint::id::LocalSendSocketId, _>::from(vec![]),
            crate::endpoint::id::IdMap::<
                crate::endpoint::id::LocalSenderId,
                crate::endpoint::id::LocalSendSocketId,
            >::new(0, crate::endpoint::id::LocalSendSocketId::new(0)),
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

#[test]
fn send_tx_wheel_drain_routes_expired_context_to_matching_socket() {
    sim(|| {
        let registry = crate::counter::Registry::default();
        let clock = Clock::default();
        let entry = test_entry();
        let mut ctx = send::Context::new(
            &entry,
            registry.register_queue_gauge("test.inflight"),
            registry.register_queue_gauge("test.ack"),
            registry.register_queue_gauge("test.pending"),
            crate::endpoint::id::LocalSenderId::from_index(1),
            &clock,
        )
        .expect("test context should be constructible");
        let _ = ctx.push_batch(test_batch(&entry).into_inner(), &clock);
        ctx.tx_wheel.target_time = Some(precision::Clock::now(&clock));
        let ctx = Rc::new(RefCell::new(ctx));

        let (mut tx_wheel_tx, tx_wheel_rx) = unsync::new_with_adapter::<send::TxWheelAdapter>();
        let (socket0_tx, mut socket0_rx) = unsync::new_with_adapter::<send::TxWheelAdapter>();
        let (socket1_tx, mut socket1_rx) = unsync::new_with_adapter::<send::TxWheelAdapter>();

        tasks::send_tx_wheel_drain(
            tx_wheel_rx,
            clock,
            registry.register_queue_gauge("test.tx_wheel"),
            vec![socket0_tx, socket1_tx].into(),
            {
                let mut m = crate::endpoint::id::IdMap::<
                    crate::endpoint::id::LocalSenderId,
                    crate::endpoint::id::LocalSendSocketId,
                >::new(
                    2, crate::endpoint::id::LocalSendSocketId::new(usize::MAX)
                );
                m[crate::endpoint::id::LocalSenderId::from_index(0)] =
                    crate::endpoint::id::LocalSendSocketId::new(0);
                m[crate::endpoint::id::LocalSenderId::from_index(1)] =
                    crate::endpoint::id::LocalSendSocketId::new(1);
                m
            },
            32,
            registry.register_nominal_task("task.tx_wheel", "send.0"),
        )
        .spawn();

        async move {
            debug!("sending context to tx wheel");
            let _ = tx_wheel_tx.send(ctx);
            drop(tx_wheel_tx);
        }
        .spawn();

        async move {
            let routed = socket1_rx.recv().await.is_some();
            debug!(routed, "socket 1 routing result");
            assert!(
                routed,
                "sender_idx=1 context should route to socket queue 1"
            );
            let unexpected = socket0_rx.recv().await.is_some();
            debug!(unexpected, "socket 0 routing result");
            assert!(
                !unexpected,
                "socket queue 0 should not receive sender_idx=1 context"
            );
        }
        .primary()
        .spawn();
    });
}

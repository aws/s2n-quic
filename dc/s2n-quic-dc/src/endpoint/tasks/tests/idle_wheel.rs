// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Contract tests for the idle wheel drain tasks (send and recv).
//!
//! The idle wheel fires after a peer's negotiated idle timeout elapses without activity.
//! On expiry the callback re-checks PathSecretEntry.last_activity — if still idle, the
//! context is evicted from the cache; if activity occurred, it is rescheduled.

use super::helpers::{test_entry, test_frame, RecvContextBuilder, TestReceiverExt, WakeNowSender};
use crate::{
    endpoint::{
        frame::{self, Frame},
        id::{Id, IdMap, LocalSendSocketId, LocalSenderId},
        msg, recv, send, tasks,
    },
    flow,
    intrusive::Entry,
    socket::channel::{intrusive::unsync, ReceiverExt as _, UnboundedSender as _},
    testing::{ext::*, sim},
    time::{bach::Clock, precision},
};
use s2n_quic_core::varint::VarInt;
use std::{cell::RefCell, rc::Rc};

// ── Send idle wheel tests ────────────────────────────────────────────────────

fn setup_send() -> (
    IdMap<LocalSendSocketId, Rc<RefCell<send::Cache>>>,
    unsync::Sender<send::IdleWheelAdapter>,
    unsync::Receiver<crate::intrusive::EntryAdapter<Frame>>,
    Clock,
    crate::counter::Registry,
    msg::queue::Allocator,
) {
    let registry = crate::counter::Registry::default();
    let clock = Clock::default();
    let send_caches: IdMap<LocalSendSocketId, _> = vec![Rc::new(RefCell::new(send::Cache::new(
        &registry,
        LocalSenderId::from_index(0),
    )))]
    .into();
    let sender_idx_to_local =
        IdMap::<LocalSenderId, LocalSendSocketId>::new(1, LocalSendSocketId::new(0));

    let (idle_wheel_tx, idle_wheel_rx) = unsync::new_with_adapter::<send::IdleWheelAdapter>();
    let (completed_tx, completed_rx) = unsync::new::<Frame>();
    let queue_allocator = msg::queue::Allocator::new();
    let queue_dispatcher = queue_allocator.dispatcher();
    let (peer_dead_tx, peer_dead_rx) = unsync::new::<tasks::PeerDead>();
    let q_gauge = registry.register_queue_gauge("test.idle_wheel");

    tasks::send_idle_wheel_drain(
        idle_wheel_rx,
        idle_wheel_tx.clone(),
        clock.clone(),
        q_gauge,
        send_caches.clone(),
        sender_idx_to_local,
        completed_tx,
        peer_dead_tx,
        crate::endpoint::DEFAULT_DEAD_PEER_COOLDOWN,
        registry.register("idle.send.expired"),
        registry.register("idle.send.rescheduled"),
        registry.register_nominal_timer("idle.send.lifetime", "send.0"),
        32,
        registry.register_nominal_task("task.idle_wheel", "send.0"),
    )
    .spawn();

    let peer_dead_broadcast_task = tasks::peer_dead_broadcast(
        peer_dead_rx,
        queue_dispatcher,
        WakeNowSender,
        tasks::PeerDeadCounters {
            events: registry.register("test.peer_dead.events"),
            broadcasted: registry.register("test.peer_dead.broadcasted"),
        },
    );
    async move { peer_dead_broadcast_task.drain_budgeted(Some(32)).await }.spawn();

    (
        send_caches,
        idle_wheel_tx,
        completed_rx,
        clock,
        registry,
        queue_allocator,
    )
}

#[test]
fn send_idle_wheel_expires_inactive_context() {
    let _guard = crate::testing::without_snapshots();
    sim(|| {
        let (send_caches, mut idle_wheel_tx, mut completed_rx, clock, _registry, _queue_allocator) =
            setup_send();

        let pse = test_entry();
        // Simulate initial activity so is_idle_expired can fire
        pse.touch_activity(precision::Clock::now(&clock));

        let ctx = send_caches[LocalSendSocketId::new(0)]
            .borrow_mut()
            .get_or_insert(&pse, &clock)
            .unwrap();

        // Push a frame so invalidation has something to drain
        ctx.borrow_mut().queues[1].push_back(test_frame(&pse));

        // Schedule into the idle wheel
        {
            let mut c = ctx.borrow_mut();
            let timeout = c.path_secret_entry.idle_timeout();
            c.idle_wheel.target_time = Some(precision::Clock::now(&clock) + timeout);
        }
        let _ = idle_wheel_tx.send(ctx);

        let send_caches = send_caches.clone();
        async move {
            // Default idle_timeout is 60s. Advance past it.
            61.s().sleep().await;
            assert_eq!(
                send_caches[LocalSendSocketId::new(0)]
                    .borrow()
                    .context_count(),
                0,
                "context should be evicted after idle timeout"
            );

            // The drained frame should arrive on completed_rx marked as Failed(PeerDead)
            let frame: Entry<Frame> = TestReceiverExt::recv(&mut completed_rx)
                .await
                .expect("should receive the drained frame on completed_rx");
            assert_eq!(
                frame.status,
                frame::TransmissionStatus::Failed(frame::FailureReason::PeerDead),
                "frame should be marked Failed(PeerDead)"
            );
        }
        .primary()
        .spawn();
    });
}

#[test]
fn send_idle_wheel_reschedules_active_context() {
    let _guard = crate::testing::without_snapshots();
    sim(|| {
        let (send_caches, mut idle_wheel_tx, _completed_rx, clock, _registry, _queue_allocator) =
            setup_send();

        let pse = test_entry();
        let ctx = send_caches[LocalSendSocketId::new(0)]
            .borrow_mut()
            .get_or_insert(&pse, &clock)
            .unwrap();

        // Touch activity so the entry has a last_activity timestamp
        pse.touch_activity(precision::Clock::now(&clock));

        // Schedule into the idle wheel
        {
            let mut c = ctx.borrow_mut();
            let timeout = c.path_secret_entry.idle_timeout();
            c.idle_wheel.target_time = Some(precision::Clock::now(&clock) + timeout);
        }
        let _ = idle_wheel_tx.send(ctx);

        let send_caches = send_caches.clone();
        let pse = pse.clone();
        async move {
            // Touch activity at 30s
            30.s().sleep().await;
            pse.touch_activity(precision::Clock::now(&Clock::default()));

            // At 70s total the context should still be alive (activity at 30s → expires at 90s)
            40.s().sleep().await;
            assert_eq!(
                send_caches[LocalSendSocketId::new(0)]
                    .borrow()
                    .context_count(),
                1,
                "context should still be alive after activity refresh"
            );

            // At 95s total (past 90s) it should be evicted
            25.s().sleep().await;
            assert_eq!(
                send_caches[LocalSendSocketId::new(0)]
                    .borrow()
                    .context_count(),
                0,
                "context should be evicted after extended idle"
            );
        }
        .primary()
        .spawn();
    });
}

#[test]
fn send_idle_wheel_expires_reader_only_queue_with_reset() {
    let _guard = crate::testing::without_snapshots();
    sim(|| {
        let (send_caches, mut idle_wheel_tx, _completed_rx, clock, _registry, mut queue_allocator) =
            setup_send();

        let pse = test_entry();
        pse.touch_activity(precision::Clock::now(&clock));

        let stream_id = VarInt::from_u8(7);
        let handle = flow::Handle::client(stream_id, pse.clone());
        let (queue_control, queue_stream) = queue_allocator.alloc_or_grow(handle, None);

        let ctx = send_caches[LocalSendSocketId::new(0)]
            .borrow_mut()
            .get_or_insert(&pse, &clock)
            .unwrap();

        {
            let mut c = ctx.borrow_mut();
            let timeout = c.path_secret_entry.idle_timeout();
            c.idle_wheel.target_time = Some(precision::Clock::now(&clock) + timeout);
        }
        let _ = idle_wheel_tx.send(ctx);

        let send_caches = send_caches.clone();
        async move {
            61.s().sleep().await;
            assert_eq!(
                send_caches[LocalSendSocketId::new(0)]
                    .borrow()
                    .context_count(),
                0,
                "context should be evicted after idle timeout"
            );

            let stream_queue_entries = queue_stream
                .try_swap()
                .expect("stream queue should still be open");
            assert!(
                stream_queue_entries.iter().any(|entry| {
                    matches!(
                        &*entry,
                        msg::Stream::Reset {
                            error_code
                        } if error_code.as_u64() == crate::endpoint::error::IDLE_TIMEOUT.as_u64()
                    )
                }),
                "stream queue should receive idle-timeout reset"
            );

            let control_queue_entries = queue_control
                .try_swap()
                .expect("control queue should still be open");
            assert!(
                control_queue_entries.iter().any(|entry| {
                    matches!(
                        &*entry,
                        msg::Control::Reset {
                            error_code
                        } if error_code.as_u64() == crate::endpoint::error::IDLE_TIMEOUT.as_u64()
                    )
                }),
                "control queue should receive idle-timeout reset"
            );
        }
        .primary()
        .spawn();
    });
}

// ── Recv idle wheel tests ────────────────────────────────────────────────────

fn setup_recv() -> (
    Rc<RefCell<recv::Cache>>,
    unsync::Sender<recv::IdleWheelAdapter>,
    Clock,
    crate::counter::Registry,
    msg::queue::Allocator,
) {
    let registry = crate::counter::Registry::default();
    let clock = Clock::default();
    let recv_cache = Rc::new(RefCell::new(recv::Cache::new(
        crate::endpoint::id::RecvDispatchWorkerId::new(0),
    )));

    let (idle_wheel_tx, idle_wheel_rx) = unsync::new_with_adapter::<recv::IdleWheelAdapter>();
    let (peer_dead_tx, peer_dead_rx) = unsync::new::<tasks::PeerDead>();
    let queue_allocator = msg::queue::Allocator::new();
    let queue_dispatcher = queue_allocator.dispatcher();
    let q_gauge = registry.register_queue_gauge("test.recv_idle_wheel");

    tasks::recv_idle_wheel_drain(
        idle_wheel_rx,
        idle_wheel_tx.clone(),
        clock.clone(),
        q_gauge,
        recv_cache.clone(),
        peer_dead_tx,
        crate::endpoint::DEFAULT_DEAD_PEER_COOLDOWN,
        registry.register("idle.recv.expired"),
        registry.register("idle.recv.rescheduled"),
        registry.register_nominal_timer("idle.recv.lifetime", "recv.0"),
        32,
        registry.register_nominal_task("task.recv_idle_wheel", "recv.0"),
    )
    .spawn();

    let rx = tasks::peer_dead_broadcast(
        peer_dead_rx,
        queue_dispatcher,
        WakeNowSender,
        tasks::PeerDeadCounters {
            events: registry.register("test.peer_dead.recv.events"),
            broadcasted: registry.register("test.peer_dead.recv.broadcasted"),
        },
    );
    async move { rx.drain_budgeted(Some(32)).await }.spawn();

    (recv_cache, idle_wheel_tx, clock, registry, queue_allocator)
}

#[test]
fn recv_idle_wheel_expires_inactive_context() {
    sim(|| {
        let (recv_cache, mut idle_wheel_tx, clock, _registry, _queue_allocator) = setup_recv();

        let ctx = RecvContextBuilder::default().build();
        // Simulate initial activity
        ctx.borrow()
            .path_entry
            .touch_activity(precision::Clock::now(&clock));
        let key = {
            let c = ctx.borrow();
            recv::Key {
                id: *c.path_entry.id(),
                remote_sender_id: c.remote_sender_id,
            }
        };
        recv_cache.borrow_mut().senders.insert(key, ctx.clone());
        let _ = idle_wheel_tx.send(ctx);

        let recv_cache = recv_cache.clone();
        async move {
            61.s().sleep().await;
            assert!(
                recv_cache.borrow().senders.is_empty(),
                "recv context should be evicted after idle timeout"
            );
        }
        .primary()
        .spawn();
    });
}

#[test]
fn recv_idle_wheel_reschedules_active_context() {
    sim(|| {
        let (recv_cache, mut idle_wheel_tx, clock, _registry, _queue_allocator) = setup_recv();

        let ctx = RecvContextBuilder::default().build();
        let key = {
            let c = ctx.borrow();
            recv::Key {
                id: *c.path_entry.id(),
                remote_sender_id: c.remote_sender_id,
            }
        };
        // Touch activity so last_activity != 0
        ctx.borrow()
            .path_entry
            .touch_activity(precision::Clock::now(&clock));
        recv_cache.borrow_mut().senders.insert(key, ctx.clone());
        let _ = idle_wheel_tx.send(ctx);

        let recv_cache = recv_cache.clone();
        async move {
            // Touch activity at 30s
            30.s().sleep().await;
            let ctx = recv_cache.borrow().senders.values().next().unwrap().clone();
            ctx.borrow()
                .path_entry
                .touch_activity(precision::Clock::now(&Clock::default()));

            // At 70s total the context should still be alive
            40.s().sleep().await;
            assert_eq!(
                recv_cache.borrow().senders.len(),
                1,
                "recv context should still be alive after activity refresh"
            );

            // At 95s total (past 90s) it should be evicted
            25.s().sleep().await;
            assert!(
                recv_cache.borrow().senders.is_empty(),
                "recv context should be evicted after extended idle"
            );
        }
        .primary()
        .spawn();
    });
}

#[test]
fn recv_idle_wheel_expires_reader_only_queue_with_reset() {
    let _guard = crate::testing::without_snapshots();
    sim(|| {
        let (recv_cache, mut idle_wheel_tx, clock, _registry, mut queue_allocator) = setup_recv();

        let ctx = RecvContextBuilder::default().build();
        ctx.borrow()
            .path_entry
            .touch_activity(precision::Clock::now(&clock));
        let key = {
            let c = ctx.borrow();
            recv::Key {
                id: *c.path_entry.id(),
                remote_sender_id: c.remote_sender_id,
            }
        };
        let path_entry = ctx.borrow().path_entry.clone();
        let stream_id = VarInt::from_u8(9);
        let handle = flow::Handle::client(stream_id, path_entry);
        let (queue_control, queue_stream) = queue_allocator.alloc_or_grow(handle, None);

        recv_cache.borrow_mut().senders.insert(key, ctx.clone());
        let _ = idle_wheel_tx.send(ctx);

        let recv_cache = recv_cache.clone();
        async move {
            61.s().sleep().await;
            assert!(
                recv_cache.borrow().senders.is_empty(),
                "recv context should be evicted after idle timeout"
            );

            let stream_queue = queue_stream
                .try_swap()
                .expect("stream queue should still be open");
            assert!(
                stream_queue.iter().any(|entry| {
                    matches!(
                        &*entry,
                        msg::Stream::Reset {
                            error_code
                        } if error_code.as_u64() == crate::endpoint::error::IDLE_TIMEOUT.as_u64()
                    )
                }),
                "stream queue should receive idle-timeout reset"
            );

            let control_queue = queue_control
                .try_swap()
                .expect("control queue should still be open");
            assert!(
                control_queue.iter().any(|entry| {
                    matches!(
                        &*entry,
                        msg::Control::Reset {
                            error_code
                        } if error_code.as_u64() == crate::endpoint::error::IDLE_TIMEOUT.as_u64()
                    )
                }),
                "control queue should receive idle-timeout reset"
            );
        }
        .primary()
        .spawn();
    });
}

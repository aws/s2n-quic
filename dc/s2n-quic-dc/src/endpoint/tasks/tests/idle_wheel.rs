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
        inflight::{Packet, TransmissionInfo},
        recv, send, tasks,
    },
    intrusive::{Entry, Queue},
    socket::channel::{intrusive::unsync, ReceiverExt as _, UnboundedSender as _},
    testing::{ext::*, sim},
    time::{bach::Clock, precision},
};
use s2n_quic_core::{packet::number::PacketNumberSpace, time::Clock as _, varint::VarInt};
use std::{cell::RefCell, rc::Rc};

// ── Send idle wheel tests ────────────────────────────────────────────────────

fn setup_send() -> (
    IdMap<LocalSendSocketId, Rc<RefCell<send::Cache>>>,
    unsync::Sender<send::IdleWheelAdapter>,
    unsync::Receiver<crate::intrusive::EntryAdapter<Frame>>,
    Clock,
    crate::counter::Registry,
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
    let sender_local_addrs =
        IdMap::<LocalSendSocketId, std::net::SocketAddr>::new(1, "127.0.0.1:0".parse().unwrap());

    let (idle_wheel_tx, idle_wheel_rx) = unsync::new_with_adapter::<send::IdleWheelAdapter>();
    let (completed_tx, completed_rx) = unsync::new::<Frame>();
    let (peer_dead_tx, peer_dead_rx) = unsync::new::<tasks::PeerDead>();
    let q_gauge = registry.register_queue_gauge("test.idle_wheel");

    tasks::send_idle_wheel_drain(
        idle_wheel_rx,
        idle_wheel_tx.clone(),
        clock.clone(),
        q_gauge,
        send_caches.clone(),
        sender_idx_to_local,
        sender_local_addrs,
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
        WakeNowSender,
        tasks::PeerDeadCounters {
            events: registry.register("test.peer_dead.events"),
            broadcasted: registry.register("test.peer_dead.broadcasted"),
        },
    );
    async move { peer_dead_broadcast_task.drain_budgeted(Some(32)).await }.spawn();

    (send_caches, idle_wheel_tx, completed_rx, clock, registry)
}

#[test]
fn send_idle_wheel_expires_inactive_context() {
    let _guard = crate::testing::without_snapshots();
    sim(|| {
        let (send_caches, mut idle_wheel_tx, mut completed_rx, clock, _registry) = setup_send();

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
        let (send_caches, mut idle_wheel_tx, _completed_rx, clock, _registry) = setup_send();

        let pse = test_entry();
        let ctx = send_caches[LocalSendSocketId::new(0)]
            .borrow_mut()
            .get_or_insert(&pse, &clock)
            .unwrap();

        // Schedule into the idle wheel
        {
            let mut c = ctx.borrow_mut();
            let timeout = c.path_secret_entry.idle_timeout();
            c.idle_wheel.target_time = Some(precision::Clock::now(&clock) + timeout);
        }
        let _ = idle_wheel_tx.send(ctx.clone());

        let send_caches = send_caches.clone();
        let ctx = ctx.clone();
        async move {
            // Simulate peer activity (ACK received) at T=1s. The idle wheel uses
            // last_peer_activity to anchor the timeout, so the context should survive
            // until last_peer_activity + idle_timeout = 1s + 30s = T=31s.
            1.s().sleep().await;
            ctx.borrow_mut().last_peer_activity = precision::Clock::now(&Clock::default());

            // At T=25s the context should still be alive (expires at T=31s)
            24.s().sleep().await;
            assert_eq!(
                send_caches[LocalSendSocketId::new(0)]
                    .borrow()
                    .context_count(),
                1,
                "context should still be alive before last_peer_activity + idle_timeout"
            );

            // At T=35s (past 31s) it should be evicted
            10.s().sleep().await;
            assert_eq!(
                send_caches[LocalSendSocketId::new(0)]
                    .borrow()
                    .context_count(),
                0,
                "context should be evicted after last_peer_activity + idle_timeout"
            );
        }
        .primary()
        .spawn();
    });
}

#[test]
fn send_idle_wheel_expires_reader_only_queue_no_reset_without_inflight() {
    let _guard = crate::testing::without_snapshots();
    sim(|| {
        let (send_caches, mut idle_wheel_tx, _completed_rx, clock, _registry) = setup_send();

        let pse = test_entry();
        pse.touch_activity(precision::Clock::now(&clock));

        let ctx = send_caches[LocalSendSocketId::new(0)]
            .borrow_mut()
            .get_or_insert(&pse, &clock)
            .unwrap();

        // No inflight packets — this is a naturally idle connection.
        assert!(!ctx.borrow().inflight.has_inflight());

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

            // Without inflight packets, the peer is not marked dead.
            // The entry's dead_at should remain unset (-1).
            let now = crate::time::DefaultClock::default().now();
            assert!(
                !pse.is_dead_during_cooldown(now, crate::endpoint::DEFAULT_DEAD_PEER_COOLDOWN),
                "peer should NOT be marked dead when idle expires without inflight"
            );
        }
        .primary()
        .spawn();
    });
}

/// When a send context goes idle with NO packets in flight, the peer must NOT be
/// marked dead. An empty inflight set means both sides simply stopped talking —
/// this is normal idle, not evidence of a dead peer.
#[test]
fn send_idle_wheel_no_inflight_does_not_mark_dead() {
    sim(|| {
        let (send_caches, mut idle_wheel_tx, _completed_rx, clock, _registry) = setup_send();

        let pse = test_entry();
        pse.touch_activity(precision::Clock::now(&clock));

        let ctx = send_caches[LocalSendSocketId::new(0)]
            .borrow_mut()
            .get_or_insert(&pse, &clock)
            .unwrap();

        // No frames queued, no inflight packets — purely idle.
        assert!(!ctx.borrow().inflight.has_inflight());

        {
            let mut c = ctx.borrow_mut();
            let timeout = c.path_secret_entry.idle_timeout();
            c.idle_wheel.target_time = Some(precision::Clock::now(&clock) + timeout);
        }
        let _ = idle_wheel_tx.send(ctx);

        let send_caches = send_caches.clone();
        let pse = pse.clone();
        async move {
            // Idle timeout is 30s. Sleep just past it so the wheel fires.
            31.s().sleep().await;

            // The context should be evicted (cleanup is fine).
            assert_eq!(
                send_caches[LocalSendSocketId::new(0)]
                    .borrow()
                    .context_count(),
                0,
                "context should be evicted after idle timeout"
            );

            // Check immediately — well within the 30s cooldown window.
            // The peer must NOT be marked dead when no packets were in flight.
            let now = precision::Clock::now(&Clock::default());
            assert!(
                !pse.is_dead_during_cooldown(now, crate::endpoint::DEFAULT_DEAD_PEER_COOLDOWN),
                "peer should NOT be marked dead when idle expires with no inflight packets"
            );
        }
        .primary()
        .spawn();
    });
}

/// When a recv context goes idle, the peer must NOT be marked dead. The recv side
/// has no way to distinguish "peer stopped sending because it has nothing to say"
/// from "peer is actually dead". Only the send side (via unacknowledged inflight
/// packets) has evidence of a dead peer.
#[test]
fn recv_idle_wheel_does_not_mark_dead() {
    sim(|| {
        let (recv_cache, mut idle_wheel_tx, clock, _registry) = setup_recv();

        let ctx = RecvContextBuilder::default().build();
        ctx.borrow()
            .path_entry
            .touch_activity(precision::Clock::now(&clock));
        let pse = ctx.borrow().path_entry.clone();
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
            // Idle timeout is 30s. Sleep just past it so the wheel fires,
            // but stay within the 30s cooldown window to detect the mark.
            31.s().sleep().await;

            // The recv context should be evicted.
            assert!(
                recv_cache.borrow().senders.is_empty(),
                "recv context should be evicted after idle timeout"
            );

            // Check immediately — well within the cooldown window.
            // The peer must NOT be marked dead from the recv side.
            let now = precision::Clock::now(&Clock::default());
            assert!(
                !pse.is_dead_during_cooldown(now, crate::endpoint::DEFAULT_DEAD_PEER_COOLDOWN),
                "peer should NOT be marked dead when recv idle expires — \
                 only the send side with inflight packets can determine peer death"
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
) {
    let registry = crate::counter::Registry::default();
    let clock = Clock::default();
    let recv_cache = Rc::new(RefCell::new(recv::Cache::new(
        crate::endpoint::id::RecvDispatchWorkerId::new(0),
    )));

    let (idle_wheel_tx, idle_wheel_rx) = unsync::new_with_adapter::<recv::IdleWheelAdapter>();
    let q_gauge = registry.register_queue_gauge("test.recv_idle_wheel");

    tasks::recv_idle_wheel_drain(
        idle_wheel_rx,
        idle_wheel_tx.clone(),
        clock.clone(),
        q_gauge,
        recv_cache.clone(),
        registry.register("idle.recv.expired"),
        registry.register("idle.recv.rescheduled"),
        registry.register_nominal_timer("idle.recv.lifetime", "recv.0"),
        32,
        registry.register_nominal_task("task.recv_idle_wheel", "recv.0"),
    )
    .spawn();

    (recv_cache, idle_wheel_tx, clock, registry)
}

#[test]
fn recv_idle_wheel_expires_inactive_context() {
    sim(|| {
        let (recv_cache, mut idle_wheel_tx, clock, _registry) = setup_recv();

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
            31.s().sleep().await;
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
        let (recv_cache, mut idle_wheel_tx, clock, _registry) = setup_recv();

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
            // Touch activity at T=1s. The reschedule anchors to last_activity,
            // so the context must survive until last_activity + idle_timeout = 1s + 30s = T=31s.
            1.s().sleep().await;
            let ctx = recv_cache.borrow().senders.values().next().unwrap().clone();
            ctx.borrow()
                .path_entry
                .touch_activity(precision::Clock::now(&Clock::default()));

            // At T=25s the context should still be alive (expires at T=31s)
            24.s().sleep().await;
            assert_eq!(
                recv_cache.borrow().senders.len(),
                1,
                "recv context should still be alive before last_activity + idle_timeout"
            );

            // At T=35s (past 31s) it should be evicted
            10.s().sleep().await;
            assert!(
                recv_cache.borrow().senders.is_empty(),
                "recv context should be evicted after last_activity + idle_timeout"
            );
        }
        .primary()
        .spawn();
    });
}

/// Reproduction: a packet sent shortly before the idle wheel fires (e.g. at t=25s with
/// idle_timeout=30s) should NOT cause the peer to be marked dead. The inflight packet is
/// only 6s old — the peer hasn't had time to respond. Currently, `is_peer_idle` only
/// checks `last_peer_activity` (set at context creation or last ACK), so it incorrectly
/// declares the peer dead despite the recent send.
#[test]
fn send_idle_wheel_defers_death_for_recent_inflight() {
    let _guard = crate::testing::without_snapshots();
    sim(|| {
        let (send_caches, mut idle_wheel_tx, _completed_rx, clock, _registry) = setup_send();

        let pse = test_entry();
        pse.touch_activity(precision::Clock::now(&clock));

        let ctx = send_caches[LocalSendSocketId::new(0)]
            .borrow_mut()
            .get_or_insert(&pse, &clock)
            .unwrap();

        // Schedule into the idle wheel: fires at t=30s
        {
            let mut c = ctx.borrow_mut();
            let timeout = c.path_secret_entry.idle_timeout();
            c.idle_wheel.target_time = Some(precision::Clock::now(&clock) + timeout);
        }
        let _ = idle_wheel_tx.send(ctx.clone());

        let send_caches = send_caches.clone();
        let pse = pse.clone();
        async move {
            // At t=25s: simulate a packet being sent (empty→non-empty inflight).
            // The fix resets last_peer_activity to now when transitioning from no
            // inflight to inflight, effectively restarting the idle timeout.
            25.s().sleep().await;
            {
                let mut ctx = ctx.borrow_mut();
                let now = clock.get_time();
                let rtt = ctx.rtt_estimator;
                let cc_info = ctx.cca.on_packet_sent(now, 200, false, &rtt);
                let mut frames = Queue::new();
                frames.push_back(test_frame(&pse));
                let pn = PacketNumberSpace::Initial.new_packet_number(VarInt::ZERO);

                // Simulate what the assembler does after the fix: restart idle on first send.
                if !ctx.inflight.has_inflight() {
                    ctx.last_peer_activity = precision::Clock::now(&clock);
                }

                ctx.inflight.insert(
                    pn,
                    Packet::new(
                        frames,
                        TransmissionInfo {
                            cc_info,
                            time_sent: now,
                            sent_bytes: 200,
                        },
                    ),
                );
            }

            // At t=31s: idle wheel fires (past original 30s target). But with the fix,
            // last_peer_activity was reset to t=25s, so is_peer_idle returns false
            // (only 6s elapsed < 30s timeout). The context is rescheduled.
            6.s().sleep().await;
            assert_eq!(
                send_caches[LocalSendSocketId::new(0)]
                    .borrow()
                    .context_count(),
                1,
                "context should still be alive — inflight packet is only 6s old"
            );
            let now = precision::Clock::now(&Clock::default());
            assert!(
                !pse.is_dead_during_cooldown(now, crate::endpoint::DEFAULT_DEAD_PEER_COOLDOWN),
                "peer should NOT be marked dead when inflight is only 6s old"
            );

            // At t=56s: rescheduled target was t=55s (last_peer_activity=25s + timeout=30s).
            // Now is_peer_idle returns true (31s elapsed >= 30s). Peer marked dead.
            25.s().sleep().await;
            assert_eq!(
                send_caches[LocalSendSocketId::new(0)]
                    .borrow()
                    .context_count(),
                0,
                "context should be evicted after inflight waited idle_timeout"
            );
            let now = precision::Clock::now(&Clock::default());
            assert!(
                pse.is_dead_during_cooldown(now, crate::endpoint::DEFAULT_DEAD_PEER_COOLDOWN),
                "peer should be marked dead after inflight waited full idle_timeout"
            );
        }
        .primary()
        .spawn();
    });
}

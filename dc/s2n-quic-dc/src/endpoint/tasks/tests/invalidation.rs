// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::helpers::{test_entry, RecvContextBuilder, TestReceiverExt};
use crate::{
    credentials,
    endpoint::{
        frame::{self, Frame},
        id::{Id, IdMap, LocalSendSocketId, LocalSenderId},
        recv, send, tasks,
    },
    intrusive::Entry,
    socket::channel::intrusive::unsync,
    testing::{ext::*, sim},
    time::{bach::Clock, precision},
};
use s2n_quic_core::varint::VarInt;
use std::{cell::RefCell, rc::Rc, sync::Arc};

fn invalidation_counters() -> tasks::SendInvalidationCounters {
    let registry = crate::counter::Registry::default();
    tasks::SendInvalidationCounters {
        unknown_path_secret_events: registry.register("test.invalidation.ups.events"),
        unknown_path_secret_contexts: registry.register("test.invalidation.ups.contexts"),
        unknown_path_secret_frames_failed: registry.register("test.invalidation.ups.frames_failed"),
        stale_or_replay_events: registry.register("test.invalidation.stale_replay.events"),
        stale_or_replay_contexts: registry.register("test.invalidation.stale_replay.contexts"),
        stale_or_replay_frames_requeued: registry
            .register("test.invalidation.stale_replay.frames_requeued"),
    }
}

// ── Send setup ──────────────────────────────────────────────────────────────

struct SendSetup {
    send_caches: IdMap<LocalSendSocketId, Rc<RefCell<send::Cache>>>,
    pse: Arc<crate::path::secret::map::Entry>,
}

fn setup_send() -> SendSetup {
    let registry = crate::counter::Registry::default();
    let clock = Clock::default();
    let send_caches: IdMap<LocalSendSocketId, _> = vec![Rc::new(RefCell::new(send::Cache::new(
        &registry,
        LocalSenderId::from_index(0),
    )))]
    .into();

    let pse = test_entry();
    pse.touch_activity(precision::Clock::now(&clock));

    let _ctx = send_caches[LocalSendSocketId::new(0)]
        .borrow_mut()
        .get_or_insert(&pse, &clock)
        .unwrap();

    SendSetup { send_caches, pse }
}

fn test_frame(pse: &Arc<crate::path::secret::map::Entry>) -> Entry<Frame> {
    Entry::new(Frame {
        header: frame::Header::QueueData {
            queue_pair: crate::packet::datagram::QueuePair {
                source_queue_id: VarInt::from_u8(1),
                dest_queue_id: VarInt::from_u8(2),
            },
            binding_id: VarInt::from_u8(1),
            offset: VarInt::ZERO,
            is_fin: false,
            dest_acceptor_id: None,
        },
        source_sender_id: LocalSenderId::new(VarInt::from_u8(0)),
        payload: Default::default(),
        path_secret_entry: pse.clone(),
        completion: None,
        status: frame::TransmissionStatus::Pending,
        ttl: 3,
        transmission_time: None,
    })
}

// ── Recv setup ──────────────────────────────────────────────────────────────

fn setup_recv() -> (Rc<RefCell<recv::Cache>>, credentials::Id) {
    let recv_cache = Rc::new(RefCell::new(recv::Cache::new(
        crate::endpoint::id::RecvDispatchWorkerId::new(0),
    )));

    let ctx_a = RecvContextBuilder::default()
        .remote_sender_id(VarInt::from_u8(0))
        .build();
    let ctx_b = RecvContextBuilder::default()
        .remote_sender_id(VarInt::from_u8(1))
        .build();

    let id = *ctx_a.borrow().path_entry.id();
    let key_a = recv::Key {
        id,
        remote_sender_id: crate::endpoint::id::RemoteSenderId::new(VarInt::from_u8(0)),
    };
    let key_b = recv::Key {
        id,
        remote_sender_id: crate::endpoint::id::RemoteSenderId::new(VarInt::from_u8(1)),
    };

    recv_cache.borrow_mut().senders.insert(key_a, ctx_a);
    recv_cache.borrow_mut().senders.insert(key_b, ctx_b);

    (recv_cache, id)
}

// ── Send invalidation tests ─────────────────────────────────────────────────

#[test]
fn send_invalidation_purges_cache_and_emits_failed_frames() {
    sim(|| {
        let SendSetup { send_caches, pse } = setup_send();

        {
            let ctx = send_caches[LocalSendSocketId::new(0)]
                .borrow()
                .get(pse.id())
                .unwrap();
            ctx.borrow_mut().queues[1].push_back(test_frame(&pse));
        }

        let id = *pse.id();
        let (cancelled_tx, mut collected_rx) = unsync::new::<Frame>();
        let (retransmit_tx, mut retransmit_rx) = unsync::new::<Frame>();

        let invalidation_rx = super::helpers::TestReceiver::new(vec![Entry::new(
            tasks::Invalidation::UnknownPathSecret { credential_id: id },
        )]);
        let mut rx = tasks::send_invalidation(
            invalidation_rx,
            send_caches.clone(),
            IdMap::<LocalSenderId, LocalSendSocketId>::new(1, LocalSendSocketId::new(0)),
            cancelled_tx,
            retransmit_tx,
            invalidation_counters(),
        );

        async move {
            rx.recv().await;
            drop(rx);

            assert_eq!(
                send_caches[LocalSendSocketId::new(0)]
                    .borrow()
                    .context_count(),
                0,
                "cache should be empty after invalidation"
            );

            let frame = collected_rx
                .recv()
                .await
                .expect("one frame should have been emitted");
            assert_eq!(
                frame.status,
                frame::TransmissionStatus::Failed(frame::FailureReason::UnknownPathSecret),
            );
            assert!(
                collected_rx.recv().await.is_none(),
                "only one failed frame should be emitted"
            );
            assert!(retransmit_rx.recv().await.is_none());
        }
        .primary()
        .spawn();
    });
}

#[test]
fn send_invalidation_noop_for_unknown_id() {
    sim(|| {
        let SendSetup { send_caches, .. } = setup_send();

        let fake_id = credentials::Id::from([0xAA; 16]);
        let (cancelled_tx, mut collected_rx) = unsync::new::<Frame>();
        let (retransmit_tx, mut retransmit_rx) = unsync::new::<Frame>();

        let invalidation_rx = super::helpers::TestReceiver::new(vec![Entry::new(
            tasks::Invalidation::UnknownPathSecret {
                credential_id: fake_id,
            },
        )]);
        let mut rx = tasks::send_invalidation(
            invalidation_rx,
            send_caches.clone(),
            IdMap::<LocalSenderId, LocalSendSocketId>::new(1, LocalSendSocketId::new(0)),
            cancelled_tx,
            retransmit_tx,
            invalidation_counters(),
        );

        async move {
            rx.recv().await;
            drop(rx);

            assert_eq!(
                send_caches[LocalSendSocketId::new(0)]
                    .borrow()
                    .context_count(),
                1,
                "unrelated context should remain"
            );
            assert!(
                collected_rx.recv().await.is_none(),
                "no frames should be emitted"
            );
            assert!(retransmit_rx.recv().await.is_none());
        }
        .primary()
        .spawn();
    });
}

// ── Recv invalidation tests ─────────────────────────────────────────────────

#[test]
fn recv_invalidation_removes_matching_entries() {
    sim(|| {
        let (recv_cache, id) = setup_recv();
        assert_eq!(recv_cache.borrow().senders.len(), 2);

        let invalidation_rx = super::helpers::TestReceiver::new(vec![Entry::new(
            tasks::Invalidation::UnknownPathSecret { credential_id: id },
        )]);
        let mut rx = tasks::recv_invalidation(invalidation_rx, recv_cache.clone());

        async move {
            rx.recv().await;

            assert_eq!(
                recv_cache.borrow().senders.len(),
                0,
                "all entries with the invalidated ID should be removed"
            );
        }
        .primary()
        .spawn();
    });
}

#[test]
fn recv_invalidation_preserves_unrelated_entries() {
    sim(|| {
        let (recv_cache, _id) = setup_recv();

        let fake_id = credentials::Id::from([0xBB; 16]);
        let invalidation_rx = super::helpers::TestReceiver::new(vec![Entry::new(
            tasks::Invalidation::UnknownPathSecret {
                credential_id: fake_id,
            },
        )]);
        let mut rx = tasks::recv_invalidation(invalidation_rx, recv_cache.clone());

        async move {
            rx.recv().await;

            assert_eq!(
                recv_cache.borrow().senders.len(),
                2,
                "unrelated entries should be preserved"
            );
        }
        .primary()
        .spawn();
    });
}

#[test]
fn ack_completion_after_recv_invalidation_does_not_resurrect_context() {
    sim(|| {
        let (recv_cache, id) = setup_recv();

        let submission = {
            let ctx = recv_cache.borrow().senders.values().next().unwrap().clone();
            let c = ctx.borrow();
            crate::endpoint::ack::state::Submission {
                body: bytes::Bytes::from_static(&[1]),
                largest_recv_time: precision::Clock::now(&Clock::default()),
                has_ecn: false,
                path_secret_entry: c.path_entry.clone(),
                local_sender_id: c.local_sender_id,
                remote_sender_id: c.remote_sender_id,
                recv_worker_id: crate::endpoint::id::RecvDispatchWorkerId::new(0),
            }
        };

        let invalidation_rx = super::helpers::TestReceiver::new(vec![Entry::new(
            tasks::Invalidation::UnknownPathSecret { credential_id: id },
        )]);
        let mut invalidation = tasks::recv_invalidation(invalidation_rx, recv_cache.clone());

        let completion_rx = super::helpers::TestReceiver::new(vec![Entry::new(
            crate::endpoint::msg::Sender::PendingAck(submission),
        )]);
        let (sender, mut collected) = unsync::new::<crate::endpoint::msg::Sender>();
        let counters =
            crate::endpoint::counters::Dispatch::new(&crate::counter::Registry::default());
        let mut completion =
            tasks::ack_completion(completion_rx, recv_cache.clone(), sender, counters);

        async move {
            invalidation.recv().await;
            completion.recv().await;
            drop(completion);
            assert!(recv_cache.borrow().senders.is_empty());
            assert!(collected.recv().await.is_none());
        }
        .primary()
        .spawn();
    });
}

#[test]
fn ack_burst_after_recv_invalidation_emits_nothing() {
    sim(|| {
        let (recv_cache, id) = setup_recv();
        let ctx = recv_cache.borrow().senders.values().next().unwrap().clone();

        let invalidation_rx = super::helpers::TestReceiver::new(vec![Entry::new(
            tasks::Invalidation::UnknownPathSecret { credential_id: id },
        )]);
        let mut invalidation = tasks::recv_invalidation(invalidation_rx, recv_cache.clone());

        let ack_burst_rx = super::helpers::TestReceiver::new(vec![ctx]);
        let (sender, mut collected) = unsync::new::<crate::endpoint::msg::Sender>();
        let counters =
            crate::endpoint::counters::Dispatch::new(&crate::counter::Registry::default());
        let mut ack_burst = tasks::ack_burst(
            ack_burst_rx,
            sender,
            crate::endpoint::id::RecvDispatchWorkerId::new(0),
            counters,
        );

        async move {
            invalidation.recv().await;
            ack_burst.recv().await;
            drop(ack_burst);
            assert!(recv_cache.borrow().senders.is_empty());
            assert!(collected.recv().await.is_none());
        }
        .primary()
        .spawn();
    });
}

#[test]
fn send_invalidation_stale_key_targets_matching_sender_only() {
    sim(|| {
        let registry = crate::counter::Registry::default();
        let clock = Clock::default();
        let send_caches: IdMap<LocalSendSocketId, _> = vec![
            Rc::new(RefCell::new(send::Cache::new(
                &registry,
                LocalSenderId::from_index(0),
            ))),
            Rc::new(RefCell::new(send::Cache::new(
                &registry,
                LocalSenderId::from_index(1),
            ))),
        ]
        .into();

        let pse = test_entry();
        pse.touch_activity(precision::Clock::now(&clock));

        for (_id, cache) in &send_caches {
            let _ctx = cache.borrow_mut().get_or_insert(&pse, &clock).unwrap();
            let ctx = cache.borrow().get(pse.id()).unwrap();
            ctx.borrow_mut().queues[1].push_back(test_frame(&pse));
        }

        let id = *pse.id();
        let (cancelled_tx, mut collected_rx) = unsync::new::<Frame>();
        let (retransmit_tx, mut retransmit_rx) = unsync::new::<Frame>();
        let invalidation_rx =
            super::helpers::TestReceiver::new(vec![Entry::new(tasks::Invalidation::StaleKey {
                credential_id: id,
                sender_id: LocalSenderId::new(VarInt::from_u8(1)),
                // Cache[1]'s context uses key_id=1 (second call to next_key_id),
                // so rejected_key_id must be >= 1 to trigger invalidation.
                rejected_key_id: VarInt::from_u8(1),
            })]);
        let mut rx = tasks::send_invalidation(
            invalidation_rx,
            send_caches.clone(),
            {
                let mut m = IdMap::<LocalSenderId, LocalSendSocketId>::new(
                    2,
                    LocalSendSocketId::new(usize::MAX),
                );
                m[LocalSenderId::from_index(0)] = LocalSendSocketId::new(0);
                m[LocalSenderId::from_index(1)] = LocalSendSocketId::new(1);
                m
            },
            cancelled_tx,
            retransmit_tx,
            invalidation_counters(),
        );

        async move {
            rx.recv().await;
            drop(rx);

            assert_eq!(
                send_caches[LocalSendSocketId::new(0)]
                    .borrow()
                    .context_count(),
                1
            );
            assert_eq!(
                send_caches[LocalSendSocketId::new(1)]
                    .borrow()
                    .context_count(),
                0
            );

            let frame = retransmit_rx
                .recv()
                .await
                .expect("one stale-key invalidated frame should be retransmitted");
            assert_eq!(frame.status, frame::TransmissionStatus::Pending);
            assert!(retransmit_rx.recv().await.is_none());
            assert!(collected_rx.recv().await.is_none());
        }
        .primary()
        .spawn();
    });
}

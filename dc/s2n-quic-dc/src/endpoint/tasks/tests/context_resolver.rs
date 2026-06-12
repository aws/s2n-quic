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
    credit::{Config, Distributor, Pool, Priority},
    endpoint::{
        combinator::FrameBatch,
        frame::{self, Frame, Header},
        id::{Id, IdMap, LocalSendSocketId, LocalSenderId},
        send, tasks,
    },
    intrusive::Entry,
    packet::datagram::QueuePair,
    path::secret::map::Entry as PathSecretEntry,
    socket::channel::{intrusive::unsync, ReceiverExt as _, UnboundedSender as _},
    testing::{ext::*, sim},
    time::bach::Clock,
};
use s2n_quic_core::varint::VarInt;
use std::{cell::RefCell, net::SocketAddr, rc::Rc, sync::Arc};

type TxWheelRx = unsync::Receiver<send::TxWheelAdapter>;
type BatchTx = unsync::Sender<crate::intrusive::EntryAdapter<FrameBatch>>;

struct Harness {
    batch_tx: BatchTx,
    tx_wheel_rx: TxWheelRx,
    send_caches: IdMap<LocalSendSocketId, Rc<RefCell<send::Cache>>>,
}

/// Spawns the context_resolver pipeline and returns a harness for driving it.
fn setup() -> Harness {
    let registry = crate::counter::Registry::default();
    let send_caches: IdMap<LocalSendSocketId, _> = vec![Rc::new(RefCell::new(send::Cache::new(
        &registry,
        LocalSenderId::from_index(0),
    )))]
    .into();
    let sender_idx_to_local =
        IdMap::<LocalSenderId, LocalSendSocketId>::new(1, LocalSendSocketId::new(0));

    let (immediate_tx_raw, _immediate_rx) = unsync::new_with_adapter::<send::TxImmediateAdapter>();
    let socket_immediate_txs: IdMap<LocalSendSocketId, _> =
        core::iter::once((LocalSendSocketId::new(0), immediate_tx_raw)).collect();
    let mut sender_idx_to_local_imm: IdMap<LocalSenderId, LocalSendSocketId> =
        IdMap::new(1, LocalSendSocketId::new(0));
    sender_idx_to_local_imm[LocalSenderId::from_index(0)] = LocalSendSocketId::new(0);
    let immediate_tx = send::ImmediateSender::new(socket_immediate_txs, sender_idx_to_local_imm);

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
        immediate_tx,
        tx_wheel_tx,
        pto_wheel_tx,
        idle_wheel_tx,
        Arc::new(Pool::new(Config::new(1_000_000))),
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

        let cache_ref = send_caches[LocalSendSocketId::new(0)].clone();
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
                LocalSenderId::from_index(0),
                &Clock::default(),
            )
            .unwrap();
            Rc::new(RefCell::new(ctx))
        };

        let interests = [
            send::WheelInterest {
                immediate: false,
                transmission: false,
                pto: false,
                idle_timeout: false,
            },
            send::WheelInterest {
                immediate: false,
                transmission: true,
                pto: false,
                idle_timeout: false,
            },
            send::WheelInterest {
                immediate: false,
                transmission: false,
                pto: true,
                idle_timeout: false,
            },
            send::WheelInterest {
                immediate: false,
                transmission: false,
                pto: false,
                idle_timeout: true,
            },
            send::WheelInterest {
                immediate: false,
                transmission: true,
                pto: true,
                idle_timeout: false,
            },
            send::WheelInterest {
                immediate: false,
                transmission: true,
                pto: false,
                idle_timeout: true,
            },
            send::WheelInterest {
                immediate: false,
                transmission: false,
                pto: true,
                idle_timeout: true,
            },
            send::WheelInterest {
                immediate: false,
                transmission: true,
                pto: true,
                idle_timeout: true,
            },
        ];
        let input = TestReceiver::new(interests.into_iter().map(|i| (make_context(), i)));
        let (tx_sender, mut tx_items) = unsync::new_with_adapter::<send::TxWheelAdapter>();
        let (pto_sender, mut pto_items) = unsync::new_with_adapter::<send::PtoWheelAdapter>();
        let (idle_sender, mut idle_items) = unsync::new_with_adapter::<send::IdleWheelAdapter>();
        let (imm_sender, mut imm_items) = unsync::new_with_adapter::<send::TxImmediateAdapter>();
        let mut router =
            send::WheelRouter::new(input, imm_sender, tx_sender, pto_sender, idle_sender);

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
            let mut imm = 0usize;
            while imm_items.recv().await.is_some() {
                imm += 1;
            }
            assert_eq!(tx, 4);
            assert_eq!(pto, 4);
            assert_eq!(idle, 4);
            // None of the test interests have immediate=true, so the immediate
            // channel must receive zero items.
            assert_eq!(imm, 0);
        }
        .primary()
        .spawn();
    });
}

/// A PathSecretEntry whose peer data addresses have NOT been exchanged yet, so
/// `send::Context::new` fails with `ContextError::PeerDataAddrsNotReady`. This is the
/// realistic state for the very first batch(es) routed to a freshly-handshaked peer.
fn entry_without_data_addrs() -> Arc<PathSecretEntry> {
    let addr: SocketAddr = "127.0.0.1:4433".parse().unwrap();
    // Note: deliberately do NOT call `set_peer_data_addrs`.
    PathSecretEntry::builder(addr)
        .socket_sender_count(8)
        .build()
}

/// A single-frame `FrameBatch` whose frame carries `flow_credits` borrowed from the send
/// credit pool, addressed to `pse`. Mirrors what the Writer produces once it has acquired
/// credit (`stream/writer.rs` `take_credits` → `Frame.flow_credits`).
fn credit_carrying_batch(pse: &Arc<PathSecretEntry>, flow_credits: u64) -> Entry<FrameBatch> {
    let frame = Entry::new(Frame {
        header: Header::QueueData {
            queue_pair: QueuePair {
                source_queue_id: VarInt::from_u8(1),
                dest_queue_id: VarInt::from_u8(2),
            },
            binding_id: VarInt::from_u8(1),
            offset: VarInt::ZERO,
            largest_offset: VarInt::ZERO,
            is_fin: false,
            blocked: false,
            dest_acceptor_id: None,
            priority: Priority::default(),
        },
        payload: Default::default(),
        path_secret_entry: pse.clone(),
        completion: None,
        status: frame::TransmissionStatus::Pending,
        ttl: 3,
        enqueued_at: None,
        flow_credits,
    });
    let mut batch = FrameBatch::single(frame);
    batch.set_sender_id(LocalSenderId::from_index(0));
    Entry::new(batch)
}

/// Regression: a credit-bearing batch routed to a peer whose send context cannot be
/// created (peer data addrs not yet exchanged) is dropped by `context_resolver`, and its
/// `flow_credits` MUST be released back to the send credit pool.
///
/// `Frame` has no `Drop` impl that releases `flow_credits`; release is wired into the
/// assembler admit path, the cancelled-drain task, and (this fix) the context-not-ready
/// reject path. Without releasing here the borrowed credit leaks for the lifetime of the
/// endpoint: this reject path is taken for the first batch(es) to a freshly-handshaked
/// peer, so under sustained connection churn the shared pool slowly drains and eventually
/// stalls every writer.
#[test]
fn context_not_ready_releases_flow_credits() {
    sim(|| {
        const CAP: u64 = 1_000_000;

        // A real pool + distributor, exactly as the endpoint wires them.
        let pool = Arc::new(Pool::new(Config::new(CAP)));
        let distributor = Distributor::new(pool.clone());

        // Borrow credit from the pool via the fast path, as the Writer would before
        // attaching it to a frame. The pool clamps to `max_single_acquire`, so the grant
        // may be smaller than asked; attach exactly what we got. Capacity is plentiful so
        // this is an immediate (non-parking) grant.
        let credit = {
            let waker = s2n_quic_core::task::waker::noop();
            let mut cx = core::task::Context::from_waker(&waker);
            let slot = Box::leak(Box::new(crate::credit::Slot::new(drop_noop_slot)));
            let slot_ptr = std::ptr::NonNull::from(&*slot);
            match unsafe { pool.poll_acquire(&mut cx, slot_ptr, CAP, Priority::default()) } {
                core::task::Poll::Ready(n) => n,
                core::task::Poll::Pending => {
                    panic!("fast-path acquire should succeed with ample capacity")
                }
            }
        };
        assert!(credit > 0, "expected a non-zero fast-path grant");
        // The credit is now out of the pool, held by the frame we are about to route.
        assert_eq!(
            pool.debug_available() as u64 + pool.debug_returned(),
            CAP - credit,
            "credit should be debited from the pool after acquire"
        );

        // Wire up context_resolver exactly like `setup()` does.
        let registry = crate::counter::Registry::default();
        let send_caches: IdMap<LocalSendSocketId, _> = vec![Rc::new(RefCell::new(
            send::Cache::new(&registry, LocalSenderId::from_index(0)),
        ))]
        .into();
        let sender_idx_to_local =
            IdMap::<LocalSenderId, LocalSendSocketId>::new(1, LocalSendSocketId::new(0));

        let (immediate_tx_raw, _immediate_rx) =
            unsync::new_with_adapter::<send::TxImmediateAdapter>();
        let socket_immediate_txs: IdMap<LocalSendSocketId, _> =
            core::iter::once((LocalSendSocketId::new(0), immediate_tx_raw)).collect();
        let mut sender_idx_to_local_imm: IdMap<LocalSenderId, LocalSendSocketId> =
            IdMap::new(1, LocalSendSocketId::new(0));
        sender_idx_to_local_imm[LocalSenderId::from_index(0)] = LocalSendSocketId::new(0);
        let immediate_tx =
            send::ImmediateSender::new(socket_immediate_txs, sender_idx_to_local_imm);

        let (tx_wheel_tx, _tx_wheel_rx) = unsync::new_with_adapter::<send::TxWheelAdapter>();
        let (pto_wheel_tx, _) = unsync::new_with_adapter::<send::PtoWheelAdapter>();
        let (idle_wheel_tx, _) = unsync::new_with_adapter::<send::IdleWheelAdapter>();

        let (mut batch_tx, batch_rx) = unsync::new::<FrameBatch>();

        let rx = tasks::context_resolver(
            batch_rx,
            send_caches.clone(),
            sender_idx_to_local,
            1,
            Clock::default(),
            immediate_tx,
            tx_wheel_tx,
            pto_wheel_tx,
            idle_wheel_tx,
            pool.clone(),
        );
        async move { rx.drain_budgeted(Some(32)).await }
            .primary()
            .spawn();

        // Route the credit-bearing batch to a peer with no data addrs → context can't be
        // created → batch dropped.
        let pse = entry_without_data_addrs();
        let _ = batch_tx.send(credit_carrying_batch(&pse, credit));
        drop(batch_tx);

        let pool = pool.clone();
        async move {
            // Let the resolver process and drop the batch.
            bach::time::sleep(core::time::Duration::from_millis(10)).await;

            // No context was created (the peer never became ready).
            assert_eq!(
                send_caches[LocalSendSocketId::new(0)]
                    .borrow()
                    .context_count(),
                0,
                "context must not exist for an unready peer"
            );

            // The credit the dropped frame was holding must have been returned to the pool.
            let recovered = pool.debug_available() as u64 + pool.debug_returned();
            assert_eq!(
                recovered,
                CAP,
                "send-credit leak: {} bytes of flow_credits were dropped with the batch and \
                 never released back to the pool (have {}, expected {})",
                CAP - recovered,
                recovered,
                CAP
            );

            drop(distributor);
        }
        .primary()
        .spawn();
    });
}

unsafe fn drop_noop_slot(_ptr: std::ptr::NonNull<crate::credit::Slot>) {}

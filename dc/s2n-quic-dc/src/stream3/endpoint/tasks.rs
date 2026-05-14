// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    clock::precision,
    datagram::batch::Priority,
    intrusive_queue::{Entry, Queue},
    socket::{
        channel::{
            intrusive_queue::{self, unsync},
            FlattenList, FlattenSegments, InspectErr, Map, Paced, Priority as PriorityRx, Receiver,
            ReceiverExt as _, RouterAdapter, SocketReceiver, SocketSender, UnboundedSender,
        },
        pool::descriptor,
        rate::Rate,
    },
    spawner::LocalSpawner,
    stream3::{
        endpoint::{
            self,
            combinator::{
                AckProcessor, Assembler, AssemblerCounters, BatchFramesByPathSecret,
                CompletionDispatcher, FrameBatch, PathSecretMapEntry, PickTwo,
            },
            dispatch, msg, send, Budgets,
        },
        frame::{Frame, PriorityStorage, SubmissionReceiver},
    },
};
use core::{future::poll_fn, task::Poll};
use s2n_quic_core::{assume, varint::VarInt};
use std::{cell::RefCell, rc::Rc, sync::Arc};

/// Default per-poll budget for [`socket_recv_task`]: process up to this many segments before
/// yielding to the executor. Tune via the `budget` parameter if workloads differ.
pub const DEFAULT_RECV_BUDGET: usize = 32;

/// Default per-poll budget for [`packet_dispatch_task`]: process up to this many packets before
/// yielding to the executor. Tune via the `budget` parameter if workloads differ.
pub const DEFAULT_DISPATCH_BUDGET: usize = 32;

// ── Pipeline Task Functions ────────────────────────────────────────────────

/// Routes frame submissions to socket workers using priority queues, pacing, and pick-two
/// load balancing.
///
/// Creates two cooperating tasks on `spawner`'s worker:
///
/// - **Priority router** (Task 1): on each poll it calls [`poll_swap`] once to atomically
///   receive the next ready shard's [`PriorityStorage`] (a Box pointer swap — O(1)).  It
///   then appends each non-empty priority queue to the corresponding per-priority unsync
///   [`ListSender`] in O([`Priority::LEVELS`]) work.  After processing one shard it yields
///   to the executor (one shard per poll).  A pre-allocated `staging` Box is reused across
///   swaps — no heap allocation on the hot path.
///
/// - **Batcher + Distributor** (Task 2): each per-priority unsync receiver is independently
///   wrapped in [`BatchFramesByPathSecret`] to coalesce frames for the same peer into
///   datagram-sized batches. The resulting per-priority [`Receiver<FrameBatch>`] streams are
///   merged in urgency order by [`channel::Priority`], overall bandwidth is throttled by
///   [`channel::Paced`], and each batch is routed to a send socket via [`pick_two`].
///
/// # Why prioritize before batching
///
/// Prioritizing individual frames before coalescing into batches ensures that frames for the
/// same peer are properly separated by priority class. If batching ran first, a single
/// `FrameBatch` might mix ACK frames (high priority) with data frames (low priority), and
/// the batch would only be routed to one priority lane based on the first frame.
/// Pre-prioritization means every `FrameBatch` that emerges from a lane is homogeneous in
/// priority class.
///
/// # Fixed-cost routing
///
/// Senders submit [`PriorityInput`] values (stack-allocated), which are merged into the
/// shard's Box-backed [`PriorityStorage`] at submission time (O([`Priority::LEVELS`])
/// appends). Task 1 pointer-swaps the Box in O(1) and then distributes the queues to the
/// per-priority unsync lanes in one O([`Priority::LEVELS`]) pass.
///
/// # Pipeline overview
///
/// ```text
/// Task 1 (priority router):
///   SubmissionReceiver
///     → poll_swap (O(1) pointer swap of PriorityStorage Box)
///     → drain staging into per-priority ListSenders (O(Priority::LEVELS))
///     → yield (one shard per poll)
///
/// Task 2 (batcher + distributor):
///   [per-priority unsync rx[i] → BatchFramesByPathSecret]
///     → Priority (urgency-ordered merge)
///     → Paced (overall bandwidth cap)
///     → pick_two (send-socket routing)
/// ```
///
/// # Sticky routing and queue metrics
///
/// Sticky routing (retransmissions to the same socket) and per-queue depth gauges are not
/// yet implemented. See stream2's dispatch pipeline for the reference implementation.
///
/// [`poll_swap`]: crate::socket::channel::intrusive_queue::sharded::Receiver::poll_swap
/// [`ListSender`]: crate::socket::channel::intrusive_queue::unsync::ListSender
/// [`channel::Priority`]: crate::socket::channel::Priority
/// [`channel::Paced`]: crate::socket::channel::Paced
/// [`Priority::LEVELS`]: crate::datagram::batch::Priority::LEVELS
/// [`PriorityStorage`]: crate::stream3::frame::PriorityStorage
/// [`PriorityInput`]: crate::stream3::frame::PriorityInput
pub fn frame_dispatch<S, Clk>(
    spawner: &mut impl LocalSpawner,
    mut frame_rx: SubmissionReceiver,
    worker_senders: Vec<S>,
    rng: crate::xorshift::Rng,
    clock: Clk,
    overall_send_rate: Rate,
    budgets: Budgets,
    counter_registry: crate::counter::Registry,
) where
    S: UnboundedSender<Entry<FrameBatch>> + 'static,
    Clk: precision::Clock + 'static,
{
    let mut priority_batch_rxs = Vec::with_capacity(Priority::LEVELS);
    let mut priority_list_txs: [_; Priority::LEVELS] = core::array::from_fn(|_| {
        let (tx, rx) = intrusive_queue::unsync::new::<Frame>();
        // let rx = BatchFramesByPathSecret::new(rx);
        // let rx = Map::new(rx, Entry::new);
        priority_batch_rxs.push(rx);
        tx.into_list_sender()
    });

    // Task 1: fixed-cost priority routing.
    let q_frames = counter_registry.register_queue_gauge("q.frames");
    spawner.spawn({
        let q_frames = q_frames.clone();
        let mut staging = PriorityStorage::default();
        poll_fn(move |cx| {
            for _ in 0..budgets.submission_router {
                match frame_rx.poll_swap(cx, &mut staging) {
                    Poll::Ready(None) => return Poll::Ready(()),
                    Poll::Pending => return Poll::Pending,
                    Poll::Ready(Some(())) => {
                        for ((_priority, queue), tx) in staging.drain().zip(&mut priority_list_txs)
                        {
                            if !queue.is_empty() {
                                q_frames.enqueue(queue.len() as u64);
                                let _ = UnboundedSender::send(tx, queue);
                            }
                        }
                    }
                }
            }
            cx.waker().wake_by_ref();
            Poll::Pending
        })
    });

    // Task 2: batch → Entry → priority merge → pace → pick-two to workers.
    spawner.spawn({
        let rx = PriorityRx::new(priority_batch_rxs);
        let rx = crate::counter::GaugedReceiver::new(rx, q_frames);
        let rx = BatchFramesByPathSecret::new(rx, &clock, overall_send_rate);
        let rx = Map::new(rx, Entry::new);
        let rx = Paced::new(rx, clock, overall_send_rate);
        let rx = PickTwo::new(rx, worker_senders, rng);
        rx.drain_budgeted(Some(budgets.frame_dispatch))
    });
}

/// Spawns all send-side tasks for a worker: context resolution, ACK processing, and
/// per-socket assembly+send.
///
/// Pipeline:
///   batch_rx (sync, from PickTwo)
///     → context resolver (resolve per-peer state, push frames)
///     → TODO: tx wheel (pacing/scheduling)
///     → per-socket Assembler → SocketSender
///
///   ack_rx (sync, from recv workers)
///     → ACK processor (loss detection, retransmission)
pub fn send_worker<Socket, Clk, WakerSink, AckComp>(
    spawner: &mut impl LocalSpawner,
    batch_rx: impl Receiver<Entry<FrameBatch>> + 'static,
    ack_rx: impl Receiver<Entry<msg::Sender>> + 'static,
    total_sender_ids: usize,
    send_sockets: Vec<endpoint::SendSocketParts<Socket, Clk>>,
    clock: Clk,
    random: crate::xorshift::Rng,
    frame_tx: crate::stream3::frame::SubmissionSender,
    ack_completions_tx: AckComp,
    mut waker_sink: WakerSink,
    budgets: Budgets,
    counter_registry: crate::counter::Registry,
) where
    Socket: crate::socket::send::Socket + 'static,
    Clk: precision::Clock + s2n_quic_core::time::Clock + Clone + 'static,
    WakerSink: UnboundedSender<crate::flow::queue::AutoWake> + 'static,
    AckComp: UnboundedSender<Queue<msg::Sender>> + Clone + 'static,
{
    let num_sockets = send_sockets.len();

    // Per-socket unsync channel: wheel drain tasks route contexts here after expiration,
    // per-socket assembler+send task drains them.
    let (socket_context_txs, socket_context_rxs): (Vec<_>, Vec<_>) = (0..num_sockets)
        .map(|_| unsync::new_with_adapter::<send::TxWheelAdapter>())
        .unzip();

    // Map sender_idx → local socket position for this worker.
    let mut sender_idx_to_local: Vec<usize> = (0..total_sender_ids).map(|_| usize::MAX).collect();

    // One send::Cache per socket, shared between the context resolver and ACK processor.
    let send_caches: Vec<Rc<RefCell<send::Cache>>> = send_sockets
        .iter()
        .enumerate()
        .map(|(local_id, st)| {
            sender_idx_to_local[st.sender_idx] = local_id;

            Rc::new(RefCell::new(send::Cache::new(
                &counter_registry,
                st.sender_idx,
            )))
        })
        .collect();

    let q_tx_wheel = counter_registry.register_queue_gauge("q.tx_wheel");
    let (tx_wheel_tx, tx_wheel_rx) = unsync::new_with_adapter();
    let (pto_wheel_tx, pto_wheel_rx) = unsync::new_with_adapter();
    let (idle_wheel_tx, idle_wheel_rx) = unsync::new_with_adapter();
    let (completed_tx, completed_rx) = unsync::new::<Frame>();
    let (cancelled_tx, cancelled_rx) = unsync::new::<Frame>();

    // Task 1: context resolver — drain batch_rx, resolve to context, push frames.
    spawner.spawn({
        let mut send_caches = send_caches.clone();
        let sender_idx_to_local = sender_idx_to_local.clone();
        let rx = batch_rx;
        let mut tx_wheel_tx = tx_wheel_tx.clone();
        let mut pto_wheel_tx = pto_wheel_tx.clone();
        let mut idle_wheel_tx = idle_wheel_tx.clone();
        let clock = clock.clone();
        let q_tx_wheel = q_tx_wheel.clone();
        let rx = Map::new(rx, move |batch: Entry<FrameBatch>| {
            let Some(sender_idx) = batch.sender_id() else {
                unsafe {
                    assume!(false, "batch needs an assigned sender id");
                }
            };
            let Some(local_id) = sender_idx_to_local.get(sender_idx).copied() else {
                unsafe {
                    assume!(
                        false,
                        "sender id {} is out of range of {}",
                        sender_idx,
                        total_sender_ids
                    )
                }
            };
            let Some(cache) = send_caches.get_mut(local_id) else {
                unsafe {
                    assume!(
                        false,
                        "sender id {} is out of range of {}",
                        sender_idx,
                        total_sender_ids
                    )
                }
            };

            let sender = {
                let mut cache = cache.borrow_mut();
                let cache = &mut *cache;
                cache.get_or_insert(batch.path_secret_entry())
            };

            let wheel_interest = {
                let mut sender = sender.borrow_mut();
                sender.push_batch(batch.into_inner(), &clock)
            };

            if wheel_interest.transmission {
                q_tx_wheel.enqueue(1);
            }
            wheel_interest.dispatch(
                sender,
                &mut tx_wheel_tx,
                &mut pto_wheel_tx,
                &mut idle_wheel_tx,
            );
        });
        rx.drain_budgeted(Some(budgets.context_resolver))
    });

    // Task 2: ACK processor — decode, update CCA/RTT, detect loss, reschedule.
    spawner.spawn({
        let send_caches = send_caches.clone();
        let sender_idx_to_local = sender_idx_to_local.clone();
        let send_counters = endpoint::counters::Send::new(&counter_registry);
        let rx = AckProcessor::new(
            ack_rx,
            send_caches,
            sender_idx_to_local,
            total_sender_ids,
            clock.clone(),
            random,
            frame_tx,
            completed_tx,
            cancelled_tx.clone(),
            tx_wheel_tx.clone(),
            pto_wheel_tx.clone(),
            idle_wheel_tx.clone(),
            send_counters,
            q_tx_wheel.clone(),
        );
        rx.drain_budgeted(Some(budgets.ack_processor))
    });

    // Task 3: Completion dispatcher — batches completed frames by channel, one lock per batch.
    spawner.spawn({
        let rx = CompletionDispatcher::new(completed_rx);
        let rx = Map::new(rx, move |waker: crate::flow::queue::AutoWake| {
            let _ = waker_sink.send(waker);
        });
        rx.drain_budgeted(Some(budgets.completion_acked))
    });

    // Task 4: Cancelled frame drain — drops frames whose writer is already gone.
    spawner.spawn({
        let rx = Map::new(cancelled_rx, |_entry: Entry<Frame>| {});
        rx.drain_budgeted(Some(budgets.completion_cancelled))
    });

    // Task 5: TX wheel drain — routes expired contexts to per-socket assembler channels.
    spawner.spawn(wheel_drain(
        tx_wheel_rx,
        clock.timer(),
        {
            let sender_idx_to_local = sender_idx_to_local.clone();
            let mut socket_context_txs = socket_context_txs;
            let q_tx_wheel = q_tx_wheel.clone();
            move |context: Rc<RefCell<send::Context>>| {
                q_tx_wheel.dequeue();
                let local_id = sender_idx_to_local[context.borrow().sender_idx];
                let _ = UnboundedSender::send(&mut socket_context_txs[local_id], context);
            }
        },
        budgets.tx_wheel,
    ));

    // Task 6: PTO wheel drain — fires probes for tail loss recovery.
    spawner.spawn(wheel_drain(
        pto_wheel_rx,
        clock.timer(),
        {
            let clock = clock.clone();
            let mut tx_wheel_tx = tx_wheel_tx.clone();
            let mut pto_wheel_tx = pto_wheel_tx.clone();
            let mut idle_wheel_tx = idle_wheel_tx.clone();
            let q_tx_wheel = q_tx_wheel.clone();
            let tx_pto_fired = counter_registry.register("tx.pto_fired");
            let tx_pto_requested = counter_registry.register("tx.pto_requested");
            move |context: Rc<RefCell<send::Context>>| {
                tx_pto_fired.add(1);
                let wheel_interest = {
                    let mut ctx = context.borrow_mut();
                    let requested = ctx.pto.probe_state.is_requested();
                    let interest = ctx.on_pto_timeout(&clock);
                    if !requested && ctx.pto.probe_state.is_requested() {
                        tx_pto_requested.add(1);
                    }
                    interest
                };
                if wheel_interest.transmission {
                    q_tx_wheel.enqueue(1);
                }
                wheel_interest.dispatch(
                    context,
                    &mut tx_wheel_tx,
                    &mut pto_wheel_tx,
                    &mut idle_wheel_tx,
                );
            }
        },
        budgets.pto_wheel,
    ));

    // Task 7: Idle wheel drain — reclaims resources for idle connections.
    spawner.spawn(wheel_drain(
        idle_wheel_rx,
        clock.timer(),
        |context: Rc<RefCell<send::Context>>| {
            // TODO reclaim idle context resources
            let _ = context;
        },
        budgets.idle_wheel,
    ));

    // Per-socket assembler + send tasks.
    let asm_counters = AssemblerCounters::new(&counter_registry);
    for (st, context_rx) in send_sockets.into_iter().zip(socket_context_rxs) {
        let source_sender_id = VarInt::new(st.sender_idx as u64).unwrap();

        spawner.spawn({
            let clock = st.clock.clone();
            let tx_wheel_tx = tx_wheel_tx.clone();
            let pto_wheel_tx = pto_wheel_tx.clone();
            let idle_wheel_tx = idle_wheel_tx.clone();
            let cancelled_tx = cancelled_tx.clone().into_list_sender();
            let ack_completions_tx = ack_completions_tx.clone();
            let asm_counters = asm_counters.clone();
            let rx = Assembler::new(
                context_rx,
                clock.clone(),
                source_sender_id,
                st.source_control_port,
                st.gso,
                st.pool,
                cancelled_tx,
                ack_completions_tx,
                tx_wheel_tx,
                pto_wheel_tx,
                idle_wheel_tx,
                asm_counters,
            );
            let rx = Paced::new(rx, clock, st.per_socket_send_rate);
            let rx = SocketSender::new(rx, st.socket);
            let rx = InspectErr::new(rx, |(err, _segments)| {
                tracing::warn!(%err, "socket send error");
            });
            let rx = Map::new(rx, |_segments| {});
            rx.drain_budgeted(Some(budgets.assembler))
        });
    }
}

/// Drains a timing wheel, yielding each expired context to the provided callback.
///
/// The wheel continuously polls its inner receiver (insertion channel) to keep time
/// progressing and insert new entries. As entries expire, they are flattened and
/// handed to `on_expire` one at a time. This runs up to `budget` items per poll.
async fn wheel_drain<A, T, F>(
    rx: intrusive_queue::unsync::Receiver<A>,
    timer: T,
    mut on_expire: F,
    budget: usize,
) where
    A: crate::clock::wheel::WheelAdapter,
    T: precision::Timer,
    F: FnMut(A::Pointer),
{
    let wheel: crate::clock::wheel::Wheel<A, T, _> =
        crate::clock::wheel::Wheel::new(rx.into_list_receiver(), timer);
    let rx = FlattenList::new(wheel);
    let rx = Map::new(rx, |item| on_expire(item));
    rx.drain_budgeted(Some(budget)).await;
}

/// Per-socket receive worker: reads raw UDP segments and routes decoded packets to dispatch.
///
/// `packet_tx` is generic so callers can substitute a local unsync sender when the dispatch
/// task is co-located on the same worker.
///
/// Drives a [`SocketReceiver`] → [`InspectErr`] → [`FlattenSegments`] → [`RouterAdapter`] chain,
/// drained with a per-poll `budget` so the executor can interleave other tasks.
/// Each segment is decoded; datagram packets are forwarded via `packet_tx` to the dispatch task,
/// and decode errors are tallied via `decode_error_counter`.
///
/// [`SocketReceiver`]: crate::socket::channel::SocketReceiver
/// [`InspectErr`]: crate::socket::channel::InspectErr
/// [`FlattenSegments`]: crate::socket::channel::FlattenSegments
/// [`RouterAdapter`]: crate::socket::channel::RouterAdapter
///
/// # TODO: missing stream2 pipeline stages
///
/// - **Receive metrics**: stream2 counts received packets (`socket.rx`) and bytes
///   (`socket.rx:bytes`) via `channel::Inspect` before the router. Add equivalent counters once
///   the counter infrastructure is wired up.
///
/// - **Recv-side pacing** (experimental): stream2 has a commented-out `Paced` stage on the
///   recv side to cap ingest rate. Revisit if recv processing becomes a bottleneck.
pub async fn socket_recv_task<Socket, R>(
    socket: Socket,
    pool: crate::socket::pool::Pool,
    router: R,
    budgets: Budgets,
) where
    Socket: crate::socket::recv::Socket,
    R: crate::socket::recv::router::Router,
{
    let rx = SocketReceiver::new(socket, pool);
    let rx = InspectErr::new(rx, |err| {
        tracing::warn!(%err, "socket recv error");
    });
    let rx = FlattenSegments::new(rx);
    RouterAdapter::new(rx, router)
        .drain_budgeted(Some(budgets.socket_recv))
        .await;
}

/// Per-worker packet dispatch loop: decrypts, deduplicates, and dispatches received packets.
///
/// `packet_rx` and `ack_sender` are generic so callers can substitute local unsync receivers
/// or a custom ACK fan-out when tasks are co-located on the same worker.
///
/// Accepts a worker-shared `recv_cache` as `Rc<RefCell<recv::Cache>>` created once in
/// [`Worker::spawn`]. This matches stream2's `Rc<RefCell<SenderStateCache>>` pattern: all tasks
/// that run on the same worker thread share the same cache so they can coordinate without locks.
///
/// Uses a [`Map`] combinator over `packet_rx` that calls [`dispatch::process`] for each packet,
/// then drains up to `budget` items per poll so the executor can interleave other tasks.
///
/// Dispatch errors are silently dropped — they represent invalid/duplicate/unauthenticated
/// packets which should not terminate the worker.
///
/// [`Map`]: crate::socket::channel::Map
/// [`recv::Cache`]: crate::stream3::endpoint::recv::Cache
/// [`dispatch::process`]: crate::stream3::endpoint::dispatch::process
///
/// # TODO: missing stream2 pipeline stages
///
/// - **Dispatch counters** (`rx.data_pkt`, process-level counters): stream2 increments per-packet
///   and per-frame counters. The dispatch sub-counters are not yet wired up in stream3.
///
/// - **Queue depth metric** (`q.datagram`): stream2 wraps the input queue in `GaugedQueue`.
///   Add once the counter infrastructure is available per-worker.
pub async fn packet_dispatch_task<PacketRx, AckTx, WakerSink, Clk, Route>(
    packet_rx: PacketRx,
    recv_cache: Rc<RefCell<endpoint::recv::Cache>>,
    path_secret_map: crate::path::secret::Map,
    acceptor_registry: crate::acceptor::Registry<crate::stream3::Stream>,
    frame_tx: crate::stream3::frame::SubmissionSender,
    ack_sender: AckTx,
    queue_dispatcher: msg::queue::Dispatcher,
    counters: Arc<endpoint::counters::Dispatch>,
    clock: Clk,
    route: Route,
    mut waker_sink: WakerSink,
    budgets: Budgets,
) where
    PacketRx: Receiver<
        crate::intrusive_queue::Entry<crate::packet::datagram::decoder::Packet<descriptor::Filled>>,
    >,
    AckTx: UnboundedSender<Entry<msg::Sender>>,
    WakerSink: UnboundedSender<crate::flow::queue::AutoWake>,
    Clk: s2n_quic_core::time::Clock + precision::Clock,
    Route: endpoint::routing::SenderRoute,
{
    // Response frames (ACKs sent back to peers) re-enter via the same submission channel.
    // TODO: route responses through a dedicated channel + RetransmissionBatcher (see above).
    let rx = Map::new(packet_rx, {
        let mut response_tx = frame_tx.clone();
        let mut ack_sender = ack_sender;
        let mut queue_dispatcher = queue_dispatcher;
        let counters = counters.clone();

        move |packet| {
            counters.rx_data_pkt.add(1);
            dispatch::process(
                packet,
                &mut recv_cache.borrow_mut(),
                &path_secret_map,
                &acceptor_registry,
                &frame_tx,
                &mut response_tx,
                &mut ack_sender,
                &mut queue_dispatcher,
                &clock,
                &counters,
                &route,
                &mut waker_sink,
            )
        }
    });
    let rx = InspectErr::new(rx, {
        let counters = counters;
        move |err| on_packet_dispatch_error(&counters, err)
    });
    rx.drain_budgeted(Some(budgets.packet_dispatch)).await;
}

/// Drains offloaded wakers from dispatch workers, invoking each one.
///
/// Composes the `waker::Drain` receiver (which yields `Waker` values from its assigned slots)
/// with a `Map` that calls `wake()` on each, then uses `drain_budgeted` for per-poll fairness.
pub async fn waker_drain_task(drain: endpoint::waker::Drain, budgets: Budgets) {
    let rx = Map::new(drain, |waker: core::task::Waker| waker.wake());
    rx.drain_budgeted(Some(budgets.waker_drain)).await;
}

/// Drains ACK completion entries returning from the send worker's assembler.
///
/// For each returned entry, looks up the recv context and checks if new packets arrived
/// while the ACK was in flight. If stale (ack_state went back to Scheduled), re-submits
/// a fresh PendingAck. Otherwise transitions Flushed → Idle.
pub async fn ack_completion_task<AckTx>(
    completion_rx: impl Receiver<Entry<msg::Sender>>,
    recv_cache: Rc<RefCell<endpoint::recv::Cache>>,
    mut ack_sender: AckTx,
    budgets: Budgets,
) where
    AckTx: UnboundedSender<Entry<msg::Sender>>,
{
    let rx = Map::new(completion_rx, move |entry: Entry<msg::Sender>| {
        let msg::Sender::PendingAck(ref submission) = *entry else {
            return;
        };

        let key = endpoint::recv::Key {
            id: *submission.path_secret_entry.id(),
            remote_sender_id: submission.remote_sender_id,
        };

        let mut cache = recv_cache.borrow_mut();
        let Some(ctx) = cache.senders.get_mut(&key) else {
            return;
        };

        if ctx.ack_state.is_scheduled() {
            // New packets arrived while in flight — re-submit.
            let mtu = ctx.path_entry.max_datagram_size() as usize;
            let max_body_len = mtu.saturating_sub(endpoint::recv::ack_ranges::PACKET_OVERHEAD);
            let _ = ctx.ack_state.on_flush();
            ctx.ack_writer.update(
                ctx.ack_ranges
                    .encode_body(ctx.ecn_counts.as_option(), max_body_len)
                    .unwrap_or_default(),
                ctx.ack_ranges
                    .largest_recv_time()
                    .map(Into::into)
                    .unwrap_or(crate::clock::precision::Timestamp { nanos: 0 }),
                ctx.ecn_counts.as_option().is_some(),
            );
            let _ = ack_sender.send(entry);
        } else {
            let _ = ctx.ack_state.on_completion_idle();
        }
    });
    rx.drain_budgeted(Some(budgets.ack_completion)).await;
}

fn on_packet_dispatch_error(counters: &endpoint::counters::Dispatch, err: dispatch::Error) {
    match err {
        dispatch::Error::PeerStateLookup {
            credentials,
            control_out,
        } => {
            counters.rx_process_err_peer_lookup.add(1);
            tracing::warn!(
                ?credentials,
                control_out_len = control_out.len(),
                "failed to get or create peer state"
            );
        }
        dispatch::Error::Decryption {
            credentials,
            packet_number,
        } => {
            counters.rx_process_err_decryption.add(1);
            tracing::debug!(
                ?credentials,
                pn = packet_number.as_u64(),
                "failed to decrypt packet - authentication failed"
            );
        }
        dispatch::Error::Duplicate {
            credentials,
            packet_number,
        } => {
            counters.rx_process_err_duplicate.add(1);
            tracing::trace!(
                ?credentials,
                pn = packet_number.as_u64(),
                "duplicate packet filtered"
            );
        }
        dispatch::Error::MissingSenderId => {
            counters.rx_process_err_missing_sender_id.add(1);
            tracing::warn!("packet missing routing info; expected SenderId");
        }
        dispatch::Error::UnsupportedRoutingInfo { routing_info } => {
            counters.rx_process_err_unsupported_routing.add(1);
            tracing::warn!(?routing_info, "unsupported datagram routing info");
        }
    }
}

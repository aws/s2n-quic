// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    endpoint::{
        self,
        combinator::{
            AckProcessor, Assembler, AssemblerCounters, BatchFramesByPathSecret,
            CompletionDispatcher, FrameBatch, PathSecretMapEntry, PickTwo,
        },
        dispatch,
        frame::{self, Frame, Priority, PriorityStorage, SubmissionReceiver},
        msg, send, Budgets,
    },
    intrusive::{Entry, Queue},
    runtime::Spawner,
    socket::{
        channel::{
            intrusive::{self, unsync},
            Budget, FlattenList, FlattenSegments, InspectErr, Map, Paced,
            Priority as PriorityRx, Receiver, ReceiverExt as _, RouterAdapter, SocketReceiver,
            SocketSender, UnboundedSender,
        },
        pool::descriptor,
        rate::Rate,
    },
    time::precision,
};
use core::task::Poll;
use s2n_quic_core::{assume, varint::VarInt};
use std::{cell::RefCell, rc::Rc, sync::Arc};

/// Default per-poll budget for [`socket_recv_task`]: process up to this many segments before
/// yielding to the executor. Tune via the `budget` parameter if workloads differ.
pub const DEFAULT_RECV_BUDGET: usize = 32;

/// Default per-poll budget for [`packet_dispatch_task`]: process up to this many packets before
/// yielding to the executor. Tune via the `budget` parameter if workloads differ.
pub const DEFAULT_DISPATCH_BUDGET: usize = 32;

#[cfg(test)]
mod tests;

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
/// [`poll_swap`]: crate::socket::channel::intrusive_queue::sharded::Receiver::poll_swap
/// [`ListSender`]: crate::socket::channel::intrusive_queue::unsync::ListSender
/// [`channel::Priority`]: crate::socket::channel::Priority
/// [`channel::Paced`]: crate::socket::channel::Paced
/// [`Priority::LEVELS`]: crate::stream::frame::Priority::LEVELS
/// [`PriorityStorage`]: crate::stream::frame::PriorityStorage
/// [`PriorityInput`]: crate::stream::frame::PriorityInput
pub fn frame_dispatch<S, Clk>(
    spawner: &mut impl Spawner,
    frame_rx: SubmissionReceiver,
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
    let priority_list_txs: [_; Priority::LEVELS] = core::array::from_fn(|_| {
        let (tx, rx) = intrusive::unsync::new::<Frame>();
        priority_batch_rxs.push(rx);
        tx.into_list_sender()
    });
    let q_router_to_batcher: [_; Priority::LEVELS] = core::array::from_fn(|i| {
        counter_registry.register_queue_gauge_nominal("q.router_to_batcher", format_args!("p{i}"))
    });

    // Task 1: fixed-cost priority routing.
    spawner.spawn({
        let rx = SwapReceiver {
            frame_rx,
            staging: PriorityStorage::default(),
            priority_list_txs,
            q_router_to_batcher: q_router_to_batcher.clone(),
        };
        let budget_summary = counter_registry
            .register_summary("task.priority_router.drained", crate::counter::Unit::Count);
        let time_summary = counter_registry.register_timer("task.priority_router.time");
        rx.drain_budgeted_metered(
            Some(budgets.submission_router),
            budget_summary,
            time_summary,
        )
    });

    // Task 2: batch → Entry → priority merge → pace → pick-two to workers.
    spawner.spawn({
        let priority_batch_rxs = priority_batch_rxs
            .into_iter()
            .zip(q_router_to_batcher)
            .map(|(rx, gauge)| {
                crate::counter::GaugedQueueReceiver::new(rx.into_list_receiver(), gauge)
            })
            .collect();
        let rx = PriorityRx::new(priority_batch_rxs);
        let rx = BatchFramesByPathSecret::new(rx, &clock, overall_send_rate);
        let rx = Map::new(rx, Entry::new);
        let rx = Paced::new(rx, clock, overall_send_rate);
        let rx = PickTwo::new(rx, worker_senders, rng, &counter_registry);
        let budget_summary = counter_registry
            .register_summary("task.frame_dispatch.drained", crate::counter::Unit::Count);
        let time_summary = counter_registry.register_timer("task.frame_dispatch.time");
        rx.drain_budgeted_metered(Some(budgets.frame_dispatch), budget_summary, time_summary)
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
    spawner: &mut impl Spawner,
    worker_id: usize,
    batch_rx: impl Receiver<Entry<FrameBatch>> + 'static,
    ack_rx: impl Receiver<Entry<msg::Sender>> + 'static,
    total_sender_ids: usize,
    send_sockets: Vec<endpoint::SendSocketParts<Socket, Clk>>,
    clock: Clk,
    random: crate::xorshift::Rng,
    frame_tx: frame::SubmissionSender,
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
    // Per-socket unsync channel: wheel drain tasks route contexts here after expiration,
    // per-socket assembler+send task drains them.
    let (socket_context_txs, socket_context_rxs, q_wheel_to_assembler): (Vec<_>, Vec<_>, Vec<_>) =
        send_sockets
            .iter()
            .map(|st| {
                let (tx, rx) = unsync::new_with_adapter::<send::TxWheelAdapter>();
                let gauge = counter_registry.register_queue_gauge_nominal(
                    "q.wheel_to_assembler",
                    format_args!("send.{}", st.sender_idx),
                );
                (tx, rx, gauge)
            })
            .fold(
                (Vec::new(), Vec::new(), Vec::new()),
                |(mut txs, mut rxs, mut gauges), (tx, rx, gauge)| {
                    txs.push(tx);
                    rxs.push(rx);
                    gauges.push(gauge);
                    (txs, rxs, gauges)
                },
            );

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

    let variant = format!("send.{worker_id}");
    let q_resolver_to_tx_wheel =
        counter_registry.register_queue_gauge_nominal("q.resolver_to_tx_wheel", &variant);
    let (tx_wheel_tx, tx_wheel_rx) = unsync::new_with_adapter();
    let q_resolver_to_pto_wheel =
        counter_registry.register_queue_gauge_nominal("q.resolver_to_pto_wheel", &variant);
    let (pto_wheel_tx, pto_wheel_rx) = unsync::new_with_adapter();
    let q_resolver_to_idle_wheel =
        counter_registry.register_queue_gauge_nominal("q.resolver_to_idle_wheel", &variant);
    let (idle_wheel_tx, idle_wheel_rx) = unsync::new_with_adapter();
    let q_ack_to_completion =
        counter_registry.register_queue_gauge_nominal("q.ack_to_completion", &variant);
    let (completed_tx, completed_rx) = unsync::new::<Frame>();
    let q_ack_to_cancelled =
        counter_registry.register_queue_gauge_nominal("q.ack_to_cancelled", &variant);
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
        let q_resolver_to_tx_wheel = q_resolver_to_tx_wheel.clone();
        let q_resolver_to_pto_wheel = q_resolver_to_pto_wheel.clone();
        let q_resolver_to_idle_wheel = q_resolver_to_idle_wheel.clone();
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
                match cache.get_or_insert(batch.path_secret_entry()) {
                    Ok(ctx) => ctx,
                    Err(error) => {
                        tracing::warn!(?error, "dropping batch: send context not ready");
                        return;
                    }
                }
            };

            let wheel_interest = {
                let mut sender = sender.borrow_mut();
                sender.push_batch(batch.into_inner(), &clock)
            };

            if wheel_interest.transmission {
                q_resolver_to_tx_wheel.enqueue(1);
            }
            if wheel_interest.pto {
                q_resolver_to_pto_wheel.enqueue(1);
            }
            if wheel_interest.idle_timeout {
                q_resolver_to_idle_wheel.enqueue(1);
            }
            wheel_interest.dispatch(
                sender,
                &mut tx_wheel_tx,
                &mut pto_wheel_tx,
                &mut idle_wheel_tx,
            );
        });
        let variant = format!("send.{worker_id}");
        let budget_summary = counter_registry.register_nominal_summary(
            "task.context_resolver.drained",
            &variant,
            crate::counter::Unit::Count,
        );
        let time_summary =
            counter_registry.register_nominal_timer("task.context_resolver.time", &variant);
        rx.drain_budgeted_metered(Some(budgets.context_resolver), budget_summary, time_summary)
    });

    // Task 2: ACK processor — decode, update CCA/RTT, detect loss, reschedule.
    spawner.spawn({
        let send_caches = send_caches.clone();
        let sender_idx_to_local = sender_idx_to_local.clone();
        let send_counters = endpoint::counters::Send::new(&counter_registry);
        let completed_tx =
            crate::counter::GaugedSender::new(completed_tx, q_ack_to_completion.clone());
        let cancelled_tx =
            crate::counter::GaugedSender::new(cancelled_tx.clone(), q_ack_to_cancelled.clone());
        let rx = AckProcessor::new(
            ack_rx,
            send_caches,
            sender_idx_to_local,
            total_sender_ids,
            clock.clone(),
            random,
            frame_tx,
            completed_tx,
            cancelled_tx,
            tx_wheel_tx.clone(),
            pto_wheel_tx.clone(),
            idle_wheel_tx.clone(),
            send_counters,
            q_resolver_to_tx_wheel.clone(),
        );
        let variant = format!("send.{worker_id}");
        let budget_summary = counter_registry.register_nominal_summary(
            "task.ack_processor.drained",
            &variant,
            crate::counter::Unit::Count,
        );
        let time_summary =
            counter_registry.register_nominal_timer("task.ack_processor.time", &variant);
        rx.drain_budgeted_metered(Some(budgets.ack_processor), budget_summary, time_summary)
    });

    // Task 3: Completion dispatcher — batches completed frames by channel, one lock per batch.
    spawner.spawn({
        let rx = crate::counter::GaugedReceiver::new(completed_rx, q_ack_to_completion);
        let rx = CompletionDispatcher::new(rx);
        let rx = Map::new(rx, move |waker: crate::flow::queue::AutoWake| {
            let _ = waker_sink.send(waker);
        });
        let variant = format!("send.{worker_id}");
        let budget_summary = counter_registry.register_nominal_summary(
            "task.completion.drained",
            &variant,
            crate::counter::Unit::Count,
        );
        let time_summary =
            counter_registry.register_nominal_timer("task.completion.time", &variant);
        rx.drain_budgeted_metered(Some(budgets.completion_acked), budget_summary, time_summary)
    });

    // Task 4: Cancelled frame drain — drops frames whose writer is already gone.
    spawner.spawn({
        let rx = crate::counter::GaugedReceiver::new(cancelled_rx, q_ack_to_cancelled);
        let rx = Map::new(rx, |_entry: Entry<Frame>| {});
        let variant = format!("send.{worker_id}");
        let budget_summary = counter_registry.register_nominal_summary(
            "task.cancelled.drained",
            &variant,
            crate::counter::Unit::Count,
        );
        let time_summary = counter_registry.register_nominal_timer("task.cancelled.time", &variant);
        rx.drain_budgeted_metered(
            Some(budgets.completion_cancelled),
            budget_summary,
            time_summary,
        )
    });

    // Task 5: TX wheel drain — routes expired contexts to per-socket assembler channels.
    spawner.spawn(wheel_drain(
        tx_wheel_rx,
        clock.timer(),
        {
            let sender_idx_to_local = sender_idx_to_local.clone();
            let mut socket_context_txs = socket_context_txs;
            let q_resolver_to_tx_wheel = q_resolver_to_tx_wheel.clone();
            let q_wheel_to_assembler = q_wheel_to_assembler.clone();
            move |context: Rc<RefCell<send::Context>>| {
                q_resolver_to_tx_wheel.dequeue();
                let local_id = sender_idx_to_local[context.borrow().sender_idx];
                q_wheel_to_assembler[local_id].enqueue(1);
                let _ = UnboundedSender::send(&mut socket_context_txs[local_id], context);
            }
        },
        budgets.tx_wheel,
        counter_registry.register_nominal_summary(
            "task.tx_wheel.drained",
            format!("send.{worker_id}"),
            crate::counter::Unit::Count,
        ),
        counter_registry.register_nominal_timer("task.tx_wheel.time", format!("send.{worker_id}")),
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
            let q_resolver_to_tx_wheel = q_resolver_to_tx_wheel.clone();
            let q_resolver_to_pto_wheel = q_resolver_to_pto_wheel.clone();
            let q_resolver_to_idle_wheel = q_resolver_to_idle_wheel.clone();
            let tx_pto_check = counter_registry.register("tx.pto_check");
            let tx_pto_requested = counter_registry.register("tx.pto_requested");
            move |context: Rc<RefCell<send::Context>>| {
                q_resolver_to_pto_wheel.dequeue();
                tx_pto_check.add(1);
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
                    q_resolver_to_tx_wheel.enqueue(1);
                }
                if wheel_interest.pto {
                    q_resolver_to_pto_wheel.enqueue(1);
                }
                if wheel_interest.idle_timeout {
                    q_resolver_to_idle_wheel.enqueue(1);
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
        counter_registry.register_nominal_summary(
            "task.pto_wheel.drained",
            format!("send.{worker_id}"),
            crate::counter::Unit::Count,
        ),
        counter_registry.register_nominal_timer("task.pto_wheel.time", format!("send.{worker_id}")),
    ));

    // Task 7: Idle wheel drain — reclaims resources for idle connections.
    spawner.spawn(wheel_drain(
        idle_wheel_rx,
        clock.timer(),
        {
            let q_resolver_to_idle_wheel = q_resolver_to_idle_wheel.clone();
            move |context: Rc<RefCell<send::Context>>| {
                q_resolver_to_idle_wheel.dequeue();
                // TODO reclaim idle context resources
                let _ = context;
            }
        },
        budgets.idle_wheel,
        counter_registry.register_nominal_summary(
            "task.idle_wheel.drained",
            format!("send.{worker_id}"),
            crate::counter::Unit::Count,
        ),
        counter_registry
            .register_nominal_timer("task.idle_wheel.time", format!("send.{worker_id}")),
    ));

    // Per-socket assembler + send tasks.
    let asm_counters = AssemblerCounters::new(&counter_registry, q_resolver_to_tx_wheel.clone());
    for ((st, context_rx), gauge) in send_sockets
        .into_iter()
        .zip(socket_context_rxs)
        .zip(q_wheel_to_assembler)
    {
        let source_sender_id = VarInt::new(st.sender_idx as u64).unwrap();
        let sender_idx = st.sender_idx;

        spawner.spawn({
            let clock = st.clock.clone();
            let tx_wheel_tx = tx_wheel_tx.clone();
            let pto_wheel_tx = pto_wheel_tx.clone();
            let idle_wheel_tx = idle_wheel_tx.clone();
            let cancelled_tx = cancelled_tx.clone().into_list_sender();
            let ack_completions_tx = ack_completions_tx.clone();
            let asm_counters = asm_counters.clone();
            let context_rx = crate::counter::GaugedReceiver::new(context_rx, gauge);
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
            let variant = format!("send.{sender_idx}");
            let budget_summary = counter_registry.register_nominal_summary(
                "task.assembler.drained",
                &variant,
                crate::counter::Unit::Count,
            );
            let time_summary =
                counter_registry.register_nominal_timer("task.assembler.time", &variant);
            rx.drain_budgeted_metered(Some(budgets.assembler), budget_summary, time_summary)
        });
    }
}

/// Drains a timing wheel, yielding each expired context to the provided callback.
///
/// The wheel continuously polls its inner receiver (insertion channel) to keep time
/// progressing and insert new entries. As entries expire, they are flattened and
/// handed to `on_expire` one at a time. This runs up to `budget` items per poll.
async fn wheel_drain<A, T, F>(
    rx: intrusive::unsync::Receiver<A>,
    timer: T,
    mut on_expire: F,
    budget: usize,
    budget_summary: crate::counter::Summary,
    time_summary: crate::counter::Timer,
) where
    A: crate::time::wheel::WheelAdapter,
    T: precision::Timer,
    F: FnMut(A::Pointer),
{
    let wheel: crate::time::wheel::Wheel<A, T, _> =
        crate::time::wheel::Wheel::new(rx.into_list_receiver(), timer);
    let rx = FlattenList::new(wheel);
    let rx = Map::new(rx, |item| on_expire(item));
    rx.drain_budgeted_metered(Some(budget), budget_summary, time_summary)
        .await;
}

/// Builds a receiver that reads raw UDP segments from a socket and routes decoded packets
/// to the dispatch task.
///
/// Drives a [`SocketReceiver`] → [`InspectErr`] → [`FlattenSegments`] → [`RouterAdapter`]
/// chain. The caller is responsible for draining with an appropriate budget and metrics.
///
/// [`SocketReceiver`]: crate::socket::channel::SocketReceiver
/// [`InspectErr`]: crate::socket::channel::InspectErr
/// [`FlattenSegments`]: crate::socket::channel::FlattenSegments
/// [`RouterAdapter`]: crate::socket::channel::RouterAdapter
pub fn socket_recv<Socket, R>(
    socket: Socket,
    pool: crate::socket::pool::Pool,
    router: R,
) -> impl Receiver<()>
where
    Socket: crate::socket::recv::Socket,
    R: crate::socket::recv::router::Router,
{
    let rx = SocketReceiver::new(socket, pool);
    let rx = InspectErr::new(rx, |err| {
        tracing::warn!(%err, "socket recv error");
    });
    let rx = FlattenSegments::new(rx);
    RouterAdapter::new(rx, router)
}

/// Per-worker packet dispatch loop: decrypts, deduplicates, and dispatches received packets.
///
/// `packet_rx` and `ack_sender` are generic so callers can substitute local unsync receivers
/// or a custom ACK fan-out when tasks are co-located on the same worker.
///
/// Accepts a worker-shared `recv_cache` as `Rc<RefCell<recv::Cache>>` created once in
/// [`Worker::spawn`]. All tasks
/// that run on the same worker thread share the same cache so they can coordinate without locks.
///
/// Uses a [`Map`] combinator over `packet_rx` that calls [`dispatch::process`] for each packet,
/// then drains up to `budget` items per poll so the executor can interleave other tasks.
///
/// Dispatch errors are silently dropped — they represent invalid/duplicate/unauthenticated
/// packets which should not terminate the worker.
///
/// [`Map`]: crate::socket::channel::Map
/// [`recv::Cache`]: crate::stream::endpoint::recv::Cache
/// [`dispatch::process`]: crate::stream::endpoint::dispatch::process
pub async fn packet_dispatch_task<PacketRx, AckSender, AckBurstSender, WakerSink, Clk, Route>(
    packet_rx: PacketRx,
    recv_cache: Rc<RefCell<endpoint::recv::Cache>>,
    mut ack_burst_tx: AckBurstSender,
    path_secret_map: crate::path::secret::Map,
    acceptor_registry: crate::acceptor::Registry<crate::stream::Stream>,
    frame_tx: frame::SubmissionSender,
    mut ack_sender: AckSender,
    queue_dispatcher: msg::queue::Dispatcher,
    counters: Arc<endpoint::counters::Dispatch>,
    clock: Clk,
    route: Route,
    mut waker_sink: WakerSink,
    budgets: Budgets,
    counter_registry: crate::counter::Registry,
    worker_idx: usize,
) where
    PacketRx: Receiver<
        crate::intrusive::Entry<crate::packet::datagram::decoder::Packet<descriptor::Filled>>,
    >,
    AckSender: UnboundedSender<Entry<msg::Sender>>,
    AckBurstSender: UnboundedSender<Rc<RefCell<endpoint::recv::Context>>>,
    WakerSink: UnboundedSender<crate::flow::queue::AutoWake>,
    Clk: s2n_quic_core::time::Clock + precision::Clock,
    Route: endpoint::routing::SenderRoute,
{
    eprintln!("[packet_dispatch.{worker_idx}] task started");

    let variant = format!("recv.{worker_idx}");
    let budget_summary = counter_registry.register_nominal_summary(
        "task.packet_dispatch.drained",
        &variant,
        crate::counter::Unit::Count,
    );
    let time_summary =
        counter_registry.register_nominal_timer("task.packet_dispatch.time", &variant);

    // Response frames (ACKs sent back to peers) re-enter via the same submission channel.
    // TODO: route responses through a dedicated channel + RetransmissionBatcher (see above).
    let rx = Map::new(packet_rx, {
        let mut response_tx = frame_tx.clone();
        let mut queue_dispatcher = queue_dispatcher;
        let counters = counters.clone();

        move |packet| {
            counters.rx_data_pkt.add(1);
            dispatch::process(
                packet,
                &mut recv_cache.borrow_mut(),
                &mut ack_burst_tx,
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
    rx.drain_budgeted_metered(Some(budgets.packet_dispatch), budget_summary, time_summary)
        .await;

    eprintln!("[packet_dispatch.{worker_idx}] task exited");
}

/// Builds a receiver that drains offloaded wakers from dispatch workers, invoking each one.
///
/// Composes the `waker::Drain` receiver (which yields `Waker` values from its assigned slots)
/// with a `Map` that calls `wake()` on each. The caller is responsible for draining with an
/// appropriate budget and metrics.
pub fn waker_drain(drain: endpoint::waker::Drain) -> impl Receiver<()> {
    Map::new(drain, |waker: core::task::Waker| waker.wake())
}

/// Drains ACK completion entries returning from the send worker's assembler.
///
/// For each returned entry, looks up the recv context and checks if new packets arrived
/// while the ACK was in flight. If stale (ack_state went back to Scheduled), re-submits
/// a fresh PendingAck. Otherwise transitions Flushed → Idle.
/// Builds a receiver that processes ACK completion entries returning from the assembler.
///
/// For each returned PendingAck entry, looks up the recv context and checks if new packets
/// arrived while the ACK was in flight. If stale (ack_state went back to Scheduled),
/// re-submits a fresh PendingAck. Otherwise transitions Flushed → Idle.
pub fn ack_completion<CompRx, AckTx>(
    completion_rx: CompRx,
    recv_cache: Rc<RefCell<endpoint::recv::Cache>>,
    mut ack_sender: AckTx,
) -> impl Receiver<()>
where
    CompRx: Receiver<Entry<msg::Sender>>,
    AckTx: UnboundedSender<Entry<msg::Sender>>,
{
    Map::new(completion_rx, move |entry: Entry<msg::Sender>| {
        let (key, recv_worker_id) = match &*entry {
            msg::Sender::PendingAck(submission) => (
                endpoint::recv::Key {
                    id: *submission.path_secret_entry.id(),
                    remote_sender_id: submission.remote_sender_id,
                },
                submission.recv_worker_id,
            ),
            _ => {
                debug_assert!(false, "ack completion task received non-PendingAck message");
                return;
            }
        };

        let ctx_rc = {
            let cache = recv_cache.borrow();
            let Some(ctx) = cache.senders.get(&key) else {
                return;
            };
            ctx.clone()
        };
        let mut ctx = ctx_rc.borrow_mut();

        if let Some(submission) = ctx.on_ack_completion(recv_worker_id) {
            let mut pending_ack_entry = entry;
            *pending_ack_entry = msg::Sender::PendingAck(submission);
            let _ = ack_sender.send(pending_ack_entry);
        }
    })
}

/// Builds a receiver that encodes and flushes pending ACK bursts from recv contexts.
///
/// For each recv context submitted to `ack_burst_rx`, calls `encode_and_flush` to produce
/// a PendingAck submission and sends it to the `ack_sender`. The caller is responsible for
/// draining with an appropriate budget and metrics.
pub fn ack_burst<AckBurstRx, AckTx>(
    ack_burst_rx: AckBurstRx,
    mut ack_sender: AckTx,
    recv_worker_id: usize,
) -> impl Receiver<()>
where
    AckBurstRx: Receiver<Rc<RefCell<endpoint::recv::Context>>>,
    AckTx: UnboundedSender<Entry<msg::Sender>>,
{
    Map::new(
        ack_burst_rx,
        move |ctx_rc: Rc<RefCell<endpoint::recv::Context>>| {
            let mut ctx = ctx_rc.borrow_mut();
            if let Some(submission) = ctx.encode_and_flush(recv_worker_id) {
                let _ = ack_sender.send(Entry::new(msg::Sender::PendingAck(submission)));
            }
        },
    )
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
    }
}

// ── SwapReceiver ──────────────────────────────────────────────────────────

/// Adapts the frame submission channel's `poll_swap` into a `Receiver<()>` so
/// it can be drained via `drain_budgeted`. Each poll_recv performs one swap and
/// distributes the resulting frames into per-priority lane senders.
struct SwapReceiver<Tx> {
    frame_rx: SubmissionReceiver,
    staging: PriorityStorage,
    priority_list_txs: [Tx; Priority::LEVELS],
    q_router_to_batcher: [crate::counter::QueueGauge; Priority::LEVELS],
}

impl<Tx> Receiver<()> for SwapReceiver<Tx>
where
    Tx: UnboundedSender<crate::intrusive::List<crate::intrusive::EntryAdapter<Frame>>>,
{
    fn poll_recv(
        &mut self,
        cx: &mut core::task::Context<'_>,
        budget: &mut Budget,
    ) -> Poll<Option<()>> {
        if budget.is_exhausted() {
            budget.set_needs_wake();
            return Poll::Pending;
        }

        match self.frame_rx.poll_swap(cx, &mut self.staging) {
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
            Poll::Ready(Some(())) => {
                budget.consume();
                for (i, ((_priority, queue), tx)) in self
                    .staging
                    .drain()
                    .zip(&mut self.priority_list_txs)
                    .enumerate()
                {
                    if !queue.is_empty() {
                        self.q_router_to_batcher[i].enqueue(queue.len() as u64);
                        let _ = UnboundedSender::send(tx, queue);
                    }
                }
                Poll::Ready(Some(()))
            }
        }
    }

    fn on_consumed(&mut self, _bytes: u64) {}
}

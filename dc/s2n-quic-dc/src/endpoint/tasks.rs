// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    counter::{self, GaugedReceiver, GaugedSender, QueueGauge},
    endpoint::{
        self,
        combinator::{
            AckProcessor, Assembler, AssemblerCounters, BatchFramesByPathSecret,
            CompletionDispatcher, FrameBatch, PathSecretMapEntry, PickTwo,
        },
        dispatch,
        frame::{self, Frame, Priority, PriorityStorage, SubmissionReceiver},
        id::{IdJoin, IdMap, LocalSendSocketId, LocalSenderId, RecvDispatchWorkerId, SendWorkerId},
        msg, send, Budgets,
    },
    intrusive::{Entry, Queue},
    packet::datagram::decoder::Packet,
    runtime::Spawner,
    socket::{
        channel::{
            intrusive::{self, unsync},
            Budget, FilterMap, Flatten, FlattenList, FlattenSegments, InspectErr, Map, Paced,
            Priority as PriorityRx, PrioritySelect, Receiver, ReceiverExt as _, RouterAdapter,
            SocketReceiver, SocketSender, UnboundedSender,
        },
        pool::descriptor,
        rate::Rate,
    },
    time::{
        precision,
        wheel::{self, Wheel},
    },
    tracing::*,
};
use core::task::Poll;
use s2n_quic_core::varint::VarInt;
use s2n_quic_platform::features::Gso;
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
    worker_senders: IdMap<LocalSenderId, S>,
    rng: crate::xorshift::Rng,
    clock: Clk,
    overall_send_rate: Rate,
    per_socket_send_rate: Rate,
    budgets: Budgets,
    counter_registry: counter::Registry,
) where
    S: UnboundedSender<Entry<FrameBatch>> + 'static,
    Clk: precision::Clock + Clone + 'static,
{
    let mut priority_batch_rxs = Vec::with_capacity(Priority::LEVELS);
    let priority_txs_raw: [_; Priority::LEVELS] = core::array::from_fn(|_| {
        let (tx, rx) = intrusive::unsync::new::<Frame>();
        priority_batch_rxs.push(rx);
        tx
    });
    let q_router_to_batcher: [_; Priority::LEVELS] = core::array::from_fn(|i| {
        counter_registry
            .register_queue_gauge_nominal("q.router_to_batcher", format_args!("p{i}"))
            .with_registration_metadata(
                format!("ch.router_to_batcher.p{i}"),
                "Per-priority unsync queue from priority router to frame dispatch",
                "endpoint::tasks::frame_dispatch",
            )
    });

    {
        // Task 1: fixed-cost priority routing.
        let priority_list_txs: [_; Priority::LEVELS] =
            core::array::from_fn(|i| priority_txs_raw[i].clone().into_list_sender());

        let rx = FrameReceiver {
            frame_rx,
            staging: PriorityStorage::default(),
            priority_list_txs,
            q_router_to_batcher: q_router_to_batcher.clone(),
        };
        let task_counter = counter_registry
            .register_task("task.priority_router")
            .with_registration_metadata(
                "task.priority_router",
                "Routes submissions into per-priority queues",
                "endpoint::tasks::frame_dispatch",
            );
        spawner.spawn_receiver_task(
            rx.drain_budgeted_metered(Some(budgets.submission_router), task_counter.clone()),
            Some(budgets.submission_router),
            task_counter,
        );
    }

    {
        // Task 2: batch → Entry → priority merge → pace → pick-two to workers.
        let priority_batch_rxs = priority_batch_rxs
            .into_iter()
            .zip(q_router_to_batcher)
            .map(|(rx, gauge)| {
                let receiver = gauge
                    .receiver("task.frame_dispatch")
                    .with_description("Frame dispatch drains per-lane queues")
                    .with_function("endpoint::tasks::frame_dispatch");
                counter::GaugedQueueReceiver::new(rx.into_list_receiver(), receiver)
            })
            .collect();
        let rx = PriorityRx::new(priority_batch_rxs);
        let rx = BatchFramesByPathSecret::new(rx, &clock, overall_send_rate);
        let rx = Map::new(rx, Entry::new);
        let pick_two_clock = clock.clone();
        let rx = Paced::new(rx, clock, overall_send_rate);
        let rx = PickTwo::new(
            rx,
            worker_senders,
            pick_two_clock,
            per_socket_send_rate,
            rng,
            &counter_registry,
        );
        let task_counter = counter_registry
            .register_task("task.frame_dispatch")
            .with_registration_metadata(
                "task.frame_dispatch",
                "Batches, paces, and routes frame batches to worker send sockets",
                "endpoint::tasks::frame_dispatch",
            );
        spawner.spawn_receiver_task(
            rx.drain_budgeted_metered(Some(budgets.frame_dispatch), task_counter.clone()),
            Some(budgets.frame_dispatch),
            task_counter,
        );
    }
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
    worker_id: SendWorkerId,
    batch_rx: impl Receiver<Entry<FrameBatch>> + 'static,
    ack_rx: impl Receiver<Entry<msg::Sender>> + 'static,
    invalidation_rx: impl Receiver<Entry<Invalidation>> + 'static,
    total_sender_ids: usize,
    send_sockets: IdMap<LocalSendSocketId, endpoint::SendSocketParts<Socket, Clk>>,
    clock: Clk,
    random: crate::xorshift::Rng,
    frame_tx: frame::SubmissionSender,
    ack_completions_tx: AckComp,
    waker_sink: WakerSink,
    peer_dead_tx: impl UnboundedSender<Entry<PeerDead>> + Clone + 'static,
    dead_peer_cooldown: core::time::Duration,
    budgets: Budgets,
    counter_registry: counter::Registry,
) where
    Socket: crate::socket::send::Socket + 'static,
    Clk: precision::Clock + s2n_quic_core::time::Clock + Clone + 'static,
    WakerSink: UnboundedSender<crate::flow::queue::AutoWake> + Clone + 'static,
    AckComp: UnboundedSender<Queue<msg::Sender>> + Clone + 'static,
{
    // Per-socket unsync channel: wheel drain tasks route contexts here after expiration,
    // per-socket assembler+send task drains them.
    let (socket_context_txs, socket_context_rxs, q_wheel_to_assembler): (
        IdMap<_, _>,
        IdMap<_, _>,
        IdMap<_, _>,
    ) = send_sockets
        .iter()
        .map(|(id, st)| {
            let (tx, rx) = unsync::new_with_adapter::<send::TxWheelAdapter>();
            let gauge = counter_registry.register_queue_gauge_nominal(
                "q.wheel_to_assembler",
                format_args!("send.{}", st.sender_idx),
            );
            ((id, tx), (id, rx), (id, gauge))
        })
        .collect();

    // Per-socket immediate channel: bypasses the tx wheel for urgent transmissions
    // (ACKs, PTO probes arriving while context is already wheel-scheduled).
    let (socket_immediate_txs, socket_immediate_rxs): (IdMap<_, _>, IdMap<_, _>) = send_sockets
        .iter()
        .map(|(id, _st)| {
            let (tx, rx) = unsync::new_with_adapter::<send::TxImmediateAdapter>();
            ((id, tx), (id, rx))
        })
        .collect();

    // Map sender_idx → local socket position for this worker.
    let mut sender_idx_to_local: IdMap<LocalSenderId, LocalSendSocketId> =
        IdMap::new(total_sender_ids, LocalSendSocketId::new(usize::MAX));

    // Collect send socket local addresses for diagnostics (routing asymmetry logs).
    let sender_local_addrs: IdMap<LocalSendSocketId, std::net::SocketAddr> = send_sockets
        .iter()
        .map(|(id, st)| {
            (
                id,
                st.socket
                    .local_addr()
                    .unwrap_or_else(|_| std::net::SocketAddr::from(([0, 0, 0, 0], 0))),
            )
        })
        .collect();

    // One send::Cache per socket, shared between the context resolver and ACK processor.
    let send_caches: IdMap<LocalSendSocketId, Rc<RefCell<send::Cache>>> = send_sockets
        .iter()
        .map(|(local_id, st)| {
            sender_idx_to_local[st.sender_idx] = local_id;

            let cache = Rc::new(RefCell::new(send::Cache::new(
                &counter_registry,
                st.sender_idx,
            )));

            (local_id, cache)
        })
        .collect();

    let immediate_tx =
        send::ImmediateSender::new(socket_immediate_txs, sender_idx_to_local.clone());

    let variant = format!("send.worker.{worker_id}");
    let q_resolver_to_tx_wheel =
        counter_registry.register_queue_gauge_nominal("q.resolver_to_tx_wheel", &variant);
    let (tx_wheel_tx, tx_wheel_rx) = unsync::new_with_adapter::<send::TxWheelAdapter>();
    let q_resolver_to_pto_wheel =
        counter_registry.register_queue_gauge_nominal("q.resolver_to_pto_wheel", &variant);
    let (pto_wheel_tx, pto_wheel_rx) = unsync::new_with_adapter::<send::PtoWheelAdapter>();
    let q_resolver_to_idle_wheel =
        counter_registry.register_queue_gauge_nominal("q.resolver_to_idle_wheel", &variant);
    let (idle_wheel_tx, idle_wheel_rx) = unsync::new_with_adapter::<send::IdleWheelAdapter>();
    let q_ack_to_completion =
        counter_registry.register_queue_gauge_nominal("q.ack_to_completion", &variant);
    let (completed_tx, completed_rx) = unsync::new::<Frame>();
    let q_ack_to_cancelled =
        counter_registry.register_queue_gauge_nominal("q.ack_to_cancelled", &variant);
    let (cancelled_tx, cancelled_rx) = unsync::new::<Frame>();
    let q_resolver_to_tx_wheel = q_resolver_to_tx_wheel.with_registration_metadata(
        format!("ch.resolver_to_tx_wheel.{variant}"),
        "Send context scheduling channel feeding the tx wheel",
        "endpoint::tasks::send_worker",
    );
    let q_resolver_to_pto_wheel = q_resolver_to_pto_wheel.with_registration_metadata(
        format!("ch.resolver_to_pto_wheel.{variant}"),
        "Send context scheduling channel feeding the pto wheel",
        "endpoint::tasks::send_worker",
    );
    let q_resolver_to_idle_wheel = q_resolver_to_idle_wheel.with_registration_metadata(
        format!("ch.resolver_to_idle_wheel.{variant}"),
        "Send context scheduling channel feeding the idle wheel",
        "endpoint::tasks::send_worker",
    );
    let q_ack_to_completion = q_ack_to_completion.with_registration_metadata(
        format!("ch.ack_to_completion.{variant}"),
        "Completed frame channel from ack/invalidation tasks to completion dispatcher",
        "endpoint::tasks::send_worker",
    );
    let invalidation_completed_tx = GaugedSender::new(
        completed_tx.clone(),
        q_ack_to_completion
            .sender("task.invalidation")
            .with_description("Invalidation task emits failed frames as completions")
            .with_function("endpoint::tasks::send_worker"),
    );
    let idle_expired_completed_tx = GaugedSender::new(
        completed_tx.clone(),
        q_ack_to_completion
            .sender("task.idle_wheel")
            .with_description("Idle wheel emits PeerDead frames as completions")
            .with_function("endpoint::tasks::send_worker"),
    );
    let q_ack_to_cancelled = q_ack_to_cancelled.with_registration_metadata(
        format!("ch.ack_to_cancelled.{variant}"),
        "Cancelled frame channel drained by cancelled task",
        "endpoint::tasks::send_worker",
    );

    {
        // Task 1: context resolver — drain batch_rx, resolve to context, push frames.
        let tx_wheel_sender = q_resolver_to_tx_wheel
            .sender("task.context_resolver")
            .with_description("Context resolver schedules transmission work")
            .with_function("endpoint::tasks::send_worker");
        let pto_wheel_sender = q_resolver_to_pto_wheel
            .sender("task.context_resolver")
            .with_description("Context resolver schedules PTO checks")
            .with_function("endpoint::tasks::send_worker");
        let idle_wheel_sender = q_resolver_to_idle_wheel
            .sender("task.context_resolver")
            .with_description("Context resolver tracks idle expiry")
            .with_function("endpoint::tasks::send_worker");

        let rx = context_resolver(
            batch_rx,
            send_caches.clone(),
            sender_idx_to_local.clone(),
            total_sender_ids,
            clock.clone(),
            immediate_tx.clone(),
            GaugedSender::new(tx_wheel_tx.clone(), tx_wheel_sender),
            GaugedSender::new(pto_wheel_tx.clone(), pto_wheel_sender),
            GaugedSender::new(idle_wheel_tx.clone(), idle_wheel_sender),
        );
        let task_counter = counter_registry
            .register_nominal_task("task.context_resolver", &variant)
            .with_registration_metadata(
                "task.context_resolver",
                "Resolves frame batches to send contexts and schedules wheels",
                "endpoint::tasks::send_worker",
            );
        spawner.spawn_receiver_task(
            rx.drain_budgeted_metered(Some(budgets.context_resolver), task_counter.clone()),
            Some(budgets.context_resolver),
            task_counter,
        );
    }

    {
        // Task 2: ACK processor — decode, update CCA/RTT, detect loss, reschedule.
        let tx_wheel_sender = q_resolver_to_tx_wheel
            .sender("task.ack_processor")
            .with_description("ACK processor re-schedules transmission work")
            .with_function("endpoint::tasks::send_worker");
        let pto_wheel_sender = q_resolver_to_pto_wheel
            .sender("task.ack_processor")
            .with_description("ACK processor updates PTO scheduling")
            .with_function("endpoint::tasks::send_worker");
        let idle_wheel_sender = q_resolver_to_idle_wheel
            .sender("task.ack_processor")
            .with_description("ACK processor updates idle scheduling")
            .with_function("endpoint::tasks::send_worker");
        let completion_sender = q_ack_to_completion
            .sender("task.ack_processor")
            .with_description("ACK processor emits completed frames")
            .with_function("endpoint::tasks::send_worker");
        let cancelled_sender = q_ack_to_cancelled
            .sender("task.ack_processor")
            .with_description("ACK processor emits cancelled frames")
            .with_function("endpoint::tasks::send_worker");
        let rx = send_ack_processor(
            ack_rx,
            send_caches.clone(),
            sender_idx_to_local.clone(),
            total_sender_ids,
            clock.clone(),
            random,
            frame_tx.clone(),
            GaugedSender::new(completed_tx, completion_sender),
            GaugedSender::new(cancelled_tx.clone(), cancelled_sender),
            counter_registry.register("!send.invalid_sender_idx"),
            immediate_tx.clone(),
            GaugedSender::new(tx_wheel_tx.clone(), tx_wheel_sender),
            GaugedSender::new(pto_wheel_tx.clone(), pto_wheel_sender),
            GaugedSender::new(idle_wheel_tx.clone(), idle_wheel_sender),
        );
        let task_counter = counter_registry
            .register_nominal_task("task.ack_processor", &variant)
            .with_registration_metadata(
                "task.ack_processor",
                "Processes ACK feedback and re-schedules contexts across wheels",
                "endpoint::tasks::send_worker",
            );
        spawner.spawn_receiver_task(
            rx.drain_budgeted_metered(Some(budgets.ack_processor), task_counter.clone()),
            Some(budgets.ack_processor),
            task_counter,
        );
    }

    {
        // Task 3: Completion dispatcher — batches completed frames by channel, one lock per batch.
        let completion_receiver = q_ack_to_completion
            .receiver("task.completion")
            .with_description("Completion task drains completed frames")
            .with_function("endpoint::tasks::send_worker");

        let rx = GaugedReceiver::new(completed_rx, completion_receiver);
        let rx = completion_dispatcher(rx, waker_sink.clone());
        let task_counter = counter_registry
            .register_nominal_task("task.completion", &variant)
            .with_registration_metadata(
                "task.completion",
                "Dispatches completion notifications back to writer wakers",
                "endpoint::tasks::send_worker",
            );
        spawner.spawn_receiver_task(
            rx.drain_budgeted_metered(Some(budgets.completion_acked), task_counter.clone()),
            Some(budgets.completion_acked),
            task_counter,
        );
    }

    {
        // Task 4: Cancelled frame drain — drops frames whose writer is already gone.
        let cancelled_receiver = q_ack_to_cancelled
            .receiver("task.cancelled")
            .with_description("Cancelled task drains cancelled frames")
            .with_function("endpoint::tasks::send_worker");

        let rx = GaugedReceiver::new(cancelled_rx, cancelled_receiver);
        let rx = cancelled_drain(rx);
        let task_counter = counter_registry
            .register_nominal_task("task.cancelled", &variant)
            .with_registration_metadata(
                "task.cancelled",
                "Drains cancelled frames that no longer have an owner",
                "endpoint::tasks::send_worker",
            );
        spawner.spawn_receiver_task(
            rx.drain_budgeted_metered(Some(budgets.completion_cancelled), task_counter.clone()),
            Some(budgets.completion_cancelled),
            task_counter,
        );
    }

    {
        // Task 5: TX wheel drain — routes expired contexts to per-socket assembler channels.
        let task_counter = counter_registry
            .register_nominal_task("task.tx_wheel", &variant)
            .with_registration_metadata(
                "task.tx_wheel",
                "Drains tx timing wheel and routes expired contexts to assemblers",
                "endpoint::tasks::send_worker",
            );
        let socket_context_txs: IdMap<_, _> = (socket_context_txs, q_wheel_to_assembler.clone())
            .join()
            .map(|(id, tx, gauge)| {
                let sender = gauge
                    .sender("task.tx_wheel")
                    .with_description("Tx wheel routes expired contexts to socket assembler")
                    .with_function("endpoint::tasks::send_worker");
                let sender = GaugedSender::new(tx, sender);
                (id, sender)
            })
            .collect();
        let tx_wheel_task = send_tx_wheel_drain(
            tx_wheel_rx,
            clock.clone(),
            q_resolver_to_tx_wheel.clone(),
            socket_context_txs,
            sender_idx_to_local.clone(),
            budgets.tx_wheel,
            task_counter.clone(),
        );
        spawner.spawn_receiver_task(tx_wheel_task, Some(budgets.tx_wheel), task_counter);
    }

    {
        // Task 6: PTO wheel drain — fires probes for tail loss recovery.
        let pto_wheel_receiver = q_resolver_to_pto_wheel
            .receiver("task.pto_wheel")
            .with_description("PTO wheel drains scheduled contexts")
            .with_function("endpoint::tasks::send_worker");
        let tx_wheel_sender = q_resolver_to_tx_wheel
            .sender("task.pto_wheel")
            .with_description("PTO wheel requests probe transmissions")
            .with_function("endpoint::tasks::send_worker");
        let pto_wheel_sender = q_resolver_to_pto_wheel
            .sender("task.pto_wheel")
            .with_description("PTO wheel re-enqueues contexts after timeout processing")
            .with_function("endpoint::tasks::send_worker");
        let idle_wheel_sender = q_resolver_to_idle_wheel
            .sender("task.pto_wheel")
            .with_description("PTO wheel updates idle scheduling")
            .with_function("endpoint::tasks::send_worker");

        let wheel: Wheel<_, _, _, 128> =
            Wheel::new(pto_wheel_rx.into_list_receiver(), clock.timer());
        let rx = FlattenList::new(wheel);
        let rx = GaugedReceiver::new(rx, pto_wheel_receiver);
        let tx_pto_check = counter_registry.register("tx.pto_check");
        let tx_pto_requested = counter_registry.register("tx.pto_requested");
        let rx = send_pto_timeout(
            rx,
            clock.clone(),
            immediate_tx.clone(),
            GaugedSender::new(tx_wheel_tx.clone(), tx_wheel_sender),
            GaugedSender::new(pto_wheel_tx.clone(), pto_wheel_sender),
            GaugedSender::new(idle_wheel_tx.clone(), idle_wheel_sender),
            tx_pto_check,
            tx_pto_requested,
            send_caches.clone(),
            sender_idx_to_local.clone(),
        );
        let task_counter = counter_registry
            .register_nominal_task("task.pto_wheel", &variant)
            .with_registration_metadata(
                "task.pto_wheel",
                "Handles probe-timeout expirations and wheel re-scheduling",
                "endpoint::tasks::send_worker",
            );
        spawner.spawn_receiver_task(
            rx.drain_budgeted_metered(Some(budgets.pto_wheel), task_counter.clone()),
            Some(budgets.pto_wheel),
            task_counter,
        );
    }

    {
        // Task 7: Idle wheel drain — reclaims resources for idle connections.
        let task_counter = counter_registry
            .register_nominal_task("task.idle_wheel", &variant)
            .with_registration_metadata(
                "task.idle_wheel",
                "Expires or re-schedules idle send contexts",
                "endpoint::tasks::send_worker",
            );
        let idle_wheel_task = send_idle_wheel_drain(
            idle_wheel_rx,
            idle_wheel_tx.clone(),
            clock.clone(),
            q_resolver_to_idle_wheel.clone(),
            send_caches.clone(),
            sender_idx_to_local.clone(),
            sender_local_addrs.clone(),
            idle_expired_completed_tx,
            peer_dead_tx.clone(),
            dead_peer_cooldown,
            counter_registry.register("idle.send.expired"),
            counter_registry.register("idle.send.rescheduled"),
            counter_registry.register_nominal_timer("idle.send.lifetime", &variant),
            budgets.idle_wheel,
            task_counter.clone(),
        );
        spawner.spawn_receiver_task(idle_wheel_task, Some(budgets.idle_wheel), task_counter);
    }

    // Per-socket assembler + send tasks.
    let asm_counters = AssemblerCounters::new(&counter_registry);
    for (local_id, st, immediate_rx, context_rx, gauge) in (
        send_sockets,
        socket_immediate_rxs,
        socket_context_rxs,
        q_wheel_to_assembler,
    )
        .join()
    {
        let sender_idx = st.sender_idx;
        let task_name = format!("task.assembler.send.{sender_idx}");
        let gauge = gauge.with_registration_metadata(
            format!("ch.wheel_to_assembler.send.{sender_idx}"),
            "Per-socket queue from tx wheel to assembler+socket sender task",
            "endpoint::tasks::send_worker",
        );
        let assembler_receiver = gauge
            .receiver(&task_name)
            .with_description("Assembler drains contexts assigned to this socket")
            .with_function("endpoint::tasks::send_worker");

        let clock = st.clock.clone();
        let tx_wheel_tx = GaugedSender::new(
            tx_wheel_tx.clone(),
            q_resolver_to_tx_wheel
                .sender(&task_name)
                .with_description("Assembler schedules immediate transmit wheel work")
                .with_function("endpoint::tasks::send_worker"),
        );
        let pto_wheel_tx = GaugedSender::new(
            pto_wheel_tx.clone(),
            q_resolver_to_pto_wheel
                .sender(&task_name)
                .with_description("Assembler schedules PTO wheel work")
                .with_function("endpoint::tasks::send_worker"),
        );
        let idle_wheel_tx = GaugedSender::new(
            idle_wheel_tx.clone(),
            q_resolver_to_idle_wheel
                .sender(&task_name)
                .with_description("Assembler updates idle wheel scheduling")
                .with_function("endpoint::tasks::send_worker"),
        );
        let cancelled_tx = cancelled_tx.clone().into_list_sender();
        let ack_completions_tx = ack_completions_tx.clone();
        let asm_counters = asm_counters.clone();
        let send_counters = send_caches[local_id].borrow().send_counters().clone();
        let context_rx = GaugedReceiver::new(context_rx, assembler_receiver);
        let rx = send_socket_assembler(
            immediate_rx,
            context_rx,
            clock,
            sender_idx,
            st.source_control_port,
            st.gso,
            st.pool,
            cancelled_tx,
            ack_completions_tx,
            asm_counters,
            send_counters,
            st.per_socket_send_rate,
            st.socket,
            immediate_tx.clone(),
            tx_wheel_tx,
            pto_wheel_tx,
            idle_wheel_tx,
        );
        let variant = format!("send.{sender_idx}");
        let task_counter = counter_registry
            .register_nominal_task("task.assembler", &variant)
            .with_registration_metadata(
                &task_name,
                "Assembles and sends packets for one socket sender id",
                "endpoint::tasks::send_worker",
            );
        spawner.spawn_receiver_task(
            rx.drain_budgeted_metered(Some(budgets.assembler), task_counter.clone()),
            Some(budgets.assembler),
            task_counter,
        );
    }

    // Task: invalidation drain — purge send caches on path secret revocation.
    // Routes to completed_tx (not cancelled) so CompletionDispatcher wakes streams.
    {
        let invalidation_counters = SendInvalidationCounters {
            unknown_path_secret_events: counter_registry.register("invalidation.ups.events"),
            unknown_path_secret_contexts: counter_registry.register("invalidation.ups.contexts"),
            unknown_path_secret_frames_failed: counter_registry
                .register("invalidation.ups.frames_failed"),
            stale_or_replay_events: counter_registry.register("invalidation.stale_replay.events"),
            stale_or_replay_contexts: counter_registry
                .register("invalidation.stale_replay.contexts"),
            stale_or_replay_frames_requeued: counter_registry
                .register("invalidation.stale_replay.frames_requeued"),
        };
        let retransmit_tx = frame_tx.clone();
        let rx = send_invalidation(
            invalidation_rx,
            send_caches,
            sender_idx_to_local.clone(),
            invalidation_completed_tx,
            retransmit_tx,
            invalidation_counters,
        );
        let task_counter = counter_registry
            .register_nominal_task("task.invalidation", &variant)
            .with_registration_metadata(
                "task.invalidation",
                "Purges revoked path secrets from send cache and emits completions",
                "endpoint::tasks::send_worker",
            );
        spawner.spawn_receiver_task(
            rx.drain_budgeted_metered(Some(budgets.invalidation), task_counter.clone()),
            Some(budgets.invalidation),
            task_counter,
        );
    }
}

/// Builds a receiver that resolves send contexts for incoming frame batches and dispatches
/// them to timing wheels for pacing and transmission.
///
/// For each `FrameBatch`, looks up the peer's `send::Context` (creating one if needed),
/// pushes the batch's frames into the context's pending queues, and enqueues the context
/// into the appropriate timing wheels (tx, pto, idle).
pub fn context_resolver<BatchRx, Clk, ImmW, TxW, PtoW, IdleW>(
    batch_rx: BatchRx,
    mut send_caches: IdMap<LocalSendSocketId, Rc<RefCell<send::Cache>>>,
    sender_idx_to_local: IdMap<LocalSenderId, LocalSendSocketId>,
    total_sender_ids: usize,
    clock: Clk,
    immediate_tx: ImmW,
    tx_wheel_tx: TxW,
    pto_wheel_tx: PtoW,
    idle_wheel_tx: IdleW,
) -> impl Receiver<()>
where
    BatchRx: Receiver<Entry<FrameBatch>>,
    Clk: precision::Clock + s2n_quic_core::time::Clock,
    ImmW: UnboundedSender<Rc<RefCell<send::Context>>>,
    TxW: UnboundedSender<Rc<RefCell<send::Context>>>,
    PtoW: UnboundedSender<Rc<RefCell<send::Context>>>,
    IdleW: UnboundedSender<Rc<RefCell<send::Context>>>,
{
    let rx = Map::new(
        batch_rx,
        move |batch: Entry<FrameBatch>| -> Option<(Rc<RefCell<send::Context>>, send::WheelInterest)> {
            let Some(sender_idx) = batch.sender_id() else {
                panic!("batch needs an assigned sender id");
            };
            let Some(local_id) = sender_idx_to_local.get(sender_idx).copied() else {
                panic!(
                    "sender id {} is out of range of {}",
                    sender_idx,
                    total_sender_ids
                );
            };
            let Some(cache) = send_caches.get_mut(local_id) else {
                panic!(
                    "sender id {} is out of range of {}",
                    sender_idx,
                    total_sender_ids
                );
            };

            let sender = {
                let mut cache = cache.borrow_mut();
                let cache = &mut *cache;
                match cache.get_or_insert(batch.path_secret_entry(), &clock) {
                    Ok(ctx) => ctx,
                    Err(error) => {
                        warn!(?error, peer = %batch.path_secret_entry().peer(), "dropping batch: send context not ready");
                        return None;
                    }
                }
            };

            let wheel_interest = {
                let mut ctx = sender.borrow_mut();
                ctx.push_batch(batch.into_inner(), &clock)
            };

            Some((sender, wheel_interest))
        },
    );
    let rx = Flatten::new(rx);
    send::WheelRouter::new(rx, immediate_tx, tx_wheel_tx, pto_wheel_tx, idle_wheel_tx)
}

/// Builds the ACK-processing receiver pipeline used by the send worker.
///
/// This pipeline decodes incoming ACK messages, updates send context state, and routes
/// the resulting wheel interest to tx/pto/idle schedulers.
#[allow(clippy::too_many_arguments)]
pub fn send_ack_processor<AckRx, Clk, Rand, C, ImmW, TxW, PtoW, IdleW>(
    ack_rx: AckRx,
    send_caches: IdMap<LocalSendSocketId, Rc<RefCell<send::Cache>>>,
    sender_idx_to_local: IdMap<LocalSenderId, LocalSendSocketId>,
    total_sender_ids: usize,
    clock: Clk,
    random: Rand,
    frame_tx: frame::SubmissionSender,
    completed_tx: C,
    cancelled_tx: C,
    invalid_sender_idx: counter::Counter,
    immediate_tx: ImmW,
    tx_wheel_tx: TxW,
    pto_wheel_tx: PtoW,
    idle_wheel_tx: IdleW,
) -> impl Receiver<()>
where
    AckRx: Receiver<Entry<msg::Sender>>,
    Clk: precision::Clock + s2n_quic_core::time::Clock,
    Rand: s2n_quic_core::random::Generator,
    C: UnboundedSender<Entry<Frame>>,
    ImmW: UnboundedSender<Rc<RefCell<send::Context>>>,
    TxW: UnboundedSender<Rc<RefCell<send::Context>>>,
    PtoW: UnboundedSender<Rc<RefCell<send::Context>>>,
    IdleW: UnboundedSender<Rc<RefCell<send::Context>>>,
{
    let rx = AckProcessor::new(
        ack_rx,
        send_caches,
        sender_idx_to_local,
        total_sender_ids,
        clock,
        random,
        frame_tx,
        completed_tx,
        cancelled_tx,
        invalid_sender_idx,
    );
    let rx = Flatten::new(rx);
    send::WheelRouter::new(rx, immediate_tx, tx_wheel_tx, pto_wheel_tx, idle_wheel_tx)
}

/// Builds the send-worker PTO timeout receiver pipeline.
///
/// For each context emitted by the PTO wheel, updates probe state and routes the resulting
/// wheel interest to tx/pto/idle schedulers.
pub fn send_pto_timeout<CtxRx, Clk, ImmW, TxW, PtoW, IdleW>(
    pto_context_rx: CtxRx,
    clock: Clk,
    immediate_tx: ImmW,
    tx_wheel_tx: TxW,
    pto_wheel_tx: PtoW,
    idle_wheel_tx: IdleW,
    tx_pto_check: counter::Counter,
    tx_pto_requested: counter::Counter,
    send_caches: IdMap<LocalSendSocketId, Rc<RefCell<send::Cache>>>,
    sender_idx_to_local: IdMap<LocalSenderId, LocalSendSocketId>,
) -> impl Receiver<()>
where
    CtxRx: Receiver<Rc<RefCell<send::Context>>>,
    Clk: precision::Clock,
    ImmW: UnboundedSender<Rc<RefCell<send::Context>>>,
    TxW: UnboundedSender<Rc<RefCell<send::Context>>>,
    PtoW: UnboundedSender<Rc<RefCell<send::Context>>>,
    IdleW: UnboundedSender<Rc<RefCell<send::Context>>>,
{
    let rx = Map::new(
        pto_context_rx,
        move |context: Rc<RefCell<send::Context>>| {
            tx_pto_check.add(1);
            let wheel_interest = {
                let mut ctx = context.borrow_mut();
                let requested = ctx.pto.probe_state.is_requested();
                let interest = ctx.on_pto_timeout(&clock);
                if !requested && ctx.pto.probe_state.is_requested() {
                    tx_pto_requested.add(1);
                    let local_idx = sender_idx_to_local[ctx.sender_idx];
                    let cache = send_caches[local_idx].borrow();
                    let counters = cache.send_counters();
                    counters
                        .tx_probe_backoff
                        .record_value(ctx.pto.backoff as u64);
                    if ctx.pto.backoff > 2 {
                        counters.on_probe_no_response();
                    }
                }
                interest
            };
            (context, wheel_interest)
        },
    );
    send::WheelRouter::new(rx, immediate_tx, tx_wheel_tx, pto_wheel_tx, idle_wheel_tx)
}

/// Builds a receiver that dispatches completed frames back to their owning writers.
///
/// Groups completed frames by completion channel and fires wakers in bulk, reducing
/// lock contention on the per-stream completion queue.
pub fn completion_dispatcher<R, WakerSink>(
    completed_rx: R,
    mut waker_sink: WakerSink,
) -> impl Receiver<()>
where
    R: Receiver<Entry<Frame>>,
    WakerSink: UnboundedSender<crate::flow::queue::AutoWake>,
{
    let rx = CompletionDispatcher::new(completed_rx);
    Map::new(rx, move |waker: crate::flow::queue::AutoWake| {
        let _ = waker_sink.send(waker);
    })
}

/// Builds a receiver that drops cancelled frames (frames whose writer has been dropped).
pub fn cancelled_drain<R>(cancelled_rx: R) -> impl Receiver<()>
where
    R: Receiver<Entry<Frame>>,
{
    Map::new(cancelled_rx, |_entry: Entry<Frame>| {})
}

/// Drains the send TX wheel and routes each expired context to its socket assembler queue.
pub async fn send_tx_wheel_drain<Clk, TxW>(
    tx_wheel_rx: intrusive::unsync::Receiver<send::TxWheelAdapter>,
    clock: Clk,
    input_gauge: QueueGauge,
    socket_context_txs: IdMap<LocalSendSocketId, TxW>,
    sender_idx_to_local: IdMap<LocalSenderId, LocalSendSocketId>,
    budget: usize,
    task_counter: counter::Task,
) where
    Clk: precision::Clock,
    TxW: UnboundedSender<Rc<RefCell<send::Context>>>,
{
    let timer = clock.timer();
    wheel_drain::<_, _, _, { wheel::MICROSECOND_GRANULARITY }>(
        tx_wheel_rx,
        timer,
        input_gauge,
        {
            let mut socket_context_txs = socket_context_txs;
            move |context: Rc<RefCell<send::Context>>| {
                let local_id = sender_idx_to_local[context.borrow().sender_idx];
                let _ = UnboundedSender::send(&mut socket_context_txs[local_id], context);
            }
        },
        budget,
        task_counter,
    )
    .await;
}

/// Drains a timing wheel, yielding each expired context to the provided callback.
///
/// The wheel continuously polls its inner receiver (insertion channel) to keep time
/// progressing and insert new entries. As entries expire, they are flattened and
/// handed to `on_expire` one at a time. This runs up to `budget` items per poll.
///
/// `input_gauge` tracks the wheel's depth: each expired item decrements it.
async fn wheel_drain<A, T, F, const GRANULARITY_US: u64>(
    rx: intrusive::unsync::Receiver<A>,
    timer: T,
    input_gauge: QueueGauge,
    mut on_expire: F,
    budget: usize,
    task_counter: counter::Task,
) where
    A: wheel::WheelAdapter,
    T: precision::Timer,
    F: FnMut(A::Pointer),
{
    let wheel: Wheel<A, T, _, GRANULARITY_US> = Wheel::new(rx.into_list_receiver(), timer);
    let rx = FlattenList::new(wheel);
    let rx = GaugedReceiver::new(
        rx,
        input_gauge
            .receiver("task.wheel_drain")
            .with_function("endpoint::tasks::wheel_drain"),
    );
    let rx = Map::new(rx, |item| on_expire(item));
    rx.drain_budgeted_metered(Some(budget), task_counter).await;
}

/// Builds the per-socket assembler + send pipeline for one send socket.
///
/// For each `Context` emitted from the tx wheel, this pipeline:
///
/// 1. Assembles pending frames into encrypted UDP datagrams via [`Assembler`].
/// 2. Routes any post-assembly wheel interest (tx reschedule, PTO arm, idle update)
///    back to the appropriate wheel senders via [`send::WheelRouter`].
/// 3. Paces the outgoing segment stream with [`Paced`].
/// 4. Sends each [`Segments`] batch over the socket via [`SocketSender`].
/// 5. Logs socket send errors without terminating the pipeline.
///
/// Returns a `Receiver<()>` — callers must drain it (typically via
/// `drain_budgeted_metered`) to make progress.
///
/// [`Assembler`]: crate::endpoint::combinator::Assembler
/// [`send::WheelRouter`]: crate::endpoint::send::WheelRouter
/// [`Segments`]: crate::socket::pool::descriptor::Segments
pub fn send_socket_assembler<ImmediateRx, ContextRx, Clk, Socket, C, A, ImmW, TxW, PtoW, IdleW>(
    immediate_rx: ImmediateRx,
    context_rx: ContextRx,
    clock: Clk,
    source_sender_id: LocalSenderId,
    source_control_port: u16,
    gso: Gso,
    pool: crate::socket::pool::Pool,
    cancelled_tx: C,
    ack_completions_tx: A,
    asm_counters: AssemblerCounters,
    send_counters: Rc<endpoint::counters::Send>,
    per_socket_send_rate: Rate,
    socket: Socket,
    immediate_tx: ImmW,
    tx_wheel_tx: TxW,
    pto_wheel_tx: PtoW,
    idle_wheel_tx: IdleW,
) -> impl Receiver<()>
where
    ImmediateRx: Receiver<Rc<RefCell<send::Context>>>,
    ContextRx: Receiver<Rc<RefCell<send::Context>>>,
    Clk: precision::Clock + Clone,
    Socket: crate::socket::send::Socket,
    C: UnboundedSender<Queue<Frame>>,
    A: UnboundedSender<Queue<msg::Sender>>,
    ImmW: UnboundedSender<Rc<RefCell<send::Context>>>,
    TxW: UnboundedSender<Rc<RefCell<send::Context>>>,
    PtoW: UnboundedSender<Rc<RefCell<send::Context>>>,
    IdleW: UnboundedSender<Rc<RefCell<send::Context>>>,
{
    let context_rx = PrioritySelect::new(immediate_rx, context_rx);
    let rx = Assembler::new(
        context_rx,
        clock.clone(),
        source_sender_id,
        source_control_port,
        gso,
        pool,
        cancelled_tx,
        ack_completions_tx,
        asm_counters,
        send_counters,
    );
    let rx = send::WheelRouter::new(rx, immediate_tx, tx_wheel_tx, pto_wheel_tx, idle_wheel_tx);
    let rx = Flatten::new(rx);
    let rx = Paced::new(rx, clock, per_socket_send_rate);
    let rx = SocketSender::new(rx, socket);
    let rx = InspectErr::new(rx, |(err, _segments)| {
        warn!(%err, "socket send error");
    });
    Map::new(rx, |_segments| {})
}

pub async fn send_idle_wheel_drain<Clk, WakerSink>(
    rx: intrusive::unsync::Receiver<send::IdleWheelAdapter>,
    idle_wheel_tx: intrusive::unsync::Sender<send::IdleWheelAdapter>,
    clock: Clk,
    input_gauge: QueueGauge,
    send_caches: IdMap<LocalSendSocketId, Rc<RefCell<send::Cache>>>,
    sender_idx_to_local: IdMap<LocalSenderId, LocalSendSocketId>,
    sender_local_addrs: IdMap<LocalSendSocketId, std::net::SocketAddr>,
    mut completed_tx: impl UnboundedSender<Entry<Frame>>,
    mut peer_dead_tx: WakerSink,
    dead_peer_cooldown: core::time::Duration,
    idle_expired: counter::Counter,
    idle_rescheduled: counter::Counter,
    idle_lifetime: counter::Timer,
    budget: usize,
    task_counter: counter::Task,
) where
    Clk: precision::Clock,
    WakerSink: UnboundedSender<Entry<PeerDead>>,
{
    let timer = clock.timer();
    wheel_drain::<_, _, _, { wheel::SECOND_GRANULARITY }>(
        rx,
        timer,
        input_gauge.clone(),
        {
            let mut idle_wheel_tx = GaugedSender::new(
                idle_wheel_tx,
                input_gauge
                    .sender("task.send_idle_wheel_drain")
                    .with_description("Idle wheel re-enqueues active send contexts")
                    .with_function("endpoint::tasks::send_idle_wheel_drain"),
            );
            move |context: Rc<RefCell<send::Context>>| {
                let now = clock.now();
                let ctx = context.borrow();

                // Compute the reschedule target. If it's in a future wheel tick,
                // reschedule and return early.
                if !ctx.is_peer_idle(now) {
                    let target = ctx.last_peer_activity + ctx.path_secret_entry.idle_timeout();
                    if target.nanos_since(now) >= wheel::SECOND_GRANULARITY_NANOS {
                        drop(ctx);
                        context.borrow_mut().idle_wheel.target_time = Some(target);
                        let _ = UnboundedSender::send(&mut idle_wheel_tx, context);
                        idle_rescheduled.add(1);
                        return;
                    }
                }

                // Expired: either is_peer_idle fired, or the target landed on
                // the current tick (within wheel granularity).
                let id = *ctx.path_secret_entry.id();
                let path_secret_entry = ctx.path_secret_entry.clone();
                let local_id = sender_idx_to_local[ctx.sender_idx];
                let lifetime = now.duration_since(ctx.created_at);
                let path_active = !ctx.path_secret_entry.is_idle_expired(now);
                let sender_idx = ctx.sender_idx;
                let peer_addr = ctx.peer_addr;
                let ever_responded = ctx.last_peer_activity.nanos != ctx.created_at.nanos;
                let has_inflight = ctx.inflight.has_inflight();
                let pto_backoff = ctx.pto.backoff;
                let packets_sent = ctx.next_packet_number.as_u64();
                drop(ctx);

                if path_active && has_inflight {
                    let cache = send_caches[local_id].borrow();
                    cache.send_counters().routing_asymmetry.add(1);
                    let local_addr = sender_local_addrs[local_id];
                    warn!(
                        %id,
                        %sender_idx,
                        %local_addr,
                        %peer_addr,
                        ever_responded,
                        pto_backoff,
                        packets_sent,
                        "send context idle but path still active — possible routing asymmetry"
                    );
                }

                // Mark the peer dead only when we have packets in flight that
                // were never acknowledged — that's evidence the peer is
                // unreachable. If there are no inflight packets, both sides
                // simply stopped talking (natural idle) and we must not mark dead.
                let marked_dead = has_inflight
                    && path_secret_entry.mark_dead_if_cooldown_elapsed(now, dead_peer_cooldown);

                let result = send_caches[local_id].borrow_mut().invalidate(
                    &id,
                    frame::FailureReason::PeerDead,
                    &mut completed_tx,
                );

                if let Some((drained, discarded_bytes)) = result {
                    let cache = send_caches[local_id].borrow();
                    let counters = cache.send_counters();
                    counters.on_inflight_drain_expire(drained as u64);
                    counters.on_inflight_leaked_on_invalidate(discarded_bytes as u64);
                }

                if marked_dead {
                    let _ = peer_dead_tx.send(Entry::new(PeerDead { path_secret_entry }));
                }

                idle_expired.add(1);
                idle_lifetime.record(lifetime);
            }
        },
        budget,
        task_counter,
    )
    .await;
}

pub async fn recv_idle_wheel_drain<Clk>(
    rx: intrusive::unsync::Receiver<endpoint::recv::IdleWheelAdapter>,
    idle_wheel_tx: intrusive::unsync::Sender<endpoint::recv::IdleWheelAdapter>,
    clock: Clk,
    input_gauge: QueueGauge,
    recv_cache: Rc<RefCell<endpoint::recv::Cache>>,
    idle_expired: counter::Counter,
    idle_rescheduled: counter::Counter,
    idle_lifetime: counter::Timer,
    budget: usize,
    task_counter: counter::Task,
) where
    Clk: precision::Clock,
{
    let timer = clock.timer();
    wheel_drain::<_, _, _, { wheel::SECOND_GRANULARITY }>(
        rx,
        timer,
        input_gauge.clone(),
        {
            let mut idle_wheel_tx = GaugedSender::new(
                idle_wheel_tx,
                input_gauge
                    .sender("task.recv_idle_wheel_drain")
                    .with_description("Idle wheel re-enqueues active recv contexts")
                    .with_function("endpoint::tasks::recv_idle_wheel_drain"),
            );
            move |context: Rc<RefCell<endpoint::recv::Context>>| {
                let now = clock.now();
                let ctx = context.borrow();

                if !ctx.path_entry.is_idle_expired(now) {
                    let target = ctx.path_entry.last_activity() + ctx.path_entry.idle_timeout();
                    if target.nanos_since(now) >= wheel::SECOND_GRANULARITY_NANOS {
                        drop(ctx);
                        context.borrow_mut().idle_wheel.target_time = Some(target);
                        let _ = UnboundedSender::send(&mut idle_wheel_tx, context);
                        idle_rescheduled.add(1);
                        return;
                    }
                }

                let key = endpoint::recv::Key {
                    id: *ctx.path_entry.id(),
                    remote_sender_id: ctx.remote_sender_id,
                };
                let lifetime = now.duration_since(ctx.created_at);
                drop(ctx);
                recv_cache.borrow_mut().remove(&key);
                // The recv side does not mark the peer dead. An idle recv context
                // only means the peer stopped sending — it cannot distinguish
                // "nothing to say" from "actually unreachable". Only the send
                // side (with unacknowledged inflight packets) has evidence of
                // peer death.
                idle_expired.add(1);
                idle_lifetime.record(lifetime);
            }
        },
        budget,
        task_counter,
    )
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
        warn!(%err, "socket recv error");
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
/// Builds a receiver that decrypts, deduplicates, and dispatches received packets.
///
/// For each packet from `packet_rx`, calls [`dispatch::process`] to decrypt, validate,
/// and route frames to flow queues. Dispatch errors are silently dropped — they represent
/// invalid/duplicate/unauthenticated packets which should not terminate the worker.
///
/// [`dispatch::process`]: crate::endpoint::dispatch::process
pub fn packet_dispatch<
    PacketRx,
    AckSender,
    AckBurstSender,
    IdleWheelSender,
    UpsSender,
    WakerSink,
    Clk,
    Route,
>(
    packet_rx: PacketRx,
    recv_cache: Rc<RefCell<endpoint::recv::Cache>>,
    mut ack_burst_tx: AckBurstSender,
    mut idle_wheel_tx: IdleWheelSender,
    path_secret_map: crate::path::secret::Map,
    acceptor_registry: crate::acceptor::Registry<crate::stream::PendingValidation>,
    frame_tx: frame::SubmissionSender,
    mut ack_sender: AckSender,
    queue_dispatcher: msg::queue::Dispatcher,
    counters: Arc<endpoint::counters::Dispatch>,
    clock: Clk,
    route: Route,
    mut waker_sink: WakerSink,
    ups_tx: UpsSender,
) -> impl Receiver<()>
where
    PacketRx: Receiver<crate::intrusive::Entry<Packet<descriptor::Filled>>>,
    AckSender: UnboundedSender<Entry<msg::Sender>>,
    AckBurstSender: UnboundedSender<Rc<RefCell<endpoint::recv::Context>>>,
    IdleWheelSender: UnboundedSender<Rc<RefCell<endpoint::recv::Context>>>,
    UpsSender: UnboundedSender<Entry<endpoint::ups::Response>>,
    WakerSink: UnboundedSender<crate::flow::queue::AutoWake>,
    Clk: s2n_quic_core::time::Clock + precision::Clock,
    Route: endpoint::routing::SenderRoute,
{
    let rx = Map::new(packet_rx, {
        let mut response_tx = frame_tx.clone();
        let mut queue_dispatcher = queue_dispatcher;
        let counters = counters.clone();
        let mut acceptor_local = acceptor_registry.local();

        move |packet| {
            counters.rx_data_pkt.add(1);
            dispatch::process(
                packet,
                &mut recv_cache.borrow_mut(),
                &mut ack_burst_tx,
                &mut idle_wheel_tx,
                &path_secret_map,
                &mut acceptor_local,
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
    InspectErr::new(rx, {
        let counters = counters;
        let mut ups_tx = ups_tx;
        move |err| on_packet_dispatch_error(&counters, &mut ups_tx, err)
    })
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
    counters: Arc<endpoint::counters::Dispatch>,
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
                counters.rx_ack_completion_impossible.add(1);
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
        if cfg!(debug_assertions) {
            assert_eq!(
                ctx.key(),
                key,
                "recv cache key/context mismatch in ack_completion"
            );
        }
        ctx.invariants();

        if let Some(submission) = ctx.on_ack_completion(recv_worker_id) {
            let mut pending_ack_entry = entry;
            *pending_ack_entry = msg::Sender::PendingAck(submission);
            let _ = ack_sender.send(pending_ack_entry);
        } else if ctx.ack_state.is_flushed() || ctx.ack_state.is_flushed_stale() {
            counters.rx_ack_completion_impossible.add(1);
        }
        ctx.invariants();
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
    recv_worker_id: endpoint::id::RecvDispatchWorkerId,
    counters: Arc<endpoint::counters::Dispatch>,
) -> impl Receiver<()>
where
    AckBurstRx: Receiver<Rc<RefCell<endpoint::recv::Context>>>,
    AckTx: UnboundedSender<Entry<msg::Sender>>,
{
    Map::new(
        ack_burst_rx,
        move |ctx_rc: Rc<RefCell<endpoint::recv::Context>>| {
            let mut ctx = ctx_rc.borrow_mut();
            let was_scheduled = ctx.ack_state.is_scheduled();
            ctx.invariants();
            if let Some(submission) = ctx.encode_and_flush(recv_worker_id) {
                let _ = ack_sender.send(Entry::new(msg::Sender::PendingAck(submission)));
            } else if was_scheduled {
                counters.rx_ack_state_impossible.add(1);
            }
            ctx.invariants();
        },
    )
}

fn on_packet_dispatch_error(
    counters: &endpoint::counters::Dispatch,
    ups_tx: &mut impl UnboundedSender<Entry<endpoint::ups::Response>>,
    err: dispatch::Error,
) {
    match err {
        dispatch::Error::PeerStateLookup {
            dest_addr,
            credentials,
            control_out,
        } => {
            counters.rx_process_err_peer_lookup.add(1);
            if !control_out.is_empty() {
                let response = endpoint::ups::Response {
                    dest_addr,
                    packet: control_out,
                };
                let _ = ups_tx.send(Entry::new(response));
            }
            debug!(
                ?credentials,
                "peer state lookup failed - queued UPS response"
            );
        }
        dispatch::Error::Decryption {
            credentials,
            packet_number,
        } => {
            counters.rx_process_err_decryption.add(1);
            debug!(
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
            trace!(
                ?credentials,
                pn = packet_number.as_u64(),
                "duplicate packet filtered"
            );
        }
        dispatch::Error::StaleKey {
            dest_addr,
            credentials,
            packet_number,
            control_out,
        } => {
            counters.rx_process_err_stale_key.add(1);
            if !control_out.is_empty() {
                let response = endpoint::ups::Response {
                    dest_addr,
                    packet: control_out,
                };
                let _ = ups_tx.send(Entry::new(response));
            }
            debug!(
                ?credentials,
                pn = packet_number.as_u64(),
                "stale key detected - key-id already seen or outside replay window"
            );
        }
        dispatch::Error::MissingSenderId => {
            counters.rx_process_err_missing_sender_id.add(1);
            warn!("packet missing routing info; expected SenderId");
        }
    }
}

// ── UPS send ──────────────────────────────────────────────────────────────

/// Drains the shared UPS queue, applies per-credential dedup, paces, and sends via socket.
pub fn ups_send<Rx, Socket, Clk>(
    rx: Rx,
    socket: Socket,
    clock: Clk,
    rate: Rate,
    dedup_capacity: usize,
    dedup_window: core::time::Duration,
    counters: endpoint::ups::Counters,
) -> impl Receiver<()>
where
    Rx: Receiver<Entry<endpoint::ups::Response>>,
    Socket: crate::socket::send::Socket,
    Clk: precision::Clock,
{
    use crate::time::precision::Timer;

    let timer = clock.timer();
    let dedup_counters = endpoint::ups::DedupCounters {
        suppressed: counters.dedup_suppressed.clone(),
    };
    let mut dedup = endpoint::ups::DedupFilter::new(dedup_capacity, dedup_window, dedup_counters);

    let rx = FilterMap::new(rx, move |entry: Entry<endpoint::ups::Response>| {
        let now = timer.now();
        if dedup.check(&entry, now) {
            Some(entry)
        } else {
            None
        }
    });
    let rx = Paced::new(rx, clock, rate);
    let rx = SocketSender::new(rx, socket);
    let send_error = counters.send_error;
    let rx = InspectErr::new(rx, move |(_err, _item)| {
        send_error.add(1);
    });
    let sent = counters.sent;
    Map::new(rx, move |_| {
        sent.add(1);
    })
}

// ── FrameReceiver ──────────────────────────────────────────────────────────

/// Adapts the frame submission channel's `poll_swap` into a `Receiver<()>` so
/// it can be drained via `drain_budgeted`. Each poll_recv performs one swap and
/// distributes the resulting frames into per-priority lane senders.
struct FrameReceiver<Tx> {
    frame_rx: SubmissionReceiver,
    staging: PriorityStorage,
    priority_list_txs: [Tx; Priority::LEVELS],
    q_router_to_batcher: [QueueGauge; Priority::LEVELS],
}

impl<Tx> Receiver<()> for FrameReceiver<Tx>
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

// ── Invalidation tasks ───────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct PeerDead {
    pub path_secret_entry: Arc<crate::path::secret::map::Entry>,
}

#[derive(Clone)]
pub struct PeerDeadCounters {
    pub events: counter::Counter,
    pub broadcasted: counter::Counter,
}

pub fn peer_dead_broadcast<R, WakerSink>(
    peer_dead_rx: R,
    mut queue_dispatcher: msg::queue::Dispatcher,
    mut waker_sink: WakerSink,
    counters: PeerDeadCounters,
) -> impl Receiver<()>
where
    R: Receiver<Entry<PeerDead>>,
    WakerSink: UnboundedSender<crate::flow::queue::AutoWake>,
{
    use crate::{endpoint::error::IDLE_TIMEOUT, flow};

    Map::new(peer_dead_rx, move |entry: Entry<PeerDead>| {
        counters.events.add(1);
        let peer_dead = entry.into_inner();
        let credential_id = *peer_dead.path_secret_entry.id();

        let request = flow::Request {
            credential_id,
            stream_id: None,
        };

        queue_dispatcher.send_both_by_request(
            &request,
            || {
                msg::Stream::Reset {
                    error_code: IDLE_TIMEOUT,
                }
                .into()
            },
            || {
                msg::Control::Reset {
                    error_code: IDLE_TIMEOUT,
                }
                .into()
            },
            |waker_a, waker_b| {
                let _ = waker_sink.send(waker_a);
                let _ = waker_sink.send(waker_b);
            },
        );
        counters.broadcasted.add(1);
    })
}

pub fn send_invalidation<R>(
    invalidation_rx: R,
    send_caches: IdMap<LocalSendSocketId, Rc<RefCell<send::Cache>>>,
    sender_idx_to_local: IdMap<LocalSenderId, LocalSendSocketId>,
    mut cancelled_tx: impl UnboundedSender<Entry<Frame>> + 'static,
    mut retransmit_tx: impl UnboundedSender<Entry<Frame>> + 'static,
    counters: SendInvalidationCounters,
) -> impl Receiver<()>
where
    R: Receiver<Entry<Invalidation>>,
{
    Map::new(
        invalidation_rx,
        move |entry: Entry<Invalidation>| match *entry {
            Invalidation::UnknownPathSecret { credential_id } => {
                counters.unknown_path_secret_events.add(1);
                for (_, cache) in &send_caches {
                    let mut cache = cache.borrow_mut();
                    if let Some((drained, discarded_bytes)) = cache.invalidate(
                        &credential_id,
                        frame::FailureReason::UnknownPathSecret,
                        &mut cancelled_tx,
                    ) {
                        counters.unknown_path_secret_contexts.add(1);
                        counters
                            .unknown_path_secret_frames_failed
                            .add(drained as u64);
                        let send_counters = cache.send_counters();
                        send_counters.on_inflight_drain_invalidate(drained as u64);
                        send_counters.on_inflight_leaked_on_invalidate(discarded_bytes as u64);
                    }
                }
            }
            Invalidation::StaleKey {
                credential_id,
                sender_id,
                rejected_key_id,
            } => {
                counters.stale_or_replay_events.add(1);
                let local_id = sender_idx_to_local.get(sender_id).copied();
                assert!(
                    local_id.is_some(),
                    "sender_id had no local sender_idx mapping; this should not occur in normal operation and may indicate sender_id_to_worker mapping drift"
                );
                let Some(local_id) = local_id else {
                    return;
                };
                let cache = send_caches.get(local_id);
                assert!(
                    cache.is_some(),
                    "sender_id resolved to a sender_idx not owned by this worker; this should not occur in normal operation and may indicate sender_id_to_worker mapping drift"
                );
                let Some(cache) = cache else {
                    return;
                };
                let mut cache = cache.borrow_mut();
                if let Some((drained, discarded_bytes)) = cache.invalidate_stale_key(
                    &credential_id,
                    sender_id,
                    rejected_key_id,
                    &mut retransmit_tx,
                ) {
                    counters.stale_or_replay_contexts.add(1);
                    counters.stale_or_replay_frames_requeued.add(drained as u64);
                    let send_counters = cache.send_counters();
                    send_counters.on_inflight_drain_invalidate(drained as u64);
                    send_counters.on_inflight_leaked_on_invalidate(discarded_bytes as u64);
                }
            }
        },
    )
}

pub fn recv_invalidation<R>(
    invalidation_rx: R,
    recv_cache: Rc<RefCell<endpoint::recv::Cache>>,
) -> impl Receiver<()>
where
    R: Receiver<Entry<Invalidation>>,
{
    Map::new(
        invalidation_rx,
        move |entry: Entry<Invalidation>| match *entry {
            Invalidation::UnknownPathSecret { credential_id } => {
                recv_cache.borrow_mut().invalidate_by_id(&credential_id);
            }
            msg => {
                debug_assert!(
                    false,
                    "recv invalidation only accepts UnknownPathSecret invalidations: {msg:?}"
                );
            }
        },
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Invalidation {
    UnknownPathSecret {
        credential_id: crate::credentials::Id,
    },
    StaleKey {
        credential_id: crate::credentials::Id,
        sender_id: LocalSenderId,
        /// The key_id that was rejected. Only invalidate send contexts whose
        /// key_id <= this value; contexts already advanced past it are fine.
        rejected_key_id: VarInt,
    },
}

pub fn invalidation_validator<R, Tx>(
    raw_rx: R,
    path_secret_map: crate::path::secret::Map,
    mut send_txs: IdMap<SendWorkerId, Tx>,
    mut recv_txs: IdMap<RecvDispatchWorkerId, Tx>,
    sender_id_to_worker: IdMap<LocalSenderId, SendWorkerId>,
    counters: ValidatorInvalidationCounters,
) -> impl Receiver<()>
where
    R: Receiver<Entry<descriptor::Filled>>,
    Tx: UnboundedSender<Entry<Invalidation>>,
{
    use crate::packet::secret_control;
    use s2n_codec::DecoderBufferMut;

    Map::new(raw_rx, move |mut entry: Entry<descriptor::Filled>| {
        let remote_address = entry.remote_address().get();
        let peer = std::net::SocketAddr::from(remote_address);
        let buf = entry.payload_mut();
        let decoder = DecoderBufferMut::new(buf);
        let Ok((packet, _)) = secret_control::Packet::decode(decoder) else {
            debug!(%peer, "ignored invalidation control packet: decode failed");
            return;
        };
        let Some(invalidation) = (match packet {
            secret_control::Packet::UnknownPathSecret(packet) => {
                let Some(validated) =
                    path_secret_map.handle_unknown_path_secret_packet(&packet, &peer)
                else {
                    debug!(%peer, "ignored invalidation control packet: unknown path secret rejected");
                    return;
                };

                let local_id = validated.credential_id.for_peer();
                debug!(
                    %peer,
                    credential_id = %local_id,
                    sinks = send_txs.len() + recv_txs.len(),
                    "validated unknown path secret invalidation"
                );
                counters.unknown_path_secret_validated.add(1);
                Some(Invalidation::UnknownPathSecret {
                    credential_id: local_id,
                })
            }
            secret_control::Packet::StaleKey(packet) => {
                let Some(validated) = path_secret_map.handle_stale_key_packet(&packet, &peer)
                else {
                    debug!(%peer, "ignored invalidation control packet: stale key rejected");
                    return;
                };
                let Some(sender_id) = validated.sender_id else {
                    debug!(%peer, "ignored invalidation control packet: stale key missing sender_id");
                    return;
                };
                let local_id = validated.credential_id.for_peer();
                // convert to a local sender ID since we're receiving the packet
                let sender_id = LocalSenderId::new(sender_id);
                debug!(
                    %peer,
                    credential_id = %local_id,
                    %sender_id,
                    sinks = send_txs.len(),
                    "validated stale key invalidation"
                );
                counters.stale_key_validated.add(1);
                Some(Invalidation::StaleKey {
                    credential_id: local_id,
                    sender_id,
                    rejected_key_id: validated.min_key_id,
                })
            }
            secret_control::Packet::ReplayDetected(packet) => {
                let Some(validated) = path_secret_map.handle_replay_detected_packet(&packet, &peer)
                else {
                    debug!(%peer, "ignored invalidation control packet: replay detected rejected");
                    return;
                };
                let Some(sender_id) = validated.sender_id else {
                    debug!(%peer, "ignored invalidation control packet: replay detected missing sender_id");
                    return;
                };
                let local_id = validated.credential_id.for_peer();
                // convert to a local sender ID since we're receiving the packet
                let sender_id = LocalSenderId::new(sender_id);
                debug!(
                    %peer,
                    credential_id = %local_id,
                    sender_id = %sender_id,
                    sinks = send_txs.len(),
                    "validated replay detected invalidation"
                );
                counters.replay_detected_validated.add(1);
                Some(Invalidation::StaleKey {
                    credential_id: local_id,
                    sender_id,
                    rejected_key_id: validated.rejected_key_id,
                })
            }
        }) else {
            return;
        };

        match invalidation {
            Invalidation::UnknownPathSecret { .. } => {
                for (_, tx) in &mut send_txs {
                    let _ = tx.send(Entry::new(invalidation));
                }
                for (_, tx) in &mut recv_txs {
                    let _ = tx.send(Entry::new(invalidation));
                }
            }
            Invalidation::StaleKey { sender_id, .. } => {
                let worker_id = sender_id_to_worker.get(sender_id).copied();
                assert!(
                    worker_id.is_some(),
                    "stale/replay invalidation sender_id had no sender_id_to_worker mapping; this indicates an invalid endpoint worker configuration"
                );
                let Some(worker_id) = worker_id else {
                    return;
                };
                let tx = send_txs.get_mut(worker_id);
                assert!(
                    tx.is_some(),
                    "stale/replay invalidation worker mapping exceeded send_txs length; sender_id_to_worker and send worker wiring are inconsistent"
                );
                let Some(tx) = tx else {
                    return;
                };
                let _ = tx.send(Entry::new(invalidation));
            }
        }
    })
}

#[derive(Clone)]
pub struct SendInvalidationCounters {
    pub unknown_path_secret_events: counter::Counter,
    pub unknown_path_secret_contexts: counter::Counter,
    pub unknown_path_secret_frames_failed: counter::Counter,
    pub stale_or_replay_events: counter::Counter,
    pub stale_or_replay_contexts: counter::Counter,
    pub stale_or_replay_frames_requeued: counter::Counter,
}

#[derive(Clone)]
pub struct ValidatorInvalidationCounters {
    pub unknown_path_secret_validated: counter::Counter,
    pub stale_key_validated: counter::Counter,
    pub replay_detected_validated: counter::Counter,
}

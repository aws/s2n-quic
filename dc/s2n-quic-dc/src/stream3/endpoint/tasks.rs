// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    clock::precision,
    datagram::batch::Priority,
    intrusive_queue::{Entry, Queue},
    socket::{
        channel::{
            cell,
            intrusive_queue::{self, unsync},
            FlattenQueue, FlattenSegments, InspectErr, Map, Paced, Priority as PriorityRx,
            Receiver, ReceiverExt as _, RouterAdapter, Sender, SocketReceiver, SocketSender,
            UnboundedSender,
        },
        pool::descriptor,
        rate::Rate,
    },
    stream2::spawner::LocalSpawner,
    stream3::{
        endpoint::{
            self, ack,
            combinator::{
                Assembler, BatchFramesByPathSecret, FrameBatch, PathSecretMapEntry, PickTwo,
            },
            dispatch, msg, send,
            worker::ChannelRouter,
        },
        frame::{Frame, PriorityStorage, SubmissionReceiver},
    },
};
use core::{
    future::poll_fn,
    task::{self, Poll},
};
use s2n_quic_core::{assume, ready, varint::VarInt};
use std::{cell::RefCell, rc::Rc};

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
pub fn frame_dispatch<S, Rand, Clk>(
    spawner: &mut impl LocalSpawner,
    mut frame_rx: SubmissionReceiver,
    worker_senders: Vec<S>,
    random: Rand,
    clock: Clk,
    overall_send_rate: Rate,
    budget: usize,
) where
    S: UnboundedSender<Entry<FrameBatch>> + 'static,
    Rand: FnMut(usize) -> usize + 'static,
    Clk: precision::Clock + 'static,
{
    let mut priority_batch_rxs = Vec::with_capacity(Priority::LEVELS);
    let mut priority_list_txs: [_; Priority::LEVELS] = core::array::from_fn(|_| {
        let (tx, rx) = intrusive_queue::unsync::new::<Frame>();
        let rx = BatchFramesByPathSecret::new(rx);
        let rx = Map::new(rx, Entry::new);
        priority_batch_rxs.push(rx);
        tx.into_list_sender()
    });

    // Task 1: fixed-cost priority routing.
    spawner.spawn({
        let mut staging = PriorityStorage::default();
        poll_fn(move |cx| match frame_rx.poll_swap(cx, &mut staging) {
            Poll::Ready(None) => Poll::Ready(()),
            Poll::Pending => Poll::Pending,
            Poll::Ready(Some(())) => {
                for (queue, tx) in staging.iter_mut().zip(&mut priority_list_txs) {
                    if !queue.is_empty() {
                        let _ = UnboundedSender::send(tx, core::mem::take(queue));
                    }
                }
                cx.waker().wake_by_ref();
                Poll::Pending
            }
        })
    });

    // Task 2: batch → Entry → priority merge → pace → pick-two to workers.
    spawner.spawn(async move {
        let rx = PriorityRx::new(priority_batch_rxs);
        let rx = Paced::new(rx, clock, overall_send_rate);
        let rx = PickTwo::new(rx, worker_senders, random);
        rx.drain_budgeted(Some(budget)).await;
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
pub fn send_worker<Socket, Clk, Rand>(
    spawner: &mut impl LocalSpawner,
    batch_rx: impl Receiver<Queue<FrameBatch>> + 'static,
    ack_rx: impl Receiver<Queue<msg::Sender>> + 'static,
    total_sender_ids: usize,
    send_sockets: Vec<endpoint::SendSocketParts<Socket, Clk, Rand>>,
    budget: usize,
) where
    Socket: crate::socket::send::Socket + 'static,
    Clk: precision::Clock + s2n_quic_core::time::Clock + Clone + 'static,
    Rand: crate::random::Generator + 'static,
{
    let num_sockets = send_sockets.len();

    // Per-socket unsync cell: context resolver pushes FrameBatch here,
    // per-socket assembler+send task drains it.
    let (socket_batch_txs, socket_batch_rxs): (Vec<_>, Vec<_>) = (0..num_sockets)
        .map(|_| cell::unsync::new::<FrameBatch>())
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
                st.inflight_gauge.clone(),
                st.sender_idx,
            )))
        })
        .collect();

    let (tx_wheel_tx, tx_wheel_rx) = unsync::new_with_adapter();
    let (pto_wheel_tx, pto_wheel_rx) = unsync::new_with_adapter();
    let (idle_wheel_tx, idle_wheel_rx) = unsync::new_with_adapter();

    // Task 1: context resolver — drain batch_rx, resolve to context, push frames.
    spawner.spawn({
        let send_caches = send_caches.clone();
        let mut socket_batch_txs = socket_batch_txs;
        let rx = batch_rx;
        let mut tx_wheel_tx = tx_wheel_tx.clone();
        let mut pto_wheel_tx = pto_wheel_tx.clone();
        let mut idle_whel_tx = idle_wheel_tx.clone();
        async move {
            let rx = FlattenQueue::new(rx);
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
                    sender.push_batch(batch)
                };

                if wheel_interest.transmission {
                    let _ = UnboundedSender::send(&mut tx_wheel_tx, sender);
                }

                if wheel_interest.pto {
                    let _ = UnboundedSender::send(&mut pto_wheel_tx, sender);
                }

                if wheel_interest.idle_timeout {
                    let _ = UnboundedSender::send(&mut idle_wheel_tx, sender);
                }
            });
            rx.drain_budgeted(Some(budget)).await;
        }
    });

    // Task 2: ACK processor.
    spawner.spawn({
        let send_caches = send_caches.clone();
        let rx = ack_rx;
        async move {
            let rx = FlattenQueue::new(rx);
            let rx = Map::new(rx, move |entry: Entry<msg::Sender>| {
                let msg::Sender::Ack {
                    local_sender_id,
                    path_secret_entry,
                    payload,
                } = entry.into_inner();

                let sender_idx = local_sender_id.as_u64() as usize;

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
                    // We don't want to create a new entry if it's gone since this is for processing acks
                    cache.get(&path_secret_entry.id())
                };

                let Some(sender) = sender else {
                    tracing::debug!(%sender_idx, id = %path_secret_entry.id(), "Sender state gone for ACK");
                    return;
                };

                {
                    let context = sender.borrow_mut();

                    // TODO process the ack and potentially insert into the wheels if the wheel interest says so
                }
            });
            rx.drain_budgeted(Some(budget)).await;
        }
    });

    // Per-socket assembler + send tasks.
    for (st, batch_rx) in send_sockets.into_iter().zip(socket_batch_rxs) {
        let source_sender_id = VarInt::new(st.sender_idx as u64).unwrap();
        let local_idx = sender_idx_to_local[st.sender_idx];
        let send_cache = send_caches[local_idx].clone();

        spawner.spawn({
            let clock = st.clock.clone();
            async move {
                let rx = Assembler::new(
                    batch_rx,
                    clock,
                    source_sender_id,
                    st.source_control_port,
                    st.gso,
                    st.pool,
                );
                let rx = SocketSender::new(rx, st.socket);
                let rx = InspectErr::new(rx, |(err, _segments)| {
                    tracing::warn!(%err, "socket send error");
                });
                let rx = Map::new(rx, |_segments| {});
                rx.drain_budgeted(Some(budget)).await;
            }
        });
    }
}

// ── ACK Processing ───────────────────────────────────────────────────────────

/// Decodes a single ACK entry and processes it against the send cache.
fn process_ack_entry<Clk, Rand>(
    entry: Entry<msg::Sender>,
    send_cache: &mut endpoint::send::Cache,
    clock: &Clk,
    random: &mut Rand,
    frame_tx: &mut crate::stream3::frame::SubmissionSender,
) where
    Clk: s2n_quic_core::time::Clock + ?Sized,
    Rand: crate::random::Generator,
{
    let msg::Sender::Ack {
        local_sender_id: _,
        path_secret_entry,
        mut payload,
    } = entry.into_inner();

    let ctx_rc = send_cache.get_or_insert(&path_secret_entry);
    let frames_iter = crate::packet::control::decoder::ControlFramesMut::new(&mut payload);

    let mut acked_sink = CancelledFrameSink;
    let mut lost_queue: Queue<Frame> = Queue::new();
    let mut lost_sink = QueueSink(&mut lost_queue);
    let mut cancelled_sink = CancelledFrameSink;

    for frame in frames_iter {
        let Ok(frame) = frame else {
            tracing::debug!("failed to decode control frame in ACK payload");
            break;
        };

        match frame {
            s2n_quic_core::frame::FrameMut::Ack(ack_frame) => {
                let mut ctx = ctx_rc.borrow_mut();
                ack::process_ack(
                    &ack_frame,
                    &mut ctx,
                    &mut acked_sink,
                    &mut lost_sink,
                    &mut cancelled_sink,
                    clock,
                    random,
                );
            }
            s2n_quic_core::frame::FrameMut::Padding(_)
            | s2n_quic_core::frame::FrameMut::Ping(_) => {}
            frame => {
                tracing::debug!(?frame, "unexpected control frame type in ACK payload");
            }
        }
    }

    if !lost_queue.is_empty() {
        let _ = frame_tx.send_batch(lost_queue);
    }
}

// ── Helper Sinks ─────────────────────────────────────────────────────────────

/// Sink that drops frames (completions fire on drop).
struct CancelledFrameSink;

impl UnboundedSender<Queue<Frame>> for CancelledFrameSink {
    fn send(&mut self, _value: Queue<Frame>) -> Result<(), Queue<Frame>> {
        Ok(())
    }
}

/// Sink that appends frames into a local queue for retransmission.
struct QueueSink<'a>(&'a mut Queue<Frame>);

impl UnboundedSender<Queue<Frame>> for QueueSink<'_> {
    fn send(&mut self, mut value: Queue<Frame>) -> Result<(), Queue<Frame>> {
        self.0.append(&mut value);
        Ok(())
    }
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
pub async fn socket_recv_task<Socket, Tx>(
    socket: Socket,
    pool: crate::socket::pool::Pool,
    tx: Tx,
    decode_error_counter: crate::counter::Counter,
    budget: usize,
) where
    Socket: crate::socket::recv::Socket,
    Tx: UnboundedSender<
        crate::intrusive_queue::Entry<crate::packet::datagram::decoder::Packet<descriptor::Filled>>,
    >,
{
    let rx = SocketReceiver::new(socket, pool);
    // SocketReceiver yields io::Result<Segments>; InspectErr logs errors and unwraps to Segments.
    let rx = InspectErr::new(rx, |err| {
        tracing::warn!(%err, "socket recv error");
    });
    let rx = FlattenSegments::new(rx);
    let router = ChannelRouter {
        tx,
        decode_error_counter,
    };
    RouterAdapter::new(rx, router)
        .drain_budgeted(Some(budget))
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
pub async fn packet_dispatch_task<PacketRx, AckTx, Clk>(
    packet_rx: PacketRx,
    recv_cache: Rc<RefCell<endpoint::recv::Cache>>,
    path_secret_map: crate::path::secret::Map,
    acceptor_registry: crate::acceptor::Registry<crate::stream3::Stream>,
    frame_tx: crate::stream3::frame::SubmissionSender,
    ack_sender: AckTx,
    queue_dispatcher: msg::queue::Dispatcher,
    counters: endpoint::counters::Dispatch,
    clock: Clk,
    budget: usize,
) where
    PacketRx: Receiver<
        crate::intrusive_queue::Entry<crate::packet::datagram::decoder::Packet<descriptor::Filled>>,
    >,
    AckTx: UnboundedSender<msg::Sender>,
    Clk: s2n_quic_core::time::Clock + precision::Clock,
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
            )
        }
    });
    let rx = InspectErr::new(rx, {
        let counters = counters;
        move |err| on_packet_dispatch_error(&counters, err)
    });
    rx.drain_budgeted(Some(budget)).await;
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

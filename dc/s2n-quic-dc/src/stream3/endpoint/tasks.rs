// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    datagram::batch::Priority,
    intrusive_queue::{Entry, Queue},
    socket::channel::{Receiver, Sender},
    stream3::{
        endpoint::combinator::{BatchFramesByPathSecret, FrameBatch, PathSecretMapEntry, PickTwo},
        frame::{Frame, PriorityStorage},
    },
};
use core::{
    future::poll_fn,
    task::{self, Poll},
};

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
    spawner: &mut impl crate::stream2::spawner::LocalSpawner,
    mut frame_rx: crate::stream3::frame::SubmissionReceiver,
    socket_senders: Vec<S>,
    random: Rand,
    clock: Clk,
    overall_send_rate: crate::socket::rate::Rate,
    budget: usize,
) where
    S: Sender<FrameBatch> + 'static,
    Rand: FnMut(usize) -> usize + 'static,
    Clk: crate::clock::precision::Clock + 'static,
{
    use crate::socket::channel::{intrusive_queue, Paced, Priority as PriorityRx};

    // Create one unbounded unsync channel per priority level.
    // Task 1 sends whole `Queue<Frame>` lists via `ListSender`; Task 2 pops individual
    // frames via the plain `Receiver`.  Both tasks run on the same worker, so Rc-based
    // (!Send) channels are correct here.
    let mut priority_frame_rxs = Vec::with_capacity(Priority::LEVELS);
    let mut priority_list_txs: [_; Priority::LEVELS] = core::array::from_fn(|_| {
        let (tx, rx) = intrusive_queue::unsync::new::<Frame>();
        priority_frame_rxs.push(BatchFramesByPathSecret::new(rx));
        tx.into_list_sender()
    });

    // Task 1: fixed-cost priority routing.
    //
    // A persistent `staging` PriorityStorage (pre-allocated once) avoids heap allocation on
    // the hot path.  Each poll calls `poll_swap` once to pointer-swap the next ready shard's
    // Box into `staging`, then appends each non-empty priority queue to the matching
    // per-priority ListSender.  After processing one shard the task yields to the executor
    // (one shard per poll, matching the behaviour of `drain()`).
    spawner.spawn({
        let mut staging = PriorityStorage::default();
        poll_fn(move |cx| {
            match frame_rx.poll_swap(cx, &mut staging) {
                task::Poll::Ready(None) => task::Poll::Ready(()),
                task::Poll::Pending => task::Poll::Pending,
                task::Poll::Ready(Some(())) => {
                    for (queue, tx) in staging.iter_mut().zip(&mut priority_list_txs) {
                        if !queue.is_empty() {
                            let _ = crate::socket::channel::UnboundedSender::send(
                                tx,
                                core::mem::take(queue),
                            );
                        }
                    }
                    // Yield after processing one shard to give other tasks a turn.
                    cx.waker().wake_by_ref();
                    task::Poll::Pending
                }
            }
        })
    });

    // Task 2: batch each priority lane independently, merge in urgency order,
    // apply pacing, then route to send sockets via PickTwo.
    //
    // Pipeline: [per-priority-rx[i] → BatchFramesByPathSecret] → Priority → Paced → PickTwo
    spawner.spawn(async move {
        use crate::socket::channel::ReceiverExt as _;
        let rx = PriorityRx::new(priority_frame_rxs);
        let rx = Paced::new(rx, clock, overall_send_rate);
        let rx = PickTwo::new(rx, socket_senders, random);
        rx.drain_budgeted(Some(budget)).await;
    });
}

/// Spawns per-socket send tasks: assembly + socket send, and ACK processing.
///
/// Creates two cooperating tasks on `spawner`'s worker:
///
/// - **Assembler + sender** (Task 1): pulls [`FrameBatch`] items from `batch_rx`, resolves
///   each to a per-peer [`send::Context`] via a shared cache, pushes frames onto the context's
///   pending queue, then calls [`assemble`] in a loop to pack + encrypt frames into GSO
///   segments. Each assembled [`Segments`] is sent through the socket via [`SocketSender`].
///
/// - **ACK processor** (Task 2): pulls [`msg::Sender::Ack`] messages from `ack_rx`, decodes
///   the control frame payload, and feeds each ACK frame into [`process_ack`] which updates
///   RTT, CCA, and runs loss detection. Lost frames are retransmitted via `frame_tx`.
///
/// Both tasks share a [`send::Cache`] via `Rc<RefCell<_>>` since they run on the same worker.
///
/// [`send::Context`]: crate::stream3::endpoint::send::Context
/// [`assemble`]: crate::stream3::endpoint::assemble::assemble
/// [`process_ack`]: crate::stream3::endpoint::ack::process_ack
/// [`Segments`]: crate::socket::pool::descriptor::Segments
/// [`SocketSender`]: crate::socket::channel::SocketSender
///
/// # Pipeline overview
///
/// ```text
/// Task 1 (assembler + sender):
///   batch_rx
///     → Assembler (resolve context, push frames, assemble segments)
///     → SocketSender (send via UDP socket)
///     → InspectErr (log send errors)
///     → drain_budgeted
///
/// Task 2 (ACK processor):
///   ack_rx
///     → Map (decode control frames, process_ack, retransmit lost)
///     → drain_budgeted
/// ```
///
/// # TODO: missing pipeline stages
///
/// - **Per-socket pacer** (`Paced`): enforces a per-socket send-rate cap after assembly
///   and before the actual socket write, preventing burst sending.
///
/// - **PTO wheel injection**: when CCA schedules a probe timeout, the context's PTO deadline
///   is registered with a per-worker `Wheel`; when the wheel fires, a probe batch is generated
///   and pumped back into the frame submission channel.
///
/// - **Metrics**: `tx` packet counter, `tx:bytes` byte counter, per-socket queue depth gauge.
pub fn socket_send<Socket, BatchRx, AckRx, Clk, Rand>(
    spawner: &mut impl crate::stream2::spawner::LocalSpawner,
    socket: Socket,
    batch_rx: BatchRx,
    ack_rx: AckRx,
    sender_idx: usize,
    source_control_port: u16,
    gso: s2n_quic_platform::features::Gso,
    pool: crate::socket::pool::Pool,
    clock: Clk,
    random: Rand,
    frame_tx: crate::stream3::frame::SubmissionSender,
    inflight_gauge: crate::counter::QueueGauge,
    budget: usize,
) where
    Socket: crate::socket::send::Socket + 'static,
    BatchRx: Receiver<FrameBatch> + 'static,
    AckRx: Receiver<Entry<crate::stream3::endpoint::msg::Sender>> + 'static,
    Clk: crate::clock::precision::Clock + s2n_quic_core::time::Clock + Clone + 'static,
    Rand: crate::random::Generator + 'static,
{
    use crate::{
        socket::channel::{InspectErr, Map, ReceiverExt as _, SocketSender},
        stream3::endpoint::send,
    };
    use s2n_quic_core::varint::VarInt;
    use std::{cell::RefCell, rc::Rc};

    let source_sender_id = VarInt::new(sender_idx as u64).unwrap();
    let send_cache = Rc::new(RefCell::new(send::Cache::new(inflight_gauge, sender_idx)));

    // Task 1: assemble frames into encrypted segments and send via socket.
    spawner.spawn({
        let send_cache = send_cache.clone();
        let clock = clock.clone();
        async move {
            let rx = Assembler::new(
                batch_rx,
                send_cache,
                clock,
                source_sender_id,
                source_control_port,
                gso,
                pool,
            );
            let rx = SocketSender::new(rx, socket);
            let rx = InspectErr::new(rx, |(err, _segments)| {
                tracing::warn!(%err, "socket send error");
            });
            let rx = Map::new(rx, |_segments| {});
            rx.drain_budgeted(Some(budget)).await;
        }
    });

    // Task 2: decode ACK messages and drive loss recovery.
    spawner.spawn({
        let mut random = random;
        let mut frame_tx = frame_tx;
        async move {
            let rx = Map::new(
                ack_rx,
                move |entry: Entry<crate::stream3::endpoint::msg::Sender>| {
                    process_ack_entry(
                        entry,
                        &mut send_cache.borrow_mut(),
                        &clock,
                        &mut random,
                        &mut frame_tx,
                    );
                },
            );
            rx.drain_budgeted(Some(budget)).await;
        }
    });
}

// ── Assembler Receiver ───────────────────────────────────────────────────────

/// A [`Receiver`] adapter that resolves frame batches to per-peer contexts, pushes frames,
/// and yields assembled [`Segments`] ready for socket transmission.
///
/// Internally buffers the active context between polls: after pushing a batch's frames, it
/// calls [`assemble`] repeatedly until the CCA window fills, yielding one `Segments` per
/// poll. When the context is drained, it pulls the next batch from `inner`.
///
/// [`assemble`]: crate::stream3::endpoint::assemble::assemble
struct Assembler<R, Clk> {
    inner: R,
    send_cache: std::rc::Rc<std::cell::RefCell<crate::stream3::endpoint::send::Cache>>,
    active_ctx: Option<std::rc::Rc<std::cell::RefCell<crate::stream3::endpoint::send::Context>>>,
    clock: Clk,
    source_sender_id: s2n_quic_core::varint::VarInt,
    source_control_port: u16,
    gso: s2n_quic_platform::features::Gso,
    pool: crate::socket::pool::Pool,
    header_buf: Vec<u8>,
    cancelled_tx: CancelledFrameSink,
}

impl<R, Clk> Assembler<R, Clk> {
    fn new(
        inner: R,
        send_cache: std::rc::Rc<std::cell::RefCell<crate::stream3::endpoint::send::Cache>>,
        clock: Clk,
        source_sender_id: s2n_quic_core::varint::VarInt,
        source_control_port: u16,
        gso: s2n_quic_platform::features::Gso,
        pool: crate::socket::pool::Pool,
    ) -> Self {
        Self {
            inner,
            send_cache,
            active_ctx: None,
            clock,
            source_sender_id,
            source_control_port,
            gso,
            pool,
            header_buf: Vec::new(),
            cancelled_tx: CancelledFrameSink,
        }
    }
}

impl<R, Clk> Receiver<crate::socket::pool::descriptor::Segments> for Assembler<R, Clk>
where
    R: Receiver<FrameBatch>,
    Clk: crate::clock::precision::Clock,
{
    fn poll_recv(
        &mut self,
        cx: &mut task::Context<'_>,
    ) -> Poll<Option<crate::socket::pool::descriptor::Segments>> {
        use crate::stream3::endpoint::assemble;

        loop {
            // Try to assemble from the active context first.
            if let Some(ctx_rc) = &self.active_ctx {
                let mut ctx = ctx_rc.borrow_mut();
                if ctx.has_pending() {
                    if let Some(segments) = assemble::assemble(
                        &mut ctx,
                        &self.clock,
                        self.source_sender_id,
                        self.source_control_port,
                        &self.gso,
                        &self.pool,
                        &mut self.header_buf,
                        &mut self.cancelled_tx,
                    ) {
                        return Poll::Ready(Some(segments));
                    }
                }
                // Context drained (CCA full or no pending frames) — clear and pull next batch.
                drop(ctx);
                self.active_ctx = None;
            }

            // Pull the next batch from the inner receiver.
            let Some(batch) = (match self.inner.poll_recv(cx) {
                Poll::Ready(v) => v,
                Poll::Pending => return Poll::Pending,
            }) else {
                return Poll::Ready(None);
            };

            let ctx_rc = self
                .send_cache
                .borrow_mut()
                .get_or_insert(batch.path_secret_entry());
            {
                let mut ctx = ctx_rc.borrow_mut();
                for frame in batch.into_queue() {
                    ctx.push_frame(frame);
                }
            }
            self.active_ctx = Some(ctx_rc);
            // Loop back to attempt assembly from the newly-loaded context.
        }
    }

    fn on_consumed(&mut self, bytes: u64) {
        self.inner.on_consumed(bytes);
    }
}

// ── ACK Processing ───────────────────────────────────────────────────────────

/// Decodes a single ACK entry and processes it against the send cache.
fn process_ack_entry<Clk, Rand>(
    entry: Entry<crate::stream3::endpoint::msg::Sender>,
    send_cache: &mut crate::stream3::endpoint::send::Cache,
    clock: &Clk,
    random: &mut Rand,
    frame_tx: &mut crate::stream3::frame::SubmissionSender,
) where
    Clk: s2n_quic_core::time::Clock + ?Sized,
    Rand: crate::random::Generator,
{
    use crate::{
        intrusive_queue::Queue,
        stream3::endpoint::{ack, msg},
    };

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

impl crate::socket::channel::UnboundedSender<Queue<Frame>> for CancelledFrameSink {
    fn send(&mut self, _value: Queue<Frame>) -> Result<(), Queue<Frame>> {
        Ok(())
    }
}

/// Sink that appends frames into a local queue for retransmission.
struct QueueSink<'a>(&'a mut Queue<Frame>);

impl crate::socket::channel::UnboundedSender<Queue<Frame>> for QueueSink<'_> {
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
    Tx: crate::socket::channel::UnboundedSender<
        crate::intrusive_queue::Entry<
            crate::packet::datagram::decoder::Packet<crate::socket::pool::descriptor::Filled>,
        >,
    >,
{
    use crate::{
        socket::channel::{
            FlattenSegments, InspectErr, ReceiverExt as _, RouterAdapter, SocketReceiver,
        },
        stream3::endpoint::worker::ChannelRouter,
    };

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
    recv_cache: std::rc::Rc<std::cell::RefCell<crate::stream3::endpoint::recv::Cache>>,
    path_secret_map: crate::path::secret::Map,
    acceptor_registry: crate::acceptor::Registry<crate::stream3::Stream>,
    frame_tx: crate::stream3::frame::SubmissionSender,
    ack_sender: AckTx,
    queue_dispatcher: crate::stream3::endpoint::msg::queue::Dispatcher,
    counters: crate::stream3::endpoint::counters::Dispatch,
    clock: Clk,
    budget: usize,
) where
    PacketRx: Receiver<
        crate::intrusive_queue::Entry<
            crate::packet::datagram::decoder::Packet<crate::socket::pool::descriptor::Filled>,
        >,
    >,
    AckTx: crate::socket::channel::UnboundedSender<crate::stream3::endpoint::msg::Sender>,
    Clk: s2n_quic_core::time::Clock + crate::clock::precision::Clock,
{
    use crate::{
        socket::channel::{InspectErr, Map, ReceiverExt as _},
        stream3::endpoint::dispatch,
    };

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

fn on_packet_dispatch_error(
    counters: &crate::stream3::endpoint::counters::Dispatch,
    err: crate::stream3::endpoint::dispatch::Error,
) {
    use crate::stream3::endpoint::dispatch;

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

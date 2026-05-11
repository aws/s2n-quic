// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    datagram::batch::Priority,
    intrusive_queue::{Entry, Queue},
    path::secret::map::Entry as PathSecretEntry,
    socket::channel::{ByteCost, Receiver, Sender},
    stream3::frame::Frame,
};
use core::{
    future::poll_fn,
    task::{self, Poll},
};
use std::sync::Arc;

/// Default per-poll budget for [`socket_recv_task`]: process up to this many segments before
/// yielding to the executor. Tune via the `budget` parameter if workloads differ.
pub const DEFAULT_RECV_BUDGET: usize = 32;

/// Default per-poll budget for [`packet_dispatch_task`]: process up to this many packets before
/// yielding to the executor. Tune via the `budget` parameter if workloads differ.
pub const DEFAULT_DISPATCH_BUDGET: usize = 32;

/// Routing key accessor for stream3 send-side load-balancing tasks.
pub trait PathSecretMapEntry {
    fn path_secret_entry(&self) -> &Arc<PathSecretEntry>;
}

impl<T> PathSecretMapEntry for crate::intrusive_queue::Entry<T>
where
    T: PathSecretMapEntry,
{
    #[inline]
    fn path_secret_entry(&self) -> &Arc<PathSecretEntry> {
        (**self).path_secret_entry()
    }
}

impl PathSecretMapEntry for crate::stream3::frame::Frame {
    #[inline]
    fn path_secret_entry(&self) -> &Arc<PathSecretEntry> {
        &self.path_secret_entry
    }
}

/// Conservative packet-level overhead estimate for stream3 frame batches.
///
/// Uses the same upper-bound constant as datagram partials so batching leaves room for packet
/// fields that are added later by workers (credentials, packet number, routing, tag, etc).
const MAX_FRAME_BATCH_PACKET_OVERHEAD: u64 =
    crate::packet::datagram::partial::MAX_FLOW_DATA_HEADER_OVERHEAD as u64;
const BATCH_FRAMES_POLL_BUDGET: usize = 10;

/// A queue of frames grouped for a single path-secret entry.
///
/// This wrapper keeps the queue byte-cost estimate and path-secret entry so it can be
/// routed through the priority merger and `pick_two`.
///
/// Because individual frames are routed into per-priority unsync lanes *before*
/// [`BatchFramesByPathSecret`] coalesces them, all frames in a `FrameBatch` that comes out
/// of a given lane share the same priority class.
pub struct FrameBatch {
    queue: Queue<Frame>,
    path_secret_entry: Arc<PathSecretEntry>,
    byte_cost: u64,
}

impl FrameBatch {
    #[inline]
    fn new(first: Entry<Frame>) -> Self {
        let path_secret_entry = first.path_secret_entry.clone();
        let byte_cost = MAX_FRAME_BATCH_PACKET_OVERHEAD.saturating_add(first.byte_cost());
        let mut queue = Queue::new();
        queue.push_back(first);

        Self {
            queue,
            path_secret_entry,
            byte_cost,
        }
    }

    #[inline]
    fn push_with_cost(&mut self, frame: Entry<Frame>, frame_cost: u64) {
        self.byte_cost = self.byte_cost.saturating_add(frame_cost);
        self.queue.push_back(frame);
    }

    /// Returns the number of frames currently buffered in this batch.
    #[inline]
    pub fn len(&self) -> usize {
        self.queue.len()
    }

    /// Returns true when this batch contains no frames.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    /// Borrows the underlying intrusive queue of frames.
    #[inline]
    pub fn queue(&self) -> &Queue<Frame> {
        &self.queue
    }

    /// Consumes the batch and returns the underlying frame queue.
    #[inline]
    pub fn into_queue(self) -> Queue<Frame> {
        self.queue
    }
}

impl From<FrameBatch> for Queue<Frame> {
    #[inline]
    fn from(value: FrameBatch) -> Self {
        value.into_queue()
    }
}

impl ByteCost for FrameBatch {
    #[inline]
    fn byte_cost(&self) -> u64 {
        self.byte_cost
    }
}

impl PathSecretMapEntry for FrameBatch {
    #[inline]
    fn path_secret_entry(&self) -> &Arc<PathSecretEntry> {
        &self.path_secret_entry
    }
}

/// Receiver combinator that batches consecutive frame entries by path-secret entry and byte budget.
///
/// Batches target roughly one datagram (`path_secret_entry.max_datagram_size()`) while accounting
/// for frame metadata and conservative packet overhead. A batch always contains at least one frame.
pub struct BatchFramesByPathSecret<R> {
    inner: R,
    buffered: Option<Entry<Frame>>,
}

impl<R> BatchFramesByPathSecret<R>
where
    R: Receiver<Entry<Frame>>,
{
    #[inline]
    pub fn new(inner: R) -> Self {
        Self {
            inner,
            buffered: None,
        }
    }

    #[inline]
    fn take_first(&mut self, cx: &mut task::Context<'_>) -> Poll<Option<Entry<Frame>>> {
        if let Some(frame) = self.buffered.take() {
            return Poll::Ready(Some(frame));
        }

        self.inner.poll_recv(cx)
    }
}

impl<R> Receiver<FrameBatch> for BatchFramesByPathSecret<R>
where
    R: Receiver<Entry<Frame>>,
{
    fn poll_recv(&mut self, cx: &mut task::Context<'_>) -> Poll<Option<FrameBatch>> {
        let Some(first) = (match self.take_first(cx) {
            Poll::Ready(frame) => frame,
            Poll::Pending => return Poll::Pending,
        }) else {
            return Poll::Ready(None);
        };

        let target_bytes = first.path_secret_entry.max_datagram_size() as u64;
        let mut batch = FrameBatch::new(first);

        // Keep poll work bounded and return the current batch so the executor can make progress.
        for _ in 0..BATCH_FRAMES_POLL_BUDGET {
            if batch.byte_cost() >= target_bytes {
                break;
            }

            match self.inner.poll_recv(cx) {
                Poll::Ready(Some(frame_entry)) => {
                    if !Arc::ptr_eq(batch.path_secret_entry(), frame_entry.path_secret_entry()) {
                        self.buffered = Some(frame_entry);
                        break;
                    }

                    let frame_cost = frame_entry.byte_cost();
                    let next_cost = batch.byte_cost().saturating_add(frame_cost);
                    if next_cost > target_bytes {
                        self.buffered = Some(frame_entry);
                        break;
                    }

                    batch.push_with_cost(frame_entry, frame_cost);
                }
                Poll::Ready(None) | Poll::Pending => break,
            }
        }

        Poll::Ready(Some(batch))
    }

    #[inline]
    fn on_consumed(&mut self, bytes: u64) {
        self.inner.on_consumed(bytes);
    }
}

/// Routes items to socket senders by using pick-two path scheduling from the path secret map
/// entry associated with each item.
pub async fn pick_two<T, R, S, Rand>(mut rx: R, mut senders: Vec<S>, random: Rand)
where
    T: ByteCost + PathSecretMapEntry,
    R: Receiver<T>,
    S: Sender<T>,
    Rand: Fn(usize) -> usize,
{
    loop {
        let Some(entry) = rx.recv().await else {
            break;
        };

        let bytes = entry.byte_cost();
        let mut slot = core::mem::MaybeUninit::new(entry);

        let sent = poll_fn(|cx| try_send_pick_two(cx, &mut slot, &mut senders, &random)).await;

        if !sent {
            // SAFETY: `slot` is initialized above with `MaybeUninit::new(entry)` and only
            // consumed by successful send.
            unsafe { slot.assume_init_drop() };
            break;
        }

        rx.on_consumed(bytes);
    }
}

fn try_send_pick_two<T, S, Rand>(
    cx: &mut task::Context<'_>,
    slot: &mut core::mem::MaybeUninit<T>,
    senders: &mut Vec<S>,
    random: &Rand,
) -> Poll<bool>
where
    T: PathSecretMapEntry,
    S: Sender<T>,
    Rand: Fn(usize) -> usize,
{
    if senders.is_empty() {
        return Poll::Ready(false);
    }

    let chosen_idx = {
        // SAFETY: `slot` is initialized with `MaybeUninit::new(entry)` and remains
        // initialized until it is consumed by a successful `poll_send`.
        let value = unsafe { &*slot.as_ptr() };
        let picked = value
            .path_secret_entry()
            .pick_sender_by_next_transmission(random);
        debug_assert!(
            picked < senders.len(),
            "picked sender index out of bounds: picked={} senders={}",
            picked,
            senders.len()
        );
        if picked >= senders.len() {
            return Poll::Ready(false);
        }
        picked
    };

    match senders[chosen_idx].poll_send(cx, slot) {
        Poll::Ready(Ok(())) => Poll::Ready(true),
        Poll::Ready(Err(())) => Poll::Ready(false),
        Poll::Pending => {
            let len = senders.len();
            for offset in 1..len {
                let idx = (chosen_idx + offset) % len;
                match senders[idx].poll_send(cx, slot) {
                    Poll::Ready(Ok(())) => return Poll::Ready(true),
                    Poll::Ready(Err(())) => return Poll::Ready(false),
                    Poll::Pending => {}
                }
            }
            Poll::Pending
        }
    }
}

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
) where
    S: Sender<FrameBatch> + 'static,
    Rand: Fn(usize) -> usize + 'static,
    Clk: crate::clock::precision::Clock + 'static,
{
    use crate::socket::channel::{intrusive_queue, Paced, Priority as PriorityRx};

    // Create one unbounded unsync channel per priority level.
    // Task 1 sends whole `Queue<Frame>` lists via `ListSender`; Task 2 pops individual
    // frames via the plain `Receiver`.  Both tasks run on the same worker, so Rc-based
    // (!Send) channels are correct here.
    let mut priority_list_txs = Vec::with_capacity(Priority::LEVELS);
    let mut priority_frame_rxs = Vec::with_capacity(Priority::LEVELS);
    for _ in 0..Priority::LEVELS {
        let (tx, rx) = intrusive_queue::unsync::new::<Frame>();
        priority_list_txs.push(tx.into_list_sender());
        priority_frame_rxs.push(rx);
    }

    // Task 1: fixed-cost priority routing.
    //
    // A persistent `staging` PriorityStorage (pre-allocated once) avoids heap allocation on
    // the hot path.  Each poll calls `poll_swap` once to pointer-swap the next ready shard's
    // Box into `staging`, then appends each non-empty priority queue to the matching
    // per-priority ListSender.  After processing one shard the task yields to the executor
    // (one shard per poll, matching the behaviour of `drain()`).
    spawner.spawn({
        let mut staging = crate::stream3::frame::PriorityStorage::default();
        poll_fn(move |cx| {
            match frame_rx.poll_swap(cx, &mut staging) {
                task::Poll::Ready(None) => task::Poll::Ready(()),
                task::Poll::Pending => task::Poll::Pending,
                task::Poll::Ready(Some(())) => {
                    for (i, queue) in staging.queues.iter_mut().enumerate() {
                        if !queue.is_empty() {
                            let _ = crate::socket::channel::UnboundedSender::send(
                                &mut priority_list_txs[i],
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
    // apply pacing, then route to send sockets via pick_two.
    //
    // Pipeline: [per-priority-rx[i] → BatchFramesByPathSecret] → Priority → Paced → pick_two
    spawner.spawn(async move {
        let batched_lanes: Vec<BatchFramesByPathSecret<_>> = priority_frame_rxs
            .into_iter()
            .map(BatchFramesByPathSecret::new)
            .collect();
        let rx = PriorityRx::new(batched_lanes);
        let rx = Paced::new(rx, clock, overall_send_rate);
        pick_two(rx, socket_senders, random).await;
    });
}

/// Per-socket send worker: receives frame batches, assembles packets, sends via socket.
///
/// `batch_rx` and `ack_rx` are generic so callers can wrap them with pacing, metrics, or
/// local unsync receivers when the sender is on the same worker.
///
/// Maintains a per-peer [`send::Context`] cache. For each incoming [`FrameBatch`] the frames are
/// pushed onto the matching context's pending queue, [`assemble`] is called to encrypt and pack
/// them into GSO segments, and the segments are sent through `socket`. Concurrently, incoming
/// [`msg::Sender::Ack`] messages are decoded and fed into [`ack::process_ack`] to update CCA and
/// loss-recovery state.
///
/// [`send::Context`]: crate::stream3::endpoint::send::Context
/// [`assemble`]: crate::stream3::endpoint::assemble::assemble
/// [`ack::process_ack`]: crate::stream3::endpoint::ack::process_ack
///
/// # TODO: missing stream2 pipeline stages
///
/// The following stages present in stream2's send pipeline are not yet implemented:
///
/// - **Worker-shared socket contexts** (`Rc<SocketPathContexts>`): stream2 creates a
///   per-socket `SocketPathContexts` (an `Rc`) that is registered in a worker-level
///   `sender_contexts: Rc<RefCell<HashMap<usize, Rc<SocketPathContexts>>>>`. This lets the
///   ACK processing task (phase 2) look up the context for any socket on the same worker.
///
/// - **PathResolver**: resolves each `FrameBatch` to a per-peer send context by credentials.
///   Emits errors (unknown peer, missing path secret) to a dedicated error channel so they do
///   not block the hot path.
///
/// - **Encoder** (`channel::Encoder`): encrypts frame queues into wire-format datagrams:
///   fills in credentials, GSO-aware packet boundaries, routing info (`source_sender_id`,
///   `source_control_port`), and AEAD authentication tag. Produces `PartialDatagram` items.
///
/// - **PacketRegistrar** (`channel::PacketRegistrar`): registers each encrypted packet in the
///   inflight map (packet-number → context), marking it eligible for loss recovery and PTO.
///   Stamps the transmission timestamp used for RTT estimation.
///
/// - **Per-socket pacer** (`Paced`): enforces a per-socket send-rate cap after
///   `PacketRegistrar` and before the actual socket write, preventing burst sending.
///
/// - **Acked/lost packet channels**: stream2 has unsync channels (`acked_tx`, `lost_tx`) from
///   the ACK processing task back to this task, driving CCA (`on_ack`, `on_loss`) updates,
///   retransmission batching, and completion notifications to waiters.
///
/// - **PTO wheel injection**: when CCA schedules a probe timeout, the context's PTO deadline
///   is registered with a per-worker `Wheel`; when the wheel fires, a probe batch is generated
///   and pumped back into the frame submission channel.
///
/// - **Metrics**: `tx` packet counter, `tx:bytes` byte counter, per-socket queue depth gauge.
pub async fn socket_send_task<Socket, BatchRx, AckRx>(
    _socket: Socket,
    mut batch_rx: BatchRx,
    mut ack_rx: AckRx,
    _sender_idx: usize,
    _source_control_port: u16,
    _gso: s2n_quic_platform::features::Gso,
    _pool: crate::socket::pool::Pool,
) where
    Socket: crate::socket::send::Socket,
    BatchRx: Receiver<FrameBatch>,
    AckRx: Receiver<crate::intrusive_queue::Entry<crate::stream3::endpoint::msg::Sender>>,
{
    // TODO: implement the full send pipeline (see doc comment above for all stages).
    // For now, drain and discard all incoming batches and ACKs so that the endpoint can run
    // without panicking, and so that channels don't back up. This is intentionally a no-op
    // stub until the send path is implemented.
    use core::future;
    use core::task::Poll;

    // Drain both channels with a per-poll budget so the stub does not starve other tasks.
    future::poll_fn(move |cx| {
        let mut done = 0;
        for _ in 0..DEFAULT_DISPATCH_BUDGET {
            match batch_rx.poll_recv(cx) {
                Poll::Ready(Some(_)) => done += 1,
                Poll::Ready(None) => return Poll::Ready(()),
                Poll::Pending => break,
            }
        }
        for _ in 0..DEFAULT_DISPATCH_BUDGET {
            match ack_rx.poll_recv(cx) {
                Poll::Ready(Some(_)) => done += 1,
                Poll::Ready(None) => return Poll::Ready(()),
                Poll::Pending => break,
            }
        }
        // If we consumed items this round, yield so other tasks can run.
        if done > 0 {
            cx.waker().wake_by_ref();
        }
        Poll::Pending
    })
    .await
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
    use crate::socket::channel::{
        FlattenSegments, InspectErr, ReceiverExt as _, RouterAdapter, SocketReceiver,
    };
    use crate::stream3::endpoint::worker::ChannelRouter;

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
    RouterAdapter::new(rx, router).drain_budgeted(Some(budget)).await;
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
/// - **Response channel** (`response_tx`): stream2 has a dedicated unsync channel for ACKs and
///   flow-control responses generated by `process_datagram`. These are batched by
///   `RetransmissionBatcher` and pumped into the timing wheel. Currently in stream3, response
///   frames re-enter `frame_tx` directly; a separate response channel with its own batcher would
///   allow finer-grained scheduling.
///
/// - **Error classification**: stream2 logs distinct error types with structured fields —
///   `PeerStateLookup` (warn), `Decryption` (debug), `Duplicate` (trace),
///   `MissingSenderId` (warn). The current impl silently drops all errors; each variant should
///   be logged and counted separately.
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
    use crate::socket::channel::{InspectErr, Map, ReceiverExt as _};
    use crate::stream3::endpoint::dispatch;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        byte_vec::ByteVec,
        path::secret::map::Entry as PathSecretEntry,
        stream3::frame::{Header, TransmissionStatus, DEFAULT_TTL},
    };
    use bytes::Bytes;
    use core::{future::Future, mem::MaybeUninit, task::Poll};
    use s2n_quic_core::varint::VarInt;
    use std::{
        collections::VecDeque,
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        },
    };

    struct TestItem {
        path_secret_entry: Arc<PathSecretEntry>,
        byte_cost: u64,
        drop_counter: Arc<AtomicUsize>,
    }

    impl Drop for TestItem {
        fn drop(&mut self) {
            self.drop_counter.fetch_add(1, Ordering::Relaxed);
        }
    }

    impl ByteCost for TestItem {
        fn byte_cost(&self) -> u64 {
            self.byte_cost
        }
    }

    impl PathSecretMapEntry for TestItem {
        fn path_secret_entry(&self) -> &Arc<PathSecretEntry> {
            &self.path_secret_entry
        }
    }

    #[derive(Clone, Copy)]
    enum SenderBehavior {
        Pending,
        ReadyOk,
        ReadyErr,
    }

    struct TestSender {
        behavior: SenderBehavior,
        calls: usize,
    }

    impl Sender<TestItem> for TestSender {
        fn poll_send(
            &mut self,
            _cx: &mut task::Context<'_>,
            value: &mut MaybeUninit<TestItem>,
        ) -> Poll<Result<(), ()>> {
            self.calls += 1;

            match self.behavior {
                SenderBehavior::Pending => Poll::Pending,
                SenderBehavior::ReadyOk => {
                    // SAFETY: successful send consumes the value.
                    unsafe { value.assume_init_drop() };
                    Poll::Ready(Ok(()))
                }
                SenderBehavior::ReadyErr => Poll::Ready(Err(())),
            }
        }
    }

    struct TestReceiver {
        values: VecDeque<TestItem>,
        consumed: u64,
    }

    impl Receiver<TestItem> for TestReceiver {
        fn poll_recv(&mut self, _cx: &mut task::Context<'_>) -> Poll<Option<TestItem>> {
            Poll::Ready(self.values.pop_front())
        }

        fn on_consumed(&mut self, bytes: u64) {
            self.consumed += bytes;
        }
    }

    struct TestFrameReceiver {
        values: VecDeque<Entry<Frame>>,
        consumed: u64,
    }

    impl Receiver<Entry<Frame>> for TestFrameReceiver {
        fn poll_recv(&mut self, _cx: &mut task::Context<'_>) -> Poll<Option<Entry<Frame>>> {
            Poll::Ready(self.values.pop_front())
        }

        fn on_consumed(&mut self, bytes: u64) {
            self.consumed += bytes;
        }
    }

    fn test_path_secret_entry() -> Arc<PathSecretEntry> {
        let peer = "127.0.0.1:4433"
            .parse()
            .expect("failed to parse hardcoded loopback address 127.0.0.1:4433");
        PathSecretEntry::fake(peer, None)
    }

    fn new_test_item(
        path_secret_entry: Arc<PathSecretEntry>,
        drop_counter: Arc<AtomicUsize>,
    ) -> TestItem {
        TestItem {
            path_secret_entry,
            byte_cost: 123,
            drop_counter,
        }
    }

    fn new_test_frame(path_secret_entry: Arc<PathSecretEntry>, payload_len: usize) -> Entry<Frame> {
        let mut payload = ByteVec::new();
        if payload_len > 0 {
            payload.push_back(Bytes::from(vec![0u8; payload_len]));
        }

        Entry::new(Frame {
            header: Header::Control {
                dest_sender_id: VarInt::from_u8(1),
            },
            source_sender_id: VarInt::MAX,
            payload,
            path_secret_entry,
            completion: None,
            status: TransmissionStatus::Pending,
            ttl: DEFAULT_TTL,
            transmission_time: None,
        })
    }

    fn with_noop_context<R>(f: impl FnOnce(&mut task::Context<'_>) -> R) -> R {
        let waker = s2n_quic_core::task::waker::noop();
        let mut cx = task::Context::from_waker(&waker);
        f(&mut cx)
    }

    #[test]
    fn selected_sender_is_polled_before_alternates() {
        let mut slot = MaybeUninit::new(new_test_item(
            test_path_secret_entry(),
            Arc::new(AtomicUsize::new(0)),
        ));
        let mut senders = vec![
            TestSender {
                behavior: SenderBehavior::ReadyOk,
                calls: 0,
            },
            TestSender {
                behavior: SenderBehavior::ReadyOk,
                calls: 0,
            },
        ];
        let result = with_noop_context(|cx| try_send_pick_two(cx, &mut slot, &mut senders, &|_| 0));
        assert_eq!(result, Poll::Ready(true));
        assert_eq!(senders[0].calls, 1);
        assert_eq!(senders[1].calls, 0);
    }

    #[test]
    fn falls_back_to_alternate_sender_when_selected_sender_is_pending() {
        let mut slot = MaybeUninit::new(new_test_item(
            test_path_secret_entry(),
            Arc::new(AtomicUsize::new(0)),
        ));
        let mut senders = vec![
            TestSender {
                behavior: SenderBehavior::Pending,
                calls: 0,
            },
            TestSender {
                behavior: SenderBehavior::ReadyOk,
                calls: 0,
            },
        ];
        let result = with_noop_context(|cx| try_send_pick_two(cx, &mut slot, &mut senders, &|_| 0));
        assert_eq!(result, Poll::Ready(true));
        assert_eq!(senders[0].calls, 1);
        assert_eq!(senders[1].calls, 1);
    }

    #[test]
    fn shuts_down_on_sender_error() {
        let mut slot = MaybeUninit::new(new_test_item(
            test_path_secret_entry(),
            Arc::new(AtomicUsize::new(0)),
        ));
        let mut senders = vec![
            TestSender {
                behavior: SenderBehavior::ReadyErr,
                calls: 0,
            },
            TestSender {
                behavior: SenderBehavior::ReadyOk,
                calls: 0,
            },
        ];
        let result = with_noop_context(|cx| try_send_pick_two(cx, &mut slot, &mut senders, &|_| 0));
        assert_eq!(result, Poll::Ready(false));
        assert_eq!(senders[0].calls, 1);
        assert_eq!(senders[1].calls, 0);

        // SAFETY: `Err` keeps the value in slot and caller must drop it.
        unsafe { slot.assume_init_drop() };
    }

    #[test]
    fn pick_two_drops_unsent_entry_on_shutdown() {
        let drop_counter = Arc::new(AtomicUsize::new(0));
        let rx = TestReceiver {
            values: [new_test_item(test_path_secret_entry(), drop_counter.clone())].into(),
            consumed: 0,
        };
        let senders = vec![TestSender {
            behavior: SenderBehavior::ReadyErr,
            calls: 0,
        }];
        let mut fut = core::pin::pin!(pick_two(rx, senders, |_| 0));
        let result = with_noop_context(|cx| fut.as_mut().poll(cx));
        assert_eq!(result, Poll::Ready(()));
        assert_eq!(drop_counter.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn batch_frames_groups_by_same_path_secret() {
        let path_a = test_path_secret_entry();
        let path_b = test_path_secret_entry();
        path_a.update_max_datagram_size(4_096);
        path_b.update_max_datagram_size(4_096);

        let rx = TestFrameReceiver {
            values: VecDeque::from([
                new_test_frame(path_a.clone(), 16),
                new_test_frame(path_a.clone(), 16),
                new_test_frame(path_b.clone(), 16),
            ]),
            consumed: 0,
        };
        let mut batcher = BatchFramesByPathSecret::new(rx);

        let first = with_noop_context(|cx| batcher.poll_recv(cx));
        let Poll::Ready(Some(first)) = first else {
            panic!("expected first batch");
        };
        assert_eq!(first.len(), 2);
        assert!(Arc::ptr_eq(first.path_secret_entry(), &path_a));

        let second = with_noop_context(|cx| batcher.poll_recv(cx));
        let Poll::Ready(Some(second)) = second else {
            panic!("expected second batch");
        };
        assert_eq!(second.len(), 1);
        assert!(Arc::ptr_eq(second.path_secret_entry(), &path_b));
    }

    #[test]
    fn batch_frames_enforces_datagram_byte_budget() {
        let path = test_path_secret_entry();
        path.update_max_datagram_size(220);

        let rx = TestFrameReceiver {
            values: VecDeque::from([
                new_test_frame(path.clone(), 70),
                new_test_frame(path.clone(), 70),
                new_test_frame(path.clone(), 70),
            ]),
            consumed: 0,
        };
        let mut batcher = BatchFramesByPathSecret::new(rx);

        let first = with_noop_context(|cx| batcher.poll_recv(cx));
        let Poll::Ready(Some(first)) = first else {
            panic!("expected first batch");
        };
        assert_eq!(first.len(), 1);
        assert!(first.byte_cost() <= 220);
        let frame_cost = first
            .queue()
            .peek_front()
            .expect("batch must contain the first frame")
            .byte_cost();
        assert!(first.byte_cost().saturating_add(frame_cost) > 220);

        let second = with_noop_context(|cx| batcher.poll_recv(cx));
        let Poll::Ready(Some(second)) = second else {
            panic!("expected second batch");
        };
        assert_eq!(second.len(), 1);

        let third = with_noop_context(|cx| batcher.poll_recv(cx));
        let Poll::Ready(Some(third)) = third else {
            panic!("expected third batch");
        };
        assert_eq!(third.len(), 1);
    }

    #[test]
    fn batch_frames_forwards_on_consumed() {
        let path = test_path_secret_entry();
        let rx = TestFrameReceiver {
            values: VecDeque::from([new_test_frame(path, 0)]),
            consumed: 0,
        };
        let mut batcher = BatchFramesByPathSecret::new(rx);

        batcher.on_consumed(321);
        assert_eq!(batcher.inner.consumed, 321);
    }
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Stream3 Endpoint: shared infrastructure for the process.

pub(crate) mod ack;
pub(crate) mod assemble;
pub(crate) mod counters;
pub(crate) mod decode;
pub(crate) mod dispatch;
pub(crate) mod inflight;
pub(crate) mod msg;
pub(crate) mod recv;
pub(crate) mod reset_error;
pub(crate) mod routing;
pub(crate) mod send;
pub(crate) mod socket;
pub(crate) mod tasks;
pub(crate) mod worker;

use crate::{
    acceptor,
    stream3::{frame::SubmissionSender, Stream},
};
use std::sync::atomic::AtomicU64;

pub struct Endpoint {
    /// Frame submission channel (writers submit Queue<Frame> here)
    pub frame_tx: SubmissionSender,
    /// Path secret map (shared with PSK providers)
    pub path_secret_map: crate::path::secret::Map,
    /// Queue allocator for flow queues
    pub queue_allocator: msg::queue::Allocator,
    /// Acceptor registry for server-side stream dispatch
    pub acceptor_registry: acceptor::Registry<Stream>,
    /// Endpoint-wide stream ID counter
    pub next_stream_id: AtomicU64,
    /// The port that recv sockets are bound to
    pub data_port: u16,
}

// ── Pipeline Setup ────────────────────────────────────────────────────────

/// Configuration for the stream3 pipeline.
pub struct EndpointConfig<S, C> {
    /// Worker pool spawner.
    pub spawner: S,
    /// Buffer pool for outbound (send) packets.
    pub send_pool: crate::socket::pool::Pool,
    /// Buffer pool for inbound (recv) packets.
    pub recv_pool: crate::socket::pool::Pool,
    /// Path-secret map shared with PSK providers.
    pub path_secret_map: crate::path::secret::Map,
    /// GSO capability probed for the local host.
    pub gso: s2n_quic_platform::features::Gso,
    /// Server-side acceptor registry.
    pub acceptor_registry: acceptor::Registry<Stream>,
    /// Peer idle timeout — controls when [`recv::Cache`] entries expire.
    ///
    /// [`recv::Cache`]: recv::Cache
    pub idle_timeout: core::time::Duration,
    /// Wall-clock source used for RTT estimation and timeouts.
    pub clock: C,
    /// Overall bandwidth cap applied by the frame-dispatch pacing stage.
    ///
    /// The [`Paced`] combinator in the dispatch pipeline enforces this rate across all
    /// send sockets combined. Set to a very high value (e.g. `Rate::new(100.0)` for
    /// 100 Gbps) to effectively disable pacing when the network is not a bottleneck.
    ///
    /// [`Paced`]: crate::socket::channel::Paced
    pub overall_send_rate: crate::socket::rate::Rate,
}

// ── Worker parts ──────────────────────────────────────────────────────────

/// All the ingredients needed to spawn the frame-dispatch task on a worker.
struct FrameDispatchParts<G, Clk> {
    frame_rx: crate::stream3::frame::SubmissionReceiver,
    /// Senders for each send-socket's batch channel.
    batch_txs: Vec<crate::socket::channel::cell::sync::Sender<tasks::FrameBatch>>,
    /// Random generator for pick-two routing.
    rand: G,
    /// Clock used by the pacing stage.
    clock: Clk,
    /// Overall bandwidth cap for the pacing stage.
    overall_send_rate: crate::socket::rate::Rate,
}

/// All the ingredients needed to spawn a send-socket task on a worker.
struct SendTaskParts<Socket> {
    socket: Socket,
    batch_rx: crate::socket::channel::cell::sync::Receiver<tasks::FrameBatch>,
    ack_rx: crate::socket::channel::intrusive_queue::sync::Receiver<msg::Sender>,
    sender_idx: usize,
    source_control_port: u16,
    gso: s2n_quic_platform::features::Gso,
    pool: crate::socket::pool::Pool,
}

/// All the ingredients needed to spawn a recv-socket + dispatch task pair on a worker.
struct RecvTaskParts<Socket, Clk, AckSnd> {
    // ── recv task ──────────────────────────────────────────────────────
    socket: Socket,
    recv_pool: crate::socket::pool::Pool,
    packet_tx: crate::socket::channel::intrusive_queue::sync::Sender<
        crate::packet::datagram::decoder::Packet<crate::socket::pool::descriptor::Filled>,
    >,
    decode_error_counter: crate::counter::Counter,
    // ── dispatch task ─────────────────────────────────────────────────
    packet_rx: crate::socket::channel::intrusive_queue::sync::Receiver<
        crate::packet::datagram::decoder::Packet<crate::socket::pool::descriptor::Filled>,
    >,
    path_secret_map: crate::path::secret::Map,
    acceptor_registry: acceptor::Registry<Stream>,
    frame_tx: SubmissionSender,
    ack_sender: AckSnd,
    queue_dispatcher: msg::queue::Dispatcher,
    counters: counters::Dispatch,
    clock: Clk,
}

// ── Worker ────────────────────────────────────────────────────────────────

/// Holds all the parts needed to spawn tasks on a single worker thread.
///
/// After building a `Worker` for each thread, call [`Worker::spawn`] on each to hand off all
/// its tasks to the spawner. This design makes it easy to reassign sockets or tasks across
/// workers without restructuring the spawn logic.
///
/// `idle_timeout` is stored here (not in `RecvTaskParts`) because it belongs to the worker as
/// a whole: the single `recv::Cache` is created once inside `spawn_local` as an
/// `Rc<RefCell<recv::Cache>>` and shared across all tasks that run on this worker thread.
struct Worker<SendSocket, RecvSocket, Clk, G, AckSnd> {
    /// This worker's index in the spawner.
    id: usize,
    /// Peer idle timeout — controls when `recv::Cache` entries expire.
    idle_timeout: core::time::Duration,
    /// Frame-dispatch task, assigned to exactly one worker (typically worker 0).
    frame_dispatch: Option<FrameDispatchParts<G, Clk>>,
    /// Send socket tasks assigned to this worker.
    send_tasks: Vec<SendTaskParts<SendSocket>>,
    /// Recv + dispatch task pairs assigned to this worker.
    recv_tasks: Vec<RecvTaskParts<RecvSocket, Clk, AckSnd>>,
}

impl<SendSocket, RecvSocket, Clk, G, AckSnd> Worker<SendSocket, RecvSocket, Clk, G, AckSnd>
where
    SendSocket: crate::socket::send::Socket + Send + 'static,
    RecvSocket: crate::socket::recv::Socket + Send + 'static,
    Clk: s2n_quic_core::time::Clock
        + crate::clock::precision::Clock
        + Send
        + 'static,
    G: crate::random::Generator,
    AckSnd: crate::socket::channel::UnboundedSender<msg::Sender> + Send + 'static,
{
    fn new(id: usize, idle_timeout: core::time::Duration) -> Self {
        Self {
            id,
            idle_timeout,
            frame_dispatch: None,
            send_tasks: Vec::new(),
            recv_tasks: Vec::new(),
        }
    }

    /// Spawns all tasks for this worker via `spawner.spawn_local`.
    ///
    /// The random generator (`G`) is captured as `Send` in the outer closure and then wrapped
    /// in a [`std::cell::RefCell`] inside `spawn_local`, where it is entirely worker-local.
    /// No `Mutex` is needed.
    ///
    /// A single [`recv::Cache`] is created once per worker (as `Rc<RefCell<recv::Cache>>`) and
    /// shared across all dispatch tasks. This matches stream2's `shared_sender_cache` pattern,
    /// where the cache is accessed by multiple tasks on the same worker thread via `Rc`.
    fn spawn<S: crate::stream2::Spawner>(self, spawner: &S) {
        use crate::stream2::spawner::LocalSpawner as _;

        let Self {
            id,
            idle_timeout,
            frame_dispatch,
            send_tasks,
            recv_tasks,
        } = self;

        spawner.spawn_local(id, move |mut local| {
            if let Some(fd) = frame_dispatch {
                // `fd.rand` is `G: Send`, captured in this outer `Send` closure.
                // Wrap it in `RefCell` here (inside spawn_local) so it is entirely
                // worker-local — no cross-thread synchronisation needed.
                let random = std::cell::RefCell::new(fd.rand);
                let random_fn = move |n: usize| {
                    let mut bytes = [0u8; 8];
                    random.borrow_mut().public_random_fill(&mut bytes);
                    let raw = u64::from_le_bytes(bytes) as usize;
                    // Sender counts are always powers of two, so we can use a cheap
                    // bitwise mask rather than a more expensive modulo operation.
                    debug_assert!(n.is_power_of_two(), "sender count must be a power of two");
                    raw & (n - 1)
                };
                tasks::frame_dispatch(
                    &mut local,
                    fd.frame_rx,
                    fd.batch_txs,
                    random_fn,
                    fd.clock,
                    fd.overall_send_rate,
                );
            }

            for st in send_tasks {
                local.spawn(tasks::socket_send_task(
                    st.socket,
                    st.batch_rx,
                    st.ack_rx,
                    st.sender_idx,
                    st.source_control_port,
                    st.gso,
                    st.pool,
                ));
            }

            // One recv::Cache per worker, shared across all dispatch tasks on this worker
            // (matches stream2's Rc<RefCell<SenderStateCache>> pattern).
            let recv_cache = std::rc::Rc::new(std::cell::RefCell::new(
                crate::stream3::endpoint::recv::Cache::new(idle_timeout, id),
            ));

            for rt in recv_tasks {
                local.spawn(tasks::socket_recv_task(
                    rt.socket,
                    rt.recv_pool,
                    rt.packet_tx,
                    rt.decode_error_counter,
                    tasks::DEFAULT_RECV_BUDGET,
                ));
                local.spawn(tasks::packet_dispatch_task(
                    rt.packet_rx,
                    recv_cache.clone(),
                    rt.path_secret_map,
                    rt.acceptor_registry,
                    rt.frame_tx,
                    rt.ack_sender,
                    rt.queue_dispatcher,
                    rt.counters,
                    rt.clock,
                    tasks::DEFAULT_DISPATCH_BUDGET,
                ));
            }
        });
    }
}

// ── setup_endpoint ────────────────────────────────────────────────────────

/// Assembles the stream3 pipeline from pre-opened sockets and spawns worker tasks.
///
/// This is the top-level composition function. It creates all inter-task channels, builds a
/// [`Worker`] for each spawner thread, and calls [`Worker::spawn`]. No pipeline logic lives
/// here — every stage is implemented in the task functions in [`tasks`].
///
/// # Worker distribution
///
/// * Worker `0` runs the frame-dispatch task (routes batches to send sockets).
/// * Send workers handle per-socket assembly and transmission (workers 1..=num_send).
/// * Remaining workers pair a socket-recv task with a packet-dispatch task.
///
/// When the worker count exceeds the number of sockets, extra workers are idle. When the socket
/// count exceeds workers, multiple sockets share a worker.
pub fn setup_endpoint<SendSocket, RecvSocket, G, S, C>(
    config: EndpointConfig<S, C>,
    send_sockets: Vec<SendSocket>,
    recv_sockets: Vec<RecvSocket>,
    create_rand: impl Fn() -> G,
) -> Endpoint
where
    SendSocket: crate::socket::send::Socket + Send + 'static,
    RecvSocket: crate::socket::recv::Socket + Send + 'static,
    G: crate::random::Generator,
    S: crate::stream2::Spawner,
    C: s2n_quic_core::time::Clock
        + crate::clock::precision::Clock
        + Clone
        + Send
        + 'static,
{
    let num_send = send_sockets.len();

    // Choose the routing implementation that best fits the socket count.
    if num_send.is_power_of_two() {
        setup_endpoint_inner::<_, _, _, _, _, routing::PowerOfTwoRoute>(
            config,
            send_sockets,
            recv_sockets,
            create_rand,
        )
    } else {
        setup_endpoint_inner::<_, _, _, _, _, routing::ModuloRoute>(
            config,
            send_sockets,
            recv_sockets,
            create_rand,
        )
    }
}

fn setup_endpoint_inner<SendSocket, RecvSocket, G, S, C, SenderRoute>(
    config: EndpointConfig<S, C>,
    send_sockets: Vec<SendSocket>,
    recv_sockets: Vec<RecvSocket>,
    create_rand: impl Fn() -> G,
) -> Endpoint
where
    SendSocket: crate::socket::send::Socket + Send + 'static,
    RecvSocket: crate::socket::recv::Socket + Send + 'static,
    G: crate::random::Generator,
    S: crate::stream2::Spawner,
    C: s2n_quic_core::time::Clock
        + crate::clock::precision::Clock
        + Clone
        + Send
        + 'static,
    SenderRoute: routing::SenderRoute,
{
    use crate::{
        counter::Registry as CounterRegistry,
        socket::channel::{cell, intrusive_queue},
        stream3::frame,
    };

    let EndpointConfig {
        spawner,
        send_pool,
        recv_pool,
        path_secret_map,
        gso,
        acceptor_registry,
        idle_timeout,
        clock,
        overall_send_rate,
    } = config;

    let num_workers = spawner.worker_count().max(1);
    let num_send = send_sockets.len();

    // The port our recv sockets listen on — embedded in outbound packets so peers can ACK back.
    let source_control_port = recv_sockets
        .first()
        .and_then(|s| s.local_addr().ok())
        .map(|a| a.port())
        .unwrap_or(0);

    // Frame submission channel: all writers share one sharded sender; one dispatch task drains it.
    let shard_count = (num_workers * 4).next_power_of_two();
    let (frame_tx, frame_rx) = frame::submission_channel(shard_count);

    // Per-send-socket channels ---------------------------------------------------
    // batch channel: pick_two routes FrameBatch items; the send task drains them.
    // ack channel:   dispatch tasks route ACK messages; the send task processes them.
    let (socket_batch_txs, socket_batch_rxs): (Vec<_>, Vec<_>) = (0..num_send)
        .map(|_| cell::sync::new::<tasks::FrameBatch>())
        .unzip();
    let (socket_ack_txs, socket_ack_rxs): (Vec<_>, Vec<_>) = (0..num_send)
        .map(|_| intrusive_queue::sync::new::<msg::Sender>())
        .unzip();

    // Shared flow-queue allocator and dispatch counters -------------------------
    let queue_allocator = msg::queue::Allocator::new();
    let queue_dispatcher = queue_allocator.dispatcher();
    let counter_registry = CounterRegistry::default();
    let counters = counters::Dispatch::new(&counter_registry);
    let decode_error_counter = counters.rx_none.clone();

    // Build workers -------------------------------------------------------------
    // Pre-allocate one Worker per spawner thread.
    //
    // `AckSnd` = `AckSender<EntryBoxSender<msg::Sender, intrusive_queue::sync::Sender<msg::Sender>>>`:
    // each `socket_ack_tx` is an `UnboundedSender<Entry<msg::Sender>>`; `EntryBoxSender` converts
    // it to `UnboundedSender<msg::Sender>` so `AckSender` only needs to route by sender id.
    type AckSnd = routing::AckSender<
        crate::socket::channel::EntryBoxSender<
            msg::Sender,
            crate::socket::channel::intrusive_queue::sync::Sender<msg::Sender>,
        >,
    >;
    let mut workers: Vec<Worker<SendSocket, RecvSocket, C, G, AckSnd>> = {
        let mut v = Vec::with_capacity(num_workers);
        v.extend((0..num_workers).map(|id| Worker::new(id, idle_timeout)));
        v
    };

    // Worker 0 runs frame-dispatch.
    workers[0].frame_dispatch = Some(FrameDispatchParts {
        frame_rx,
        batch_txs: socket_batch_txs,
        rand: create_rand(),
        clock: clock.clone(),
        overall_send_rate,
    });

    // Distribute send sockets across workers 1..=num_send (wrapping modulo num_workers).
    for (sender_idx, (socket, (batch_rx, ack_rx))) in send_sockets
        .into_iter()
        .zip(socket_batch_rxs.into_iter().zip(socket_ack_rxs.into_iter()))
        .enumerate()
    {
        let worker_id = (1 + sender_idx) % num_workers;
        workers[worker_id].send_tasks.push(SendTaskParts {
            socket,
            batch_rx,
            ack_rx,
            sender_idx,
            source_control_port,
            gso: gso.clone(),
            pool: send_pool.clone(),
        });
    }

    // Distribute recv sockets + dispatch pairs across workers (wrapping modulo num_workers).
    for (recv_idx, socket) in recv_sockets.into_iter().enumerate() {
        let worker_id = (1 + num_send + recv_idx) % num_workers;

        let (packet_tx, packet_rx) = intrusive_queue::sync::new();

        workers[worker_id].recv_tasks.push(RecvTaskParts {
            socket,
            recv_pool: recv_pool.clone(),
            packet_tx,
            decode_error_counter: decode_error_counter.clone(),
            packet_rx,
            path_secret_map: path_secret_map.clone(),
            acceptor_registry: acceptor_registry.clone(),
            frame_tx: frame_tx.clone(),
            ack_sender: routing::AckSender::new(
                socket_ack_txs
                    .iter()
                    .cloned()
                    .map(crate::socket::channel::EntryBoxSender::new)
                    .collect(),
            ),
            queue_dispatcher: queue_dispatcher.clone(),
            counters: counters.clone(),
            clock: clock.clone(),
        });
    }

    // Spawn all workers ---------------------------------------------------------
    for worker in workers {
        worker.spawn(&spawner);
    }

    Endpoint {
        frame_tx,
        path_secret_map,
        queue_allocator,
        acceptor_registry,
        next_stream_id: AtomicU64::new(0),
        data_port: source_control_port,
    }
}


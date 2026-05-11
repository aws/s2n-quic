// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Stream3 Endpoint: shared infrastructure for the process.

pub(crate) mod ack;
pub(crate) mod assemble;
pub(crate) mod combinator;
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
    socket::channel::intrusive_queue::sync as sync_queue,
    stream3::{frame::SubmissionSender, Stream},
};
use std::sync::atomic::AtomicU64;

type BatchSender = sync_queue::Sender<combinator::FrameBatch>;
type BatchReceiver = sync_queue::Receiver<combinator::FrameBatch>;
type AckMsgReceiver = sync_queue::Receiver<msg::Sender>;

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

/// Per-poll budgets for each pipeline task.
///
/// Each budget controls how many items a task processes per executor poll before yielding.
/// Lower values improve fairness across tasks; higher values improve throughput under load.
#[derive(Clone, Copy, Debug)]
pub struct Budgets {
    /// Budget for the frame-dispatch batcher+distributor task.
    pub frame_dispatch: usize,
    /// Budget for the per-socket assembler+sender and ACK processor tasks.
    pub socket_send: usize,
    /// Budget for the per-socket recv task.
    pub socket_recv: usize,
    /// Budget for the per-worker packet dispatch task.
    pub packet_dispatch: usize,
}

impl Default for Budgets {
    fn default() -> Self {
        Self {
            frame_dispatch: tasks::DEFAULT_DISPATCH_BUDGET,
            socket_send: tasks::DEFAULT_DISPATCH_BUDGET,
            socket_recv: tasks::DEFAULT_RECV_BUDGET,
            packet_dispatch: tasks::DEFAULT_DISPATCH_BUDGET,
        }
    }
}

/// Configuration for the stream3 pipeline.
pub struct Config<S, C> {
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
    /// Per-poll budgets for each pipeline task.
    pub budgets: Budgets,
    /// Number of shards for the frame submission channel.
    pub submission_shards: usize,
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
    config: Config<S, C>,
    send_sockets: Vec<SendSocket>,
    recv_sockets: Vec<RecvSocket>,
    create_rand: impl Fn() -> G,
) -> Endpoint
where
    SendSocket: crate::socket::send::Socket + Send + 'static,
    RecvSocket: crate::socket::recv::Socket + Send + 'static,
    G: crate::random::Generator,
    S: crate::stream2::Spawner,
    C: s2n_quic_core::time::Clock + crate::clock::precision::Clock + Clone + Send + 'static,
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
    config: Config<S, C>,
    send_sockets: Vec<SendSocket>,
    recv_sockets: Vec<RecvSocket>,
    create_rand: impl Fn() -> G,
) -> Endpoint
where
    SendSocket: crate::socket::send::Socket + Send + 'static,
    RecvSocket: crate::socket::recv::Socket + Send + 'static,
    G: crate::random::Generator,
    S: crate::stream2::Spawner,
    C: s2n_quic_core::time::Clock + crate::clock::precision::Clock + Clone + Send + 'static,
    SenderRoute: routing::SenderRoute,
{
    use crate::{
        counter::Registry as CounterRegistry, socket::channel::intrusive_queue, stream3::frame,
    };

    let Config {
        spawner,
        send_pool,
        recv_pool,
        path_secret_map,
        gso,
        acceptor_registry,
        idle_timeout,
        clock,
        overall_send_rate,
        budgets,
        submission_shards,
    } = config;

    let num_workers = spawner.worker_count().max(1);
    let num_send = send_sockets.len();

    assert!(
        num_send.is_power_of_two(),
        "number of sender sockets must be a power of two"
    );
    assert!(
        submission_shards.is_power_of_two(),
        "submission shard count must be a power of two"
    );

    // The port our recv sockets listen on — embedded in outbound packets so peers can ACK back.
    let source_control_port = recv_sockets
        .first()
        .and_then(|s| s.local_addr().ok())
        .map(|a| a.port())
        .unwrap_or(0);

    // Frame submission channel: all writers share one sharded sender; one dispatch task drains it.
    let (frame_tx, frame_rx) = frame::submission_channel(submission_shards);

    // Per-send-worker batch channels -----------------------------------------------
    // PickTwo routes Entry<FrameBatch> items to workers; the context resolver task drains them.
    // ACK channel: dispatch tasks route ACK messages to the correct send worker.
    let num_send_workers = num_workers.min(num_send).max(1);
    let (worker_batch_txs, worker_batch_rxs): (Vec<_>, Vec<_>) = (0..num_send_workers)
        .map(|_| intrusive_queue::sync::new::<combinator::FrameBatch>())
        .unzip();
    let (worker_ack_txs, worker_ack_rxs): (Vec<_>, Vec<_>) = (0..num_send_workers)
        .map(|_| intrusive_queue::sync::new::<msg::Sender>())
        .unzip();

    let mut sender_id_to_worker: Vec<usize> = Vec::with_capacity(num_send);

    // Shared flow-queue allocator and dispatch counters -------------------------
    let queue_allocator = msg::queue::Allocator::new();
    let queue_dispatcher = queue_allocator.dispatcher();
    let counter_registry = CounterRegistry::default();
    let counters = counters::Dispatch::new(&counter_registry);
    let decode_error_counter = counters.rx_none.clone();

    // Build workers -------------------------------------------------------------
    // Pre-allocate one Worker per spawner thread.
    let mut workers: Vec<Worker<SendSocket, RecvSocket, C, G, _>> = {
        let mut v = Vec::with_capacity(num_workers);
        v.extend((0..num_workers).map(|id| Worker::new(id, idle_timeout, budgets)));
        v
    };

    // Worker 0 runs frame-dispatch. PickTwo targets send workers via per-worker senders.
    workers[0].frame_dispatch = Some(FrameDispatchParts {
        frame_rx,
        worker_batch_txs,
        rand: create_rand(),
        clock: clock.clone(),
        overall_send_rate,
    });

    // Distribute send sockets across send workers, and pair each worker with its batch/ack rx.
    for (sender_idx, socket) in send_sockets.into_iter().enumerate() {
        let worker_id = sender_idx % num_send_workers;
        sender_id_to_worker.push(worker_id);
        let inflight_gauge = counter_registry.register_queue_gauge("send.inflight");
        workers[worker_id].send_sockets.push(SendSocketParts {
            socket,
            sender_idx,
            source_control_port,
            gso: gso.clone(),
            pool: send_pool.clone(),
            clock: clock.clone(),
            random: create_rand(),
            frame_tx: frame_tx.clone(),
            inflight_gauge,
        });
    }

    // Assign per-worker batch/ack receivers to the matching send workers.
    for (worker_id, (batch_rx, ack_rx)) in worker_batch_rxs
        .into_iter()
        .zip(worker_ack_rxs.into_iter())
        .enumerate()
    {
        workers[worker_id].send_worker = Some(SendWorkerParts { batch_rx, ack_rx });
    }

    // Build ACK sender after socket distribution so sender_id_to_worker is populated.
    let ack_sender = routing::AckSender::new(
        worker_ack_txs
            .into_iter()
            .map(crate::socket::channel::EntryBoxSender::new)
            .collect(),
        sender_id_to_worker,
    );

    // Distribute recv sockets + dispatch pairs across workers (wrapping modulo num_workers).
    for (recv_idx, socket) in recv_sockets.into_iter().enumerate() {
        let worker_id = (num_send_workers + recv_idx) % num_workers;

        let (packet_tx, packet_rx) = intrusive_queue::sync::new();
        let ack_sender = ack_sender.clone();

        workers[worker_id].recv_tasks.push(RecvTaskParts {
            socket,
            recv_pool: recv_pool.clone(),
            packet_tx,
            decode_error_counter: decode_error_counter.clone(),
            packet_rx,
            path_secret_map: path_secret_map.clone(),
            acceptor_registry: acceptor_registry.clone(),
            frame_tx: frame_tx.clone(),
            ack_sender,
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

// ── Worker parts ──────────────────────────────────────────────────────────

/// All the ingredients needed to spawn the frame-dispatch task on a worker.
struct FrameDispatchParts<G, Clk> {
    frame_rx: crate::stream3::frame::SubmissionReceiver,
    /// Per-worker senders for batch routing (PickTwo targets these).
    worker_batch_txs: Vec<BatchSender>,
    /// Random generator for pick-two routing.
    rand: G,
    /// Clock used by the pacing stage.
    clock: Clk,
    /// Overall bandwidth cap for the pacing stage.
    overall_send_rate: crate::socket::rate::Rate,
}

/// Per-worker state for context resolution and ACK processing.
struct SendWorkerParts {
    batch_rx: BatchReceiver,
    ack_rx: AckMsgReceiver,
}

/// Per-socket ingredients for the socket send task.
pub(crate) struct SendSocketParts<Socket, Clk, G> {
    pub socket: Socket,
    pub sender_idx: usize,
    pub source_control_port: u16,
    pub gso: s2n_quic_platform::features::Gso,
    pub pool: crate::socket::pool::Pool,
    pub clock: Clk,
    pub random: G,
    pub frame_tx: crate::stream3::frame::SubmissionSender,
    pub inflight_gauge: crate::counter::QueueGauge,
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

struct Worker<SendSocket, RecvSocket, Clk, G, AckSnd> {
    id: usize,
    idle_timeout: core::time::Duration,
    budgets: Budgets,
    frame_dispatch: Option<FrameDispatchParts<G, Clk>>,
    /// Per-worker batch/ack receiver (one per send worker).
    send_worker: Option<SendWorkerParts>,
    /// Send sockets assigned to this worker.
    send_sockets: Vec<SendSocketParts<SendSocket, Clk, G>>,
    /// Recv + dispatch task pairs assigned to this worker.
    recv_tasks: Vec<RecvTaskParts<RecvSocket, Clk, AckSnd>>,
}

impl<SendSocket, RecvSocket, Clk, G, AckSnd> Worker<SendSocket, RecvSocket, Clk, G, AckSnd>
where
    SendSocket: crate::socket::send::Socket + Send + 'static,
    RecvSocket: crate::socket::recv::Socket + Send + 'static,
    Clk: s2n_quic_core::time::Clock + crate::clock::precision::Clock + Clone + Send + 'static,
    G: crate::random::Generator + 'static,
    AckSnd: crate::socket::channel::UnboundedSender<msg::Sender> + Send + 'static,
{
    fn new(id: usize, idle_timeout: core::time::Duration, budgets: Budgets) -> Self {
        Self {
            id,
            idle_timeout,
            budgets,
            frame_dispatch: None,
            send_worker: None,
            send_sockets: Vec::new(),
            recv_tasks: Vec::new(),
        }
    }

    fn spawn<S: crate::stream2::Spawner>(self, spawner: &S) {
        use crate::stream2::spawner::LocalSpawner as _;

        let Self {
            id,
            idle_timeout,
            budgets,
            frame_dispatch,
            send_worker,
            send_sockets,
            recv_tasks,
        } = self;

        spawner.spawn_local(id, move |mut local| {
            let recv_cache = std::rc::Rc::new(std::cell::RefCell::new(
                crate::stream3::endpoint::recv::Cache::new(idle_timeout, id),
            ));

            if let Some(fd) = frame_dispatch {
                let mut random = fd.rand;
                let random_fn = move |n: usize| {
                    let mut bytes = [0u8; 8];
                    random.public_random_fill(&mut bytes);
                    let raw = u64::from_le_bytes(bytes) as usize;
                    debug_assert!(n.is_power_of_two(), "sender count must be a power of two");
                    raw & (n - 1)
                };
                tasks::frame_dispatch(
                    &mut local,
                    fd.frame_rx,
                    fd.worker_batch_txs,
                    random_fn,
                    fd.clock,
                    fd.overall_send_rate,
                    budgets.frame_dispatch,
                );
            }

            if let Some(sw) = send_worker {
                tasks::send_worker(
                    &mut local,
                    sw.batch_rx,
                    sw.ack_rx,
                    send_sockets,
                    budgets.socket_send,
                );
            }

            for rt in recv_tasks {
                local.spawn(tasks::socket_recv_task(
                    rt.socket,
                    rt.recv_pool,
                    rt.packet_tx,
                    rt.decode_error_counter,
                    budgets.socket_recv,
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
                    budgets.packet_dispatch,
                ));
            }
        });
    }
}

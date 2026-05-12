// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Stream3 Endpoint: shared infrastructure for the process.

pub use s2n_quic_platform::features::Gso;

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
pub mod socket;
pub(crate) mod tasks;
pub(crate) mod worker;

use crate::{
    acceptor, packet,
    socket::{channel::intrusive_queue::sync as sync_queue, pool::descriptor},
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

/// Per-poll budgets for each pipeline sub-task.
///
/// Each budget controls how many items a task processes per executor poll before yielding.
/// Lower values improve fairness across tasks; higher values improve throughput under load.
#[derive(Clone, Copy, Debug)]
pub struct Budgets {
    /// Budget for the frame-dispatch batcher+distributor task.
    pub frame_dispatch: usize,
    /// Budget for the send-worker context resolver task.
    pub context_resolver: usize,
    /// Budget for the send-worker ACK processor task.
    pub ack_processor: usize,
    /// Budget for the send-worker TX wheel drain task.
    pub tx_wheel: usize,
    /// Budget for the send-worker PTO wheel drain task.
    pub pto_wheel: usize,
    /// Budget for the send-worker idle wheel drain task.
    pub idle_wheel: usize,
    /// Budget for per-socket assembler+send tasks.
    pub assembler: usize,
    /// Budget for the completion dispatcher task (acked frames).
    pub completion_acked: usize,
    /// Budget for the completion dispatcher task (cancelled frames).
    pub completion_cancelled: usize,
    /// Budget for the per-socket recv task.
    pub socket_recv: usize,
    /// Budget for the per-worker packet dispatch task.
    pub packet_dispatch: usize,
}

impl Default for Budgets {
    fn default() -> Self {
        Self {
            frame_dispatch: tasks::DEFAULT_DISPATCH_BUDGET,
            context_resolver: tasks::DEFAULT_DISPATCH_BUDGET,
            ack_processor: tasks::DEFAULT_DISPATCH_BUDGET,
            tx_wheel: tasks::DEFAULT_DISPATCH_BUDGET,
            pto_wheel: tasks::DEFAULT_DISPATCH_BUDGET,
            idle_wheel: tasks::DEFAULT_DISPATCH_BUDGET,
            assembler: tasks::DEFAULT_DISPATCH_BUDGET,
            completion_acked: tasks::DEFAULT_DISPATCH_BUDGET,
            completion_cancelled: tasks::DEFAULT_DISPATCH_BUDGET,
            socket_recv: tasks::DEFAULT_RECV_BUDGET,
            packet_dispatch: tasks::DEFAULT_DISPATCH_BUDGET,
        }
    }
}

/// Assigns spawner thread indices to pipeline roles.
///
/// Each field is a list of worker IDs (indices into the spawner's thread pool). The spawner
/// must have at least `max(all IDs) + 1` threads. Overlapping IDs are allowed (e.g. recv_io
/// and recv_dispatch on the same worker) but typically kept separate for isolation.
#[derive(Debug)]
pub struct WorkerLayout {
    /// Which worker runs the frame dispatch task (single).
    pub frame_dispatch: usize,
    /// Workers that run send (context resolver + assembler + socket send).
    /// Send sockets are distributed round-robin across these workers.
    pub send: Vec<usize>,
    /// Workers that run recv IO (socket read + decode + fan-out to dispatch).
    /// Recv sockets are distributed round-robin across these workers.
    pub recv_io: Vec<usize>,
    /// Workers that run recv dispatch (decrypt + dedup + frame routing to queues).
    /// Packets are hash-routed to these workers by (credentials.id, source_sender_id).
    pub recv_dispatch: Vec<usize>,
}

/// Configuration for the stream3 pipeline.
pub struct Config<S, C> {
    /// Worker pool spawner.
    pub spawner: S,
    /// Worker layout — maps pipeline roles to spawner thread indices.
    pub layout: WorkerLayout,
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
    /// Per-socket bandwidth cap applied after assembly, before socket transmission.
    ///
    /// Each send socket gets its own [`Paced`] stage with this rate. This prevents any
    /// single socket from saturating the NIC queue even when the overall rate budget allows it.
    ///
    /// [`Paced`]: crate::socket::channel::Paced
    pub per_socket_send_rate: crate::socket::rate::Rate,
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
/// # Worker layout
///
/// The [`WorkerLayout`] in [`Config`] assigns pipeline roles to spawner thread indices:
///
/// * **frame_dispatch** (single): routes submitted frames to send workers via PickTwo.
/// * **send** workers: context resolution, assembly, and socket transmission.
/// * **recv_io** workers: socket reads, segment decoding, and hash-based fan-out.
/// * **recv_dispatch** workers: decryption, deduplication, and frame dispatch to queues.
///
/// Recv IO tasks fan out decoded packets to recv_dispatch workers by hashing
/// (credentials.id, source_sender_id), ensuring a given peer always lands in the same
/// recv::Cache for coherent ACK space and packet-number deduplication.
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
    let num_recv_dispatch = config.layout.recv_dispatch.len();

    tracing::debug!(?config.layout, "setting up endpoint");

    if num_recv_dispatch.is_power_of_two() {
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

fn setup_endpoint_inner<SendSocket, RecvSocket, G, S, C, RecvRoute>(
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
    RecvRoute: routing::SenderRoute,
{
    use crate::{
        counter::Registry as CounterRegistry, socket::channel::intrusive_queue, stream3::frame,
    };

    let Config {
        spawner,
        layout,
        send_pool,
        recv_pool,
        path_secret_map,
        gso,
        acceptor_registry,
        idle_timeout,
        clock,
        overall_send_rate,
        per_socket_send_rate,
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
    assert!(
        !layout.send.is_empty(),
        "at least one send worker is required"
    );
    assert!(
        layout.recv_io.len() == recv_sockets.len(),
        "recv_io worker count must match recv socket count"
    );
    assert!(
        !layout.recv_dispatch.is_empty(),
        "at least one recv_dispatch worker is required"
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
    let num_send_workers = layout.send.len();
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

    // Set the socket sender count on the map so path-secret entries allocate
    // per-socket transmission schedules for pick-two load balancing.
    path_secret_map.set_socket_sender_count(num_send);

    // Build workers -------------------------------------------------------------
    let mut workers: Vec<Worker<SendSocket, RecvSocket, C, G, _, RecvRoute>> = {
        let mut v = Vec::with_capacity(num_workers);
        v.extend(
            (0..num_workers)
                .map(|id| Worker::new(id, idle_timeout, budgets, num_send, clock.clone())),
        );
        v
    };

    // Distribute send sockets across send workers round-robin.
    for (sender_idx, socket) in send_sockets.into_iter().enumerate() {
        let worker_id = layout.send[sender_idx % num_send_workers];
        sender_id_to_worker.push(sender_idx % num_send_workers);
        let inflight_gauge = counter_registry.register_queue_gauge("send.inflight");
        workers[worker_id].send_sockets.push(SendSocketParts {
            socket,
            sender_idx,
            source_control_port,
            gso: gso.clone(),
            pool: send_pool.clone(),
            clock: clock.clone(),
            inflight_gauge,
            per_socket_send_rate,
        });
    }

    // Build per-socket-id senders: each socket ID maps to its owning worker's channel.
    let socket_senders: Vec<BatchSender> = sender_id_to_worker
        .iter()
        .map(|&worker_idx| worker_batch_txs[worker_idx].clone())
        .collect();

    // Frame-dispatch task on its designated worker.
    workers[layout.frame_dispatch].frame_dispatch = Some(FrameDispatchParts {
        frame_rx,
        socket_senders,
        rand: create_rand(),
        clock: clock.clone(),
        overall_send_rate,
    });

    // Assign per-send-worker batch/ack receivers.
    for (idx, (batch_rx, ack_rx)) in worker_batch_rxs
        .into_iter()
        .zip(worker_ack_rxs.into_iter())
        .enumerate()
    {
        let worker_id = layout.send[idx];
        workers[worker_id].send_worker = Some(SendWorkerParts {
            batch_rx,
            ack_rx,
            random: create_rand(),
            frame_tx: frame_tx.clone(),
        });
    }

    // Build ACK sender after socket distribution so sender_id_to_worker is populated.
    let ack_sender = routing::AckSender::new(
        worker_ack_txs
            .into_iter()
            .map(crate::socket::channel::EntryBoxSender::new)
            .collect(),
        sender_id_to_worker,
    );

    // ── Recv dispatch queues ─────────────────────────────────────────────────
    // One dispatch queue per recv_dispatch worker. Recv IO tasks fan out to all of these
    // using a hash of (credentials.id, source_sender_id) for peer affinity.
    let num_recv_dispatch = layout.recv_dispatch.len();
    let (dispatch_txs, dispatch_rxs): (Vec<_>, Vec<_>) = (0..num_recv_dispatch)
        .map(|_| {
            intrusive_queue::sync::new::<packet::datagram::decoder::Packet<descriptor::Filled>>()
        })
        .unzip();

    let ack_route = RecvRoute::new(num_send);
    for (idx, dispatch_rx) in dispatch_rxs.into_iter().enumerate() {
        let worker_id = layout.recv_dispatch[idx];
        workers[worker_id].recv_dispatch = Some(RecvDispatchParts {
            packet_rx: dispatch_rx,
            path_secret_map: path_secret_map.clone(),
            acceptor_registry: acceptor_registry.clone(),
            frame_tx: frame_tx.clone(),
            ack_sender: ack_sender.clone(),
            queue_dispatcher: queue_dispatcher.clone(),
            counters: counters.clone(),
            clock: clock.clone(),
            route: ack_route,
        });
    }

    // Assign each recv socket to its corresponding recv_io worker (1:1).
    for (socket, &worker_id) in recv_sockets.into_iter().zip(layout.recv_io.iter()) {
        let router = worker::FanOutRouter::<_, RecvRoute>::new(
            dispatch_txs.clone(),
            decode_error_counter.clone(),
        );
        workers[worker_id].recv_socket = Some(RecvSocketParts {
            socket,
            recv_pool: recv_pool.clone(),
            router,
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
    /// Per-socket-id senders: indexed by socket ID, each routes to the owning worker.
    socket_senders: Vec<BatchSender>,
    /// Random generator for pick-two routing.
    rand: G,
    /// Clock used by the pacing stage.
    clock: Clk,
    /// Overall bandwidth cap for the pacing stage.
    overall_send_rate: crate::socket::rate::Rate,
}

/// Per-worker state for context resolution and ACK processing.
struct SendWorkerParts<G> {
    batch_rx: BatchReceiver,
    ack_rx: AckMsgReceiver,
    random: G,
    frame_tx: SubmissionSender,
}

/// Per-socket ingredients for the socket send task.
pub(crate) struct SendSocketParts<Socket, Clk> {
    socket: Socket,
    sender_idx: usize,
    source_control_port: u16,
    gso: s2n_quic_platform::features::Gso,
    pool: crate::socket::pool::Pool,
    clock: Clk,
    inflight_gauge: crate::counter::QueueGauge,
    per_socket_send_rate: crate::socket::rate::Rate,
}

type PacketSender = sync_queue::Sender<packet::datagram::decoder::Packet<descriptor::Filled>>;
type PacketReceiver = sync_queue::Receiver<packet::datagram::decoder::Packet<descriptor::Filled>>;

/// Ingredients for a recv IO worker (socket read + decode + fan-out).
struct RecvSocketParts<Socket, Route> {
    socket: Socket,
    recv_pool: crate::socket::pool::Pool,
    router: worker::FanOutRouter<PacketSender, Route>,
}

/// Ingredients for a recv dispatch worker (decrypt + dedup + frame dispatch).
struct RecvDispatchParts<Clk, AckSnd, Route> {
    packet_rx: PacketReceiver,
    path_secret_map: crate::path::secret::Map,
    acceptor_registry: acceptor::Registry<Stream>,
    frame_tx: SubmissionSender,
    ack_sender: AckSnd,
    queue_dispatcher: msg::queue::Dispatcher,
    counters: counters::Dispatch,
    clock: Clk,
    route: Route,
}

// ── Worker ────────────────────────────────────────────────────────────────

struct Worker<SendSocket, RecvSocket, Clk, G, AckSnd, Route> {
    id: usize,
    idle_timeout: core::time::Duration,
    budgets: Budgets,
    total_sender_ids: usize,
    clock: Clk,
    frame_dispatch: Option<FrameDispatchParts<G, Clk>>,
    /// Per-worker batch/ack receiver (one per send worker).
    send_worker: Option<SendWorkerParts<G>>,
    /// Send sockets assigned to this worker.
    send_sockets: Vec<SendSocketParts<SendSocket, Clk>>,
    /// Recv IO: socket read + decode + fan-out (at most one per worker).
    recv_socket: Option<RecvSocketParts<RecvSocket, Route>>,
    /// Recv dispatch: decrypt + dedup + frame routing (at most one per worker).
    recv_dispatch: Option<RecvDispatchParts<Clk, AckSnd, Route>>,
}

impl<SendSocket, RecvSocket, Clk, G, AckSnd, Route>
    Worker<SendSocket, RecvSocket, Clk, G, AckSnd, Route>
where
    SendSocket: crate::socket::send::Socket + Send + 'static,
    RecvSocket: crate::socket::recv::Socket + Send + 'static,
    Clk: s2n_quic_core::time::Clock + crate::clock::precision::Clock + Clone + Send + 'static,
    G: crate::random::Generator + 'static,
    AckSnd: crate::socket::channel::UnboundedSender<msg::Sender> + Send + 'static,
    Route: routing::SenderRoute,
{
    fn new(
        id: usize,
        idle_timeout: core::time::Duration,
        budgets: Budgets,
        total_sender_ids: usize,
        clock: Clk,
    ) -> Self {
        Self {
            id,
            idle_timeout,
            budgets,
            total_sender_ids,
            clock,
            frame_dispatch: None,
            send_worker: None,
            send_sockets: Vec::new(),
            recv_socket: None,
            recv_dispatch: None,
        }
    }

    fn spawn<S: crate::stream2::Spawner>(self, spawner: &S) {
        use crate::stream2::spawner::LocalSpawner as _;

        let Self {
            id,
            idle_timeout,
            budgets,
            total_sender_ids,
            clock,
            frame_dispatch,
            send_worker,
            send_sockets,
            recv_socket,
            recv_dispatch,
        } = self;

        spawner.spawn_local(id, move |mut local| {
            if let Some(fd) = frame_dispatch {
                let mut random = fd.rand;
                let random_fn = move || {
                    let mut bytes = [0u8; 8];
                    random.public_random_fill(&mut bytes);
                    u64::from_le_bytes(bytes) as usize
                };
                tasks::frame_dispatch(
                    &mut local,
                    fd.frame_rx,
                    fd.socket_senders,
                    random_fn,
                    fd.clock,
                    fd.overall_send_rate,
                    budgets,
                );
            }

            if let Some(sw) = send_worker {
                tasks::send_worker(
                    &mut local,
                    sw.batch_rx,
                    sw.ack_rx,
                    total_sender_ids,
                    send_sockets,
                    clock.clone(),
                    sw.random,
                    sw.frame_tx,
                    budgets,
                );
            }

            if let Some(rs) = recv_socket {
                local.spawn(tasks::socket_recv_task(
                    rs.socket,
                    rs.recv_pool,
                    rs.router,
                    budgets,
                ));
            }

            if let Some(rd) = recv_dispatch {
                let recv_cache = std::rc::Rc::new(std::cell::RefCell::new(
                    crate::stream3::endpoint::recv::Cache::new(idle_timeout, id),
                ));
                local.spawn(tasks::packet_dispatch_task(
                    rd.packet_rx,
                    recv_cache,
                    rd.path_secret_map,
                    rd.acceptor_registry,
                    rd.frame_tx,
                    rd.ack_sender,
                    rd.queue_dispatcher,
                    rd.counters,
                    rd.clock,
                    rd.route,
                    budgets,
                ));
            }
        });
    }
}

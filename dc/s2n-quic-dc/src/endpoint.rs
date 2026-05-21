// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! stream Endpoint: shared infrastructure for the process.

use crate::{
    acceptor,
    counter::GaugedQueueReceiver,
    endpoint::{
        frame::SubmissionSender,
        id::{
            Id, IdJoin, IdMap, LocalSendSocketId, LocalSenderId, RecvDispatchWorkerId,
            RecvIoWorkerId, SendWorkerId,
        },
    },
    intrusive::Entry,
    packet,
    socket::{
        channel::{intrusive::sync as sync_queue, GaugedSender, UnboundedSender},
        pool::descriptor,
    },
    stream::PendingValidation,
    time::precision,
    tracing::*,
};
use core::time::Duration;
use s2n_quic_core::{time, varint::VarInt};
use std::sync::{atomic::AtomicU64, Arc};

pub(crate) mod ack;
pub(crate) mod assemble;
pub(crate) mod combinator;
pub(crate) mod counters;
pub(crate) mod decode;
pub(crate) mod dispatch;
pub(crate) mod error;
pub(crate) mod frame;
pub mod id;
pub(crate) mod inflight;
pub(crate) mod msg;
pub(crate) mod recv;
pub(crate) mod routing;
pub(crate) mod send;
pub mod socket;
pub(crate) mod tasks;
pub(crate) mod ups;
pub(crate) mod waker;
pub(crate) mod worker;

#[cfg(any(test, feature = "testing"))]
pub mod testing;

#[cfg(test)]
mod tests;

pub use error::Error;
pub use s2n_quic_platform::features::Gso;

/// The maximum time a stream will be open without activity from the peer
pub const DEFAULT_IDLE_TIMEOUT: Duration = Duration::from_secs(30);
/// Cooldown after a peer is marked dead before new flows are allowed.
pub const DEFAULT_DEAD_PEER_COOLDOWN: Duration = Duration::from_secs(30);
/// The maximum length of a single packet
pub const MAX_DATAGRAM_SIZE: usize = 1 << 15; // 32k

type BatchSender =
    GaugedSender<sync_queue::Sender<combinator::FrameBatch>, Entry<combinator::FrameBatch>>;
type BatchReceiver = sync_queue::Receiver<combinator::FrameBatch>;
type AckMsgReceiver = sync_queue::Receiver<msg::Sender>;

pub struct Endpoint {
    /// Frame submission channel (writers submit frame inputs here)
    pub frame_tx: SubmissionSender,
    /// Path secret map (shared with PSK providers)
    pub path_secret_map: crate::path::secret::Map,
    /// Queue allocator for flow queues
    pub queue_allocator: msg::queue::Allocator,
    /// Acceptor registry for server-side stream dispatch
    pub acceptor_registry: acceptor::Registry<PendingValidation>,
    /// Counters associated with this endpoint
    pub counters: crate::counter::Registry,
    /// Endpoint-wide stream ID counter
    pub next_stream_id: AtomicU64,
    /// Recv socket addresses advertised to peers during handshake.
    /// Each recv worker has its own distinct address.
    pub data_addrs: Vec<std::net::SocketAddr>,
    /// Cooldown period during which new flows are rejected after a peer is marked dead.
    pub dead_peer_cooldown: Duration,
}

// ── Pipeline Setup ────────────────────────────────────────────────────────

/// Per-poll budgets for each pipeline sub-task.
///
/// Each budget controls how many items a task processes per executor poll before yielding.
/// Lower values improve fairness across tasks; higher values improve throughput under load.
#[derive(Clone, Copy, Debug)]
pub struct Budgets {
    /// Budget for the submission router (shards drained per poll).
    pub submission_router: usize,
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
    /// Budget for the per-worker ACK burst drain task (`ack_burst_task`), i.e.
    /// how many pending recv contexts are encoded/sent per poll.
    pub ack_burst: usize,
    /// Budget for the waker drain task (wakers fired per poll).
    pub waker_drain: usize,
    /// Budget for the ACK completion drain task (entries returned from assembler per poll).
    pub ack_completion: usize,
    /// Budget for the per-worker invalidation drain task.
    pub invalidation: usize,
}

impl Default for Budgets {
    fn default() -> Self {
        Self {
            submission_router: 32,
            frame_dispatch: 128,
            context_resolver: 128,
            ack_processor: 256,
            tx_wheel: tasks::DEFAULT_DISPATCH_BUDGET,
            pto_wheel: tasks::DEFAULT_DISPATCH_BUDGET,
            idle_wheel: 1,
            assembler: tasks::DEFAULT_DISPATCH_BUDGET,
            completion_acked: 128,
            completion_cancelled: tasks::DEFAULT_DISPATCH_BUDGET,
            socket_recv: tasks::DEFAULT_RECV_BUDGET,
            packet_dispatch: 4096,
            ack_burst: 256,
            waker_drain: 512,
            ack_completion: tasks::DEFAULT_DISPATCH_BUDGET,
            invalidation: 1,
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
    /// Workers that run waker drain tasks (fire wakers offloaded from dispatch/send workers).
    /// Multiple workers are supported for sharding if the single-thread budget is exceeded.
    pub waker_drain: Vec<usize>,
    /// Worker that runs background housekeeping tasks (e.g. invalidation validation).
    pub background: usize,
}

/// Configuration for the stream pipeline.
pub struct Config {
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
    pub acceptor_registry: acceptor::Registry<PendingValidation>,
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
    /// Rate limit for UnknownPathSecret responses (pacing rate).
    pub ups_rate: crate::socket::rate::Rate,
    /// Dedup LRU capacity for UnknownPathSecret responses.
    pub ups_dedup_capacity: usize,
    /// Dedup suppression window for UnknownPathSecret responses.
    pub ups_dedup_window: core::time::Duration,
    /// Cooldown period during which new flows are rejected after a peer is marked dead.
    pub dead_peer_cooldown: core::time::Duration,
}

// ── setup_endpoint ────────────────────────────────────────────────────────

/// Assembles the stream pipeline from pre-opened sockets and spawns worker tasks.
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
pub fn setup_endpoint<SendSocket, RecvSocket, UpsSocket, R>(
    runtime: R,
    config: Config,
    send_sockets: Vec<SendSocket>,
    recv_sockets: Vec<RecvSocket>,
    ups_socket: UpsSocket,
) -> Endpoint
where
    SendSocket: crate::socket::send::Socket + Send + 'static,
    RecvSocket: crate::socket::recv::Socket + Send + 'static,
    UpsSocket: crate::socket::send::Socket + Send + 'static,
    R: crate::runtime::Runtime,
{
    let num_recv_dispatch = config.layout.recv_dispatch.len();

    let send_sockets: IdMap<LocalSendSocketId, _> = send_sockets.into();
    let recv_sockets: IdMap<id::LocalRecvSocketId, _> = recv_sockets.into();

    debug!(
        ?config.layout,
        send_sockets = ?send_sockets.iter().map(|(id, socket)| (id, socket.local_addr())).collect::<Vec<_>>(),
        recv_sockets = ?recv_sockets.iter().map(|(id, socket)| (id, socket.local_addr())).collect::<Vec<_>>(),
        "setting up endpoint"
    );

    if num_recv_dispatch.is_power_of_two() {
        setup_endpoint_inner::<_, _, _, _, routing::PowerOfTwoRoute>(
            runtime,
            config,
            send_sockets,
            recv_sockets,
            ups_socket,
        )
    } else {
        setup_endpoint_inner::<_, _, _, _, routing::ModuloRoute>(
            runtime,
            config,
            send_sockets,
            recv_sockets,
            ups_socket,
        )
    }
}

fn setup_endpoint_inner<SendSocket, RecvSocket, UpsSocket, R, RecvRoute>(
    runtime: R,
    config: Config,
    send_sockets: IdMap<LocalSendSocketId, SendSocket>,
    recv_sockets: IdMap<id::LocalRecvSocketId, RecvSocket>,
    ups_socket: UpsSocket,
) -> Endpoint
where
    SendSocket: crate::socket::send::Socket + Send + 'static,
    RecvSocket: crate::socket::recv::Socket + Send + 'static,
    UpsSocket: crate::socket::send::Socket + Send + 'static,
    R: crate::runtime::Runtime,
    RecvRoute: routing::SenderRoute,
{
    use crate::{counter::Registry as CounterRegistry, socket::channel::intrusive};

    let Config {
        layout,
        send_pool,
        recv_pool,
        path_secret_map,
        gso,
        acceptor_registry,
        overall_send_rate,
        per_socket_send_rate,
        budgets,
        submission_shards,
        ups_rate,
        ups_dedup_capacity,
        ups_dedup_window,
        dead_peer_cooldown,
    } = config;

    let clock = runtime.clock();

    let num_workers = runtime.worker_count().max(1);
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

    // Collect all recv socket addresses — advertised to peers so senders can target
    // individual recv workers directly.
    let data_addrs: Vec<std::net::SocketAddr> = recv_sockets
        .iter()
        .map(|(_, s)| {
            s.local_addr()
                .expect("recv socket must have a local address")
        })
        .collect();
    let source_control_port = data_addrs.first().map_or(0, |a| a.port());

    // Frame submission channel: all writers share one sharded sender; one dispatch task drains it.
    let (frame_tx, frame_rx) = frame::submission_channel(submission_shards);

    // Shared flow-queue allocator and dispatch counters -------------------------
    let queue_allocator = msg::queue::Allocator::new();
    let queue_dispatcher = queue_allocator.dispatcher();
    let counter_registry = CounterRegistry::default();
    let counters = counters::Dispatch::new(&counter_registry);

    // Per-send-worker batch channels -----------------------------------------------
    let num_send_workers = layout.send.len();
    let (worker_batch_txs, worker_batch_rxs): (
        IdMap<id::SendWorkerId, _>,
        IdMap<id::SendWorkerId, _>,
    ) = SendWorkerId::range(num_send_workers)
        .map(|id| {
            let (tx, rx) = intrusive::sync::new::<combinator::FrameBatch>();
            let gauge = counter_registry
                .register_queue_gauge_nominal("q.resolver", format_args!("send.{id}"));
            let tx = GaugedSender::new(tx, gauge);
            ((id, tx), (id, rx))
        })
        .unzip();
    let (worker_ack_txs, worker_ack_rxs): (IdMap<id::SendWorkerId, _>, IdMap<id::SendWorkerId, _>) =
        SendWorkerId::range(num_send_workers)
            .map(|id| {
                let (tx, rx) = intrusive::sync::new::<msg::Sender>();
                let gauge = counter_registry
                    .register_queue_gauge_nominal("q.ack", format_args!("send.{id}"));
                let tx = GaugedSender::new(tx, gauge);
                ((id, tx), (id, rx))
            })
            .unzip();

    // UPS channel: recv_dispatch workers → background (UnknownPathSecret responses)
    let (ups_tx, ups_rx) = intrusive::sync::new::<ups::Response>();
    let ups_queue_gauge = counter_registry.register_queue_gauge("q.ups");
    let ups_tx = GaugedSender::new(ups_tx, ups_queue_gauge.clone());

    // Invalidation channels: recv IO → background (raw segments) and background → workers
    let (invalidation_raw_tx, invalidation_raw_rx) = intrusive::sync::new::<descriptor::Filled>();
    let (invalidation_send_txs, invalidation_send_rxs): (
        IdMap<id::SendWorkerId, _>,
        IdMap<id::SendWorkerId, _>,
    ) = SendWorkerId::range(num_send_workers)
        .map(|id| {
            let (tx, rx) = intrusive::sync::new::<tasks::Invalidation>();
            let gauge = counter_registry
                .register_queue_gauge_nominal("q.invalidation", format_args!("send.{id}"));
            let tx = GaugedSender::new(tx, gauge);
            ((id, tx), (id, rx))
        })
        .unzip();
    let (invalidation_recv_txs, invalidation_recv_rxs): (
        IdMap<id::RecvDispatchWorkerId, _>,
        IdMap<id::RecvDispatchWorkerId, _>,
    ) = RecvDispatchWorkerId::range(layout.recv_dispatch.len())
        .map(|id| {
            let (tx, rx) = intrusive::sync::new::<tasks::Invalidation>();
            let gauge = counter_registry
                .register_queue_gauge_nominal("q.invalidation", format_args!("recv.{id}"));
            let tx = GaugedSender::new(tx, gauge);
            ((id, tx), (id, rx))
        })
        .unzip();
    let (peer_dead_tx, peer_dead_rx) = intrusive::sync::new::<tasks::PeerDead>();
    let peer_dead_tx = GaugedSender::new(
        peer_dead_tx,
        counter_registry.register_queue_gauge("q.peer_dead"),
    );

    let mut sender_id_to_worker: IdMap<LocalSenderId, id::SendWorkerId> = IdMap::default();

    // Set the socket sender count on the map so path-secret entries allocate
    // per-socket transmission schedules for pick-two load balancing.
    path_secret_map.set_socket_sender_count(num_send);

    // Build workers -------------------------------------------------------------
    let mut workers: Vec<
        Worker<
            socket::MeteredSend<SendSocket>,
            socket::MeteredRecv<RecvSocket>,
            UpsSocket,
            R::Clock,
            _,
            RecvRoute,
            _,
        >,
    > = {
        let mut v = Vec::with_capacity(num_workers);
        v.extend((0..num_workers).map(|id| {
            Worker::new(
                id,
                budgets,
                num_send,
                clock.clone(),
                dead_peer_cooldown,
                counter_registry.clone(),
            )
        }));
        v
    };

    // Distribute send sockets across send workers round-robin.
    for (socket_id, socket) in send_sockets.into_iter() {
        let raw_idx = socket_id.as_usize();
        let sender_idx = LocalSenderId::new(VarInt::new(raw_idx as u64).unwrap());
        let worker_id = layout.send[raw_idx % num_send_workers];
        sender_id_to_worker.extend(core::iter::once((
            sender_idx,
            id::SendWorkerId::new(raw_idx % num_send_workers),
        )));
        let socket = socket::MeteredSend::new(
            socket,
            counter_registry.register("socket.tx.ops"),
            counter_registry.register_bytes("socket.tx.bytes"),
        );
        workers[worker_id].send_sockets.push(SendSocketParts {
            socket,
            sender_idx,
            source_control_port,
            gso: gso.clone(),
            pool: send_pool.clone(),
            clock: clock.clone(),
            per_socket_send_rate,
        });
    }

    // Build per-socket senders: each socket ID maps to its owning worker's channel.
    let socket_senders: IdMap<LocalSenderId, BatchSender> = sender_id_to_worker
        .iter()
        .map(|(sender_id, &worker_idx)| (sender_id, worker_batch_txs[worker_idx].clone()))
        .collect();

    // Frame-dispatch task on its designated worker.
    workers[layout.frame_dispatch].frame_dispatch = Some(FrameDispatchParts {
        frame_rx,
        socket_senders,
        clock: clock.clone(),
        overall_send_rate,
    });

    // ── Waker offload ─────────────────────────────────────────────────────────
    // One slot per producer (recv_dispatch + send workers + background peer-dead fanout task),
    // partitioned across waker_drain workers.
    let num_recv_dispatch = layout.recv_dispatch.len();
    let num_waker_slots = num_recv_dispatch + num_send_workers + 1;
    let num_waker_drains = layout.waker_drain.len().max(1);
    let (mut waker_sinks, waker_drains) = waker::new(num_waker_slots, num_waker_drains);
    let mut send_and_bg_waker_sinks = waker_sinks.split_off(num_recv_dispatch);
    let bg_waker_sink = send_and_bg_waker_sinks
        .pop()
        .expect("background waker sink must exist");
    let send_waker_sinks = send_and_bg_waker_sinks;

    assert_eq!(send_waker_sinks.len(), num_send_workers);
    let send_waker_sinks: IdMap<SendWorkerId, _> = SendWorkerId::range(send_waker_sinks.len())
        .zip(send_waker_sinks)
        .collect();

    assert_eq!(waker_sinks.len(), num_recv_dispatch);
    let waker_sinks: IdMap<RecvDispatchWorkerId, _> =
        RecvDispatchWorkerId::range(num_recv_dispatch)
            .zip(waker_sinks)
            .collect();

    for (idx, drain) in waker_drains.into_iter().enumerate() {
        let worker_id = layout.waker_drain[idx % layout.waker_drain.len()];
        let prev = workers[worker_id].waker_drain.replace(drain);
        assert!(
            prev.is_none(),
            "worker {worker_id} assigned multiple waker drain tasks"
        );
    }

    // ACK completion channels: one per recv dispatch worker. Send workers route completed
    // ACK entries back to the recv worker that submitted them.
    let (ack_completion_txs, ack_completion_rxs): (
        IdMap<RecvDispatchWorkerId, _>,
        IdMap<RecvDispatchWorkerId, _>,
    ) = RecvDispatchWorkerId::range(num_recv_dispatch)
        .map(|id| {
            let (tx, rx) = crate::socket::channel::intrusive::sync::new::<msg::Sender>();
            let gauge = counter_registry
                .register_queue_gauge_nominal("q.dispatch", format_args!("recv.dispatch.{id}"));
            let tx = GaugedSender::new(tx, gauge);
            ((id, tx), (id, rx))
        })
        .unzip();
    let ack_completions_tx = routing::AckCompletionSender::new(ack_completion_txs);

    // Assign per-send-worker batch/ack receivers.
    for (send_worker_id, batch_rx, ack_rx, invalidation_rx, waker_sink) in (
        worker_batch_rxs,
        worker_ack_rxs,
        invalidation_send_rxs,
        send_waker_sinks,
    )
        .join()
    {
        let worker_id = layout.send[send_worker_id.as_usize()];
        workers[worker_id].send_worker = Some(SendWorkerParts {
            idx: send_worker_id,
            batch_rx,
            batch_gauge: counter_registry
                .register_queue_gauge_nominal("q.resolver", format_args!("send.{send_worker_id}")),
            ack_rx,
            ack_gauge: counter_registry
                .register_queue_gauge_nominal("q.ack", format_args!("send.{send_worker_id}")),
            random: crate::xorshift::Rng::new(),
            frame_tx: frame_tx.clone(),
            ack_completions_tx: ack_completions_tx.clone(),
            waker_sink,
            invalidation_rx,
            peer_dead_tx: peer_dead_tx.clone(),
        });
    }

    // Build ACK sender after socket distribution so sender_id_to_worker is populated.
    let ack_sender = routing::AckSender::new(worker_ack_txs, &sender_id_to_worker);

    // ── Recv dispatch queues ─────────────────────────────────────────────────
    // One dispatch queue per recv_dispatch worker. Recv IO tasks fan out to all of these
    // using a hash of (credentials.id, source_sender_id) for peer affinity.
    let (dispatch_txs, dispatch_rxs): (
        IdMap<RecvDispatchWorkerId, PacketSender>,
        IdMap<RecvDispatchWorkerId, PacketReceiver>,
    ) = RecvDispatchWorkerId::range(num_recv_dispatch)
        .map(|id| {
            let (tx, rx) =
                intrusive::sync::new::<packet::datagram::decoder::Packet<descriptor::Filled>>();
            let gauge = counter_registry
                .register_queue_gauge_nominal("q.dispatch_rx", format_args!("recv.{id}"));
            ((id, GaugedSender::new(tx, gauge)), (id, rx))
        })
        .unzip();

    let ack_route = RecvRoute::new(num_send);
    for (recv_dispatch_id, dispatch_rx, ack_completion_rx, invalidation_rx, waker_sink) in (
        dispatch_rxs,
        ack_completion_rxs,
        invalidation_recv_rxs,
        waker_sinks,
    )
        .join()
    {
        let worker_id = layout.recv_dispatch[recv_dispatch_id.as_usize()];
        workers[worker_id].recv_dispatch = Some(RecvDispatchParts {
            packet_rx: dispatch_rx,
            packet_gauge: counter_registry.register_queue_gauge_nominal(
                "q.dispatch_rx",
                format_args!("recv.{recv_dispatch_id}"),
            ),
            path_secret_map: path_secret_map.clone(),
            acceptor_registry: acceptor_registry.clone(),
            frame_tx: frame_tx.clone(),
            ack_sender: ack_sender.clone(),
            ack_completion_rx,
            recv_dispatch_idx: recv_dispatch_id,
            queue_dispatcher: queue_dispatcher.clone(),
            counters: counters.clone(),
            clock: clock.clone(),
            route: ack_route,
            waker_sink,
            ups_tx: ups_tx.clone(),
            invalidation_rx,
            peer_dead_tx: peer_dead_tx.clone(),
        });
    }

    // Assign each recv socket to its corresponding recv_io worker (1:1).
    let rx_ops_total = counter_registry.register("socket.rx.ops");
    let rx_bytes_total = counter_registry.register_bytes("socket.rx.bytes");
    for ((recv_socket_id, socket), &worker_id) in
        recv_sockets.into_iter().zip(layout.recv_io.iter())
    {
        let recv_io_id = RecvIoWorkerId::new(recv_socket_id.as_usize());
        let router = worker::FanOutRouter::<_, RecvRoute, _>::new(
            dispatch_txs.clone(),
            invalidation_raw_tx.clone(),
            &counter_registry,
        );
        let socket = socket::MeteredRecv::new(
            socket,
            counter_registry.register_nominal("socket.rx.ops", format_args!("recv.{recv_io_id}")),
            counter_registry.register_nominal("socket.rx.bytes", format_args!("recv.{recv_io_id}")),
            rx_ops_total.clone(),
            rx_bytes_total.clone(),
        );
        workers[worker_id].recv_socket = Some(RecvSocketParts {
            idx: recv_io_id,
            socket,
            recv_pool: recv_pool.clone(),
            router,
        });
    }

    // Background worker — invalidation validation + future housekeeping.
    workers[layout.background].background = Some(BackgroundParts {
        raw_rx: invalidation_raw_rx,
        peer_dead_rx,
        path_secret_map: path_secret_map.clone(),
        send_txs: invalidation_send_txs,
        recv_txs: invalidation_recv_txs,
        sender_id_to_worker,
        ups_rx,
        ups_queue_gauge: counter_registry.register_queue_gauge("q.ups"),
        ups_socket,
        ups_rate,
        ups_dedup_capacity,
        ups_dedup_window,
        acceptor_cleaner: acceptor_registry.cleaner(),
        queue_dispatcher: queue_dispatcher.clone(),
        waker_sink: bg_waker_sink,
    });

    // Spawn all workers ---------------------------------------------------------
    for worker in workers {
        worker.spawn(&runtime);
    }

    Endpoint {
        frame_tx,
        path_secret_map,
        queue_allocator,
        acceptor_registry,
        counters: counter_registry,
        next_stream_id: AtomicU64::new(0),
        data_addrs,
        dead_peer_cooldown,
    }
}

// ── Worker parts ──────────────────────────────────────────────────────────

/// All the ingredients needed to spawn the frame-dispatch task on a worker.
struct FrameDispatchParts<Clk> {
    frame_rx: frame::SubmissionReceiver,
    /// Per-socket-id senders: indexed by socket ID, each routes to the owning worker.
    socket_senders: IdMap<LocalSenderId, BatchSender>,
    /// Clock used by the pacing stage.
    clock: Clk,
    /// Overall bandwidth cap for the pacing stage.
    overall_send_rate: crate::socket::rate::Rate,
}

/// Per-worker state for context resolution and ACK processing.
struct SendWorkerParts {
    idx: SendWorkerId,
    batch_rx: BatchReceiver,
    batch_gauge: crate::counter::QueueGauge,
    ack_rx: AckMsgReceiver,
    ack_gauge: crate::counter::QueueGauge,
    random: crate::xorshift::Rng,
    frame_tx: SubmissionSender,
    ack_completions_tx: routing::AckCompletionSender<
        GaugedSender<sync_queue::Sender<msg::Sender>, crate::intrusive::Queue<msg::Sender>>,
    >,
    waker_sink: waker::Sink,
    invalidation_rx: sync_queue::Receiver<tasks::Invalidation>,
    peer_dead_tx: PeerDeadSender,
}

/// Per-socket ingredients for the socket send task.
pub(crate) struct SendSocketParts<Socket, Clk> {
    socket: Socket,
    sender_idx: LocalSenderId,
    source_control_port: u16,
    gso: s2n_quic_platform::features::Gso,
    pool: crate::socket::pool::Pool,
    clock: Clk,
    per_socket_send_rate: crate::socket::rate::Rate,
}

type PacketSender = GaugedSender<
    sync_queue::Sender<packet::datagram::decoder::Packet<descriptor::Filled>>,
    Entry<packet::datagram::decoder::Packet<descriptor::Filled>>,
>;
type PacketReceiver = sync_queue::Receiver<packet::datagram::decoder::Packet<descriptor::Filled>>;

/// Ingredients for a recv IO worker (socket read + decode + fan-out).
struct RecvSocketParts<Socket, Route, Inv> {
    idx: RecvIoWorkerId,
    socket: Socket,
    recv_pool: crate::socket::pool::Pool,
    router: worker::FanOutRouter<PacketSender, Route, Inv>,
}

/// Ingredients for a recv dispatch worker (decrypt + dedup + frame dispatch).
struct RecvDispatchParts<Clk, AckSnd, Route> {
    packet_rx: PacketReceiver,
    packet_gauge: crate::counter::QueueGauge,
    path_secret_map: crate::path::secret::Map,
    acceptor_registry: acceptor::Registry<PendingValidation>,
    frame_tx: SubmissionSender,
    ack_sender: AckSnd,
    ack_completion_rx: sync_queue::Receiver<msg::Sender>,
    /// Index into the AckCompletionSender's staging array (0..num_recv_dispatch).
    recv_dispatch_idx: RecvDispatchWorkerId,
    queue_dispatcher: msg::queue::Dispatcher,
    counters: Arc<counters::Dispatch>,
    clock: Clk,
    route: Route,
    waker_sink: waker::Sink,
    ups_tx: UpsSender,
    invalidation_rx: sync_queue::Receiver<tasks::Invalidation>,
    peer_dead_tx: PeerDeadSender,
}

/// Ingredients for the background worker (invalidation validation + future housekeeping).
struct BackgroundParts<UpsSocket> {
    raw_rx: sync_queue::Receiver<descriptor::Filled>,
    peer_dead_rx: sync_queue::Receiver<tasks::PeerDead>,
    path_secret_map: crate::path::secret::Map,
    send_txs: IdMap<id::SendWorkerId, InvalidationSender>,
    recv_txs: IdMap<id::RecvDispatchWorkerId, InvalidationSender>,
    sender_id_to_worker: IdMap<LocalSenderId, id::SendWorkerId>,
    ups_rx: sync_queue::Receiver<ups::Response>,
    ups_queue_gauge: crate::counter::QueueGauge,
    ups_socket: UpsSocket,
    ups_rate: crate::socket::rate::Rate,
    ups_dedup_capacity: usize,
    ups_dedup_window: core::time::Duration,
    acceptor_cleaner: acceptor::Cleaner<PendingValidation>,
    queue_dispatcher: msg::queue::Dispatcher,
    waker_sink: waker::Sink,
}

type InvalidationSender =
    GaugedSender<sync_queue::Sender<tasks::Invalidation>, Entry<tasks::Invalidation>>;
type PeerDeadSender = GaugedSender<sync_queue::Sender<tasks::PeerDead>, Entry<tasks::PeerDead>>;

type UpsSender = GaugedSender<sync_queue::Sender<ups::Response>, Entry<ups::Response>>;

// ── Worker ────────────────────────────────────────────────────────────────

struct Worker<SendSocket, RecvSocket, UpsSocket, Clk, AckSnd, Route, Inv> {
    id: usize,
    budgets: Budgets,
    total_sender_ids: usize,
    dead_peer_cooldown: core::time::Duration,
    clock: Clk,
    counter_registry: crate::counter::Registry,
    frame_dispatch: Option<FrameDispatchParts<Clk>>,
    /// Per-worker batch/ack receiver (one per send worker).
    send_worker: Option<SendWorkerParts>,
    /// Send sockets assigned to this worker.
    send_sockets: Vec<SendSocketParts<SendSocket, Clk>>,
    /// Recv IO: socket read + decode + fan-out (at most one per worker).
    recv_socket: Option<RecvSocketParts<RecvSocket, Route, Inv>>,
    /// Recv dispatch: decrypt + dedup + frame routing (at most one per worker).
    recv_dispatch: Option<RecvDispatchParts<Clk, AckSnd, Route>>,
    /// Waker drain task assigned to this worker.
    waker_drain: Option<waker::Drain>,
    /// Background worker parts (invalidation validation).
    background: Option<BackgroundParts<UpsSocket>>,
}

impl<SendSocket, RecvSocket, UpsSocket, Clk, AckSnd, Route, Inv>
    Worker<SendSocket, RecvSocket, UpsSocket, Clk, AckSnd, Route, Inv>
where
    SendSocket: crate::socket::send::Socket + Send + 'static,
    RecvSocket: crate::socket::recv::Socket + Send + 'static,
    UpsSocket: crate::socket::send::Socket + Send + 'static,
    Clk: time::Clock + precision::Clock + Clone + Send + 'static,
    AckSnd: UnboundedSender<Entry<msg::Sender>> + Clone + Send + 'static,
    Route: routing::SenderRoute,
    Inv: UnboundedSender<Entry<descriptor::Filled>> + Send + 'static,
{
    #[inline]
    fn new(
        id: usize,
        budgets: Budgets,
        total_sender_ids: usize,
        clock: Clk,
        dead_peer_cooldown: core::time::Duration,
        counter_registry: crate::counter::Registry,
    ) -> Self {
        Self {
            id,
            budgets,
            total_sender_ids,
            dead_peer_cooldown,
            clock,
            counter_registry,
            frame_dispatch: None,
            send_worker: None,
            send_sockets: Vec::new(),
            recv_socket: None,
            recv_dispatch: None,
            waker_drain: None,
            background: None,
        }
    }

    #[inline]
    fn spawn<R: crate::runtime::Runtime>(self, runtime: &R) {
        use crate::{runtime::Spawner as _, socket::channel::ReceiverExt as _};

        let Self {
            id,
            budgets,
            total_sender_ids,
            dead_peer_cooldown,
            clock,
            counter_registry,
            frame_dispatch,
            send_worker,
            send_sockets,
            recv_socket,
            recv_dispatch,
            waker_drain,
            background,
        } = self;

        runtime.spawn_local(id, move |mut local| {
            if let Some(fd) = frame_dispatch {
                tasks::frame_dispatch(
                    &mut local,
                    fd.frame_rx,
                    fd.socket_senders,
                    crate::xorshift::Rng::new(),
                    fd.clock,
                    fd.overall_send_rate,
                    budgets,
                    counter_registry.clone(),
                );
            }

            if let Some(sw) = send_worker {
                let batch_rx = GaugedQueueReceiver::new(
                    sw.batch_rx,
                    sw.batch_gauge
                        .receiver("task.context_resolver")
                        .with_function("endpoint::Worker::spawn"),
                );
                let ack_rx = GaugedQueueReceiver::new(
                    sw.ack_rx,
                    sw.ack_gauge
                        .receiver("task.ack_processor")
                        .with_function("endpoint::Worker::spawn"),
                );
                tasks::send_worker(
                    &mut local,
                    sw.idx,
                    batch_rx,
                    ack_rx,
                    sw.invalidation_rx,
                    total_sender_ids,
                    send_sockets.into(),
                    clock.clone(),
                    sw.random,
                    sw.frame_tx,
                    sw.ack_completions_tx,
                    sw.waker_sink,
                    sw.peer_dead_tx,
                    dead_peer_cooldown,
                    budgets,
                    counter_registry.clone(),
                );
            }

            if let Some(rs) = recv_socket {
                let recv_idx = rs.idx;
                let variant = format!("recv.{recv_idx}");
                let rx = tasks::socket_recv(rs.socket, rs.recv_pool, rs.router);
                let task_counter = counter_registry
                    .register_nominal_task("task.socket_recv", &variant)
                    .with_registration_metadata(
                        "task.socket_recv",
                        "Reads UDP datagrams and routes packets into dispatch queues",
                        "endpoint::Worker::spawn",
                    );
                local.spawn_receiver_task(
                    rx.drain_budgeted_metered(Some(budgets.socket_recv), task_counter.clone()),
                    Some(budgets.socket_recv),
                    task_counter,
                );
            }

            if let Some(rd) = recv_dispatch {
                let packet_rx = GaugedQueueReceiver::new(
                    rd.packet_rx,
                    rd.packet_gauge
                        .receiver("task.packet_dispatch")
                        .with_function("endpoint::Worker::spawn"),
                );
                let recv_dispatch_idx = rd.recv_dispatch_idx;
                let recv_cache = std::rc::Rc::new(std::cell::RefCell::new(
                    crate::stream::endpoint::recv::Cache::new(recv_dispatch_idx),
                ));
                let (ack_burst_tx, ack_burst_rx) =
                    crate::socket::channel::intrusive::unsync::new_with_adapter::<
                        crate::stream::endpoint::recv::AckBurstAdapter,
                    >();

                // Recv idle wheel — expires inactive recv contexts.
                let variant = format!("recv.dispatch.{recv_dispatch_idx}");
                let q_recv_idle_wheel =
                    counter_registry.register_queue_gauge_nominal("q.idle_wheel", &variant);
                let (recv_idle_wheel_tx, recv_idle_wheel_rx) =
                    crate::socket::channel::intrusive::unsync::new_with_adapter::<
                        crate::stream::endpoint::recv::IdleWheelAdapter,
                    >();
                {
                    let recv_cache = recv_cache.clone();
                    let clock = rd.clock.clone();
                    let idle_expired = counter_registry.register("idle.recv.expired");
                    let idle_rescheduled = counter_registry.register("idle.recv.rescheduled");
                    let idle_lifetime =
                        counter_registry.register_nominal_timer("idle.recv.lifetime", &variant);
                    let task_counter = counter_registry
                        .register_nominal_task("task.recv_idle_wheel", &variant)
                        .with_registration_metadata(
                            "task.recv_idle_wheel",
                            "Expires or re-schedules idle recv contexts",
                            "endpoint::Worker::spawn",
                        );
                    local.spawn_receiver_task(
                        tasks::recv_idle_wheel_drain(
                            recv_idle_wheel_rx,
                            recv_idle_wheel_tx.clone(),
                            clock,
                            q_recv_idle_wheel,
                            recv_cache,
                            rd.peer_dead_tx.clone(),
                            dead_peer_cooldown,
                            idle_expired,
                            idle_rescheduled,
                            idle_lifetime,
                            budgets.idle_wheel,
                            task_counter.clone(),
                        ),
                        Some(budgets.idle_wheel),
                        task_counter,
                    );
                }

                let rx = tasks::packet_dispatch(
                    packet_rx,
                    recv_cache.clone(),
                    ack_burst_tx,
                    recv_idle_wheel_tx,
                    rd.path_secret_map,
                    rd.acceptor_registry,
                    rd.frame_tx,
                    rd.ack_sender.clone(),
                    rd.queue_dispatcher,
                    rd.counters.clone(),
                    rd.clock,
                    rd.route,
                    rd.waker_sink,
                    rd.ups_tx,
                );
                let variant = format!("recv.dispatch.{recv_dispatch_idx}");
                let task_counter = counter_registry
                    .register_nominal_task("task.packet_dispatch", &variant)
                    .with_registration_metadata(
                        "task.packet_dispatch",
                        "Decrypts, validates, and routes inbound packets",
                        "endpoint::Worker::spawn",
                    );
                local.spawn_receiver_task(
                    rx.drain_budgeted_metered(Some(budgets.packet_dispatch), task_counter.clone()),
                    Some(budgets.packet_dispatch),
                    task_counter,
                );
                let rx = tasks::ack_burst(
                    crate::socket::channel::FlattenList::new(ack_burst_rx.into_list_receiver()),
                    rd.ack_sender.clone(),
                    recv_dispatch_idx,
                    rd.counters.clone(),
                );
                let task_counter = counter_registry
                    .register_nominal_task("task.ack_burst", &variant)
                    .with_registration_metadata(
                        "task.ack_burst",
                        "Encodes and submits ACK bursts from recv contexts",
                        "endpoint::Worker::spawn",
                    );
                local.spawn_receiver_task(
                    rx.drain_budgeted_metered(Some(budgets.ack_burst), task_counter.clone()),
                    Some(budgets.ack_burst),
                    task_counter,
                );
                let ack_completion_gauge =
                    counter_registry.register_queue_gauge_nominal("q.dispatch", &variant);
                let ack_completion_rx = crate::counter::GaugedReceiver::new(
                    rd.ack_completion_rx,
                    ack_completion_gauge
                        .receiver("task.ack_completion")
                        .with_function("endpoint::Worker::spawn"),
                );
                let rx = tasks::ack_completion(
                    ack_completion_rx,
                    recv_cache.clone(),
                    rd.ack_sender,
                    rd.counters.clone(),
                );
                let task_counter = counter_registry
                    .register_nominal_task("task.ack_completion", &variant)
                    .with_registration_metadata(
                        "task.ack_completion",
                        "Finalizes ACK send completions and retries stale acknowledgements",
                        "endpoint::Worker::spawn",
                    );
                local.spawn_receiver_task(
                    rx.drain_budgeted_metered(Some(budgets.ack_completion), task_counter.clone()),
                    Some(budgets.ack_completion),
                    task_counter,
                );

                let rx = tasks::recv_invalidation(rd.invalidation_rx, recv_cache);
                let task_counter = counter_registry
                    .register_nominal_task("task.invalidation", &variant)
                    .with_registration_metadata(
                        "task.invalidation",
                        "Purges invalidated recv contexts",
                        "endpoint::Worker::spawn",
                    );
                local.spawn_receiver_task(
                    rx.drain_budgeted_metered(Some(budgets.invalidation), task_counter.clone()),
                    Some(budgets.invalidation),
                    task_counter,
                );
            }

            if let Some(drain) = waker_drain {
                let variant = format!("waker.{id}");
                let rx = tasks::waker_drain(drain);
                let task_counter = counter_registry
                    .register_nominal_task("task.waker_drain", &variant)
                    .with_registration_metadata(
                        "task.waker_drain",
                        "Invokes deferred wakers enqueued by dispatch workers",
                        "endpoint::Worker::spawn",
                    );
                local.spawn_receiver_task(
                    rx.drain_budgeted_metered(Some(budgets.waker_drain), task_counter.clone()),
                    Some(budgets.waker_drain),
                    task_counter,
                );
            }

            if let Some(bg) = background {
                let invalidation_counters = tasks::ValidatorInvalidationCounters {
                    unknown_path_secret_validated: counter_registry
                        .register("invalidation.validator.ups.validated"),
                    stale_key_validated: counter_registry
                        .register("invalidation.validator.stale_key.validated"),
                    replay_detected_validated: counter_registry
                        .register("invalidation.validator.replay_detected.validated"),
                };
                let rx = tasks::invalidation_validator(
                    bg.raw_rx,
                    bg.path_secret_map,
                    bg.send_txs,
                    bg.recv_txs,
                    bg.sender_id_to_worker,
                    invalidation_counters,
                );
                let task_counter = counter_registry
                    .register_nominal_task("task.invalidation_validator", "background")
                    .with_registration_metadata(
                        "task.invalidation_validator",
                        "Validates invalidation datagrams and fan-outs revocation events",
                        "endpoint::Worker::spawn",
                    );
                local.spawn_receiver_task(
                    rx.drain_budgeted_metered(None, task_counter.clone()),
                    None,
                    task_counter,
                );

                let peer_dead_counters = tasks::PeerDeadCounters {
                    events: counter_registry.register("peer_dead.events"),
                    broadcasted: counter_registry.register("peer_dead.broadcasted"),
                };
                let rx = tasks::peer_dead_broadcast(
                    bg.peer_dead_rx,
                    bg.queue_dispatcher,
                    bg.waker_sink,
                    peer_dead_counters,
                );
                let task_counter = counter_registry
                    .register_nominal_task("task.peer_dead_broadcast", "background")
                    .with_registration_metadata(
                        "task.peer_dead_broadcast",
                        "Marks peers dead and performs credential-wide reset fanout",
                        "endpoint::Worker::spawn",
                    );
                local.spawn_receiver_task(
                    rx.drain_budgeted_metered(None, task_counter.clone()),
                    None,
                    task_counter,
                );

                let ups_counters = ups::Counters::new(&counter_registry);
                let ups_rx = crate::counter::GaugedReceiver::new(
                    bg.ups_rx,
                    bg.ups_queue_gauge
                        .receiver("task.ups_send")
                        .with_function("endpoint::Worker::spawn"),
                );
                let rx = tasks::ups_send(
                    ups_rx,
                    bg.ups_socket,
                    clock.clone(),
                    bg.ups_rate,
                    bg.ups_dedup_capacity,
                    bg.ups_dedup_window,
                    ups_counters,
                );
                let task_counter =
                    counter_registry.register_nominal_task("task.ups_send", "background");
                local.spawn(rx.drain_budgeted_metered(None, task_counter));

                local.spawn(bg.acceptor_cleaner);
            }
        });
    }
}

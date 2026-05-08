// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Common pipeline components for send and receive paths.
//!
//! Both the client and server need symmetric send/receive capabilities:
//! - Client: sends data datagrams, receives ACK control packets
//! - Server: receives data datagrams, sends ACK control packets
//!
//! This module provides shared pipeline building blocks.

pub use s2n_quic_platform::features::Gso;

use crate::{
    acceptor,
    busy_poll::clock::Timer as BusyPollClock,
    clock::{precision::Clock as _, tokio::Clock as TokioClock, wheel::Wheel},
    congestion,
    credentials::{self, Credentials},
    datagram::batch::{Batch, Priority as BatchPriority},
    flow::{self, queue},
    intrusive_queue::{Entry, Queue},
    packet::{
        self,
        datagram::{partial::PartialDatagram, QueuePair, ResetTarget, RoutingInfo},
    },
    path::{self, secret::map::Entry as PathSecretEntry},
    random,
    socket::{
        self,
        channel::{
            self, intrusive_queue, FlattenSegments, InspectErr, Map, Paced,
            ReceiverExt, RouterAdapter, SocketReceiver, UnboundedSender,
        },
        pool::{self, descriptor},
        rate::Rate,
        recv::router::Router,
    },
    stream::socket::{BusyPoll, Gso as GsoSocket, Options, ReusePort},
    stream2::LocalSpawner as _,
};
use bytes::{Bytes, BytesMut};
use core::time::Duration;
use s2n_codec::{Encoder as _, EncoderBuffer};
use s2n_quic_core::{
    frame,
    frame::ack::EcnCounts,
    packet::number::{PacketNumberRange, PacketNumberSpace},
    varint::VarInt,
};
use s2n_quic_platform::features;
use std::{
    cell::RefCell,
    collections::{hash_map, HashMap},
    io,
    net::SocketAddr,
    rc::Rc,
    sync::{
        atomic::{AtomicI64, AtomicU64, Ordering},
        Arc, Mutex,
    },
};
use tracing::info;

// ── Flow Reset Error Codes ─────────────────────────────────────────────────

/// Error codes for FlowReset packets
pub(crate) mod reset_error {
    use s2n_quic_core::varint::VarInt;
    use std::fmt;

    /// Stream ID was reused before the previous flow completed
    pub const STREAM_ID_ERROR: VarInt = VarInt::from_u32(1);

    /// The acceptor ID specified in FlowInit was not found
    pub const ACCEPTOR_NOT_FOUND: VarInt = VarInt::from_u32(2);

    /// The queue state became stale or inconsistent during validation
    pub const STALE_STATE: VarInt = VarInt::from_u32(3);

    /// Failed to decode control frames
    pub const FRAME_DECODE_ERROR: VarInt = VarInt::from_u32(4);

    /// The sender terminated abnormally (e.g., panic, crash)
    pub const ABNORMAL_TERMINATION: VarInt = VarInt::from_u32(5);

    /// The receiver no longer wants to receive data
    pub const STOP_SENDING: VarInt = VarInt::from_u32(6);

    /// Retransmissions exhausted after repeated transmission failures
    pub const RETRANSMISSIONS_EXHAUSTED: VarInt = VarInt::from_u32(7);

    /// Server accept queue overflowed - stream was dropped before the application could handle it
    pub const SERVER_BUSY: VarInt = VarInt::from_u32(8);

    /// Reset error codes for stream resets
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum ResetError {
        /// Stream ID was reused before the previous flow completed
        StreamIdError,
        /// The acceptor ID specified in FlowInit was not found
        AcceptorNotFound,
        /// The queue state became stale or inconsistent during validation
        StaleState,
        /// Failed to decode control frames
        FrameDecodeError,
        /// The sender terminated abnormally (e.g., panic, crash)
        AbnormalTermination,
        /// The receiver no longer wants to receive data
        StopSending,
        /// Retransmissions exhausted after repeated transmission failures
        RetransmissionsExhausted,
        /// Server accept queue overflowed
        ServerBusy,
        /// Unknown error code
        Unknown(VarInt),
    }

    impl ResetError {
        /// Convert to VarInt error code
        pub fn as_varint(self) -> VarInt {
            match self {
                Self::StreamIdError => STREAM_ID_ERROR,
                Self::AcceptorNotFound => ACCEPTOR_NOT_FOUND,
                Self::StaleState => STALE_STATE,
                Self::FrameDecodeError => FRAME_DECODE_ERROR,
                Self::AbnormalTermination => ABNORMAL_TERMINATION,
                Self::StopSending => STOP_SENDING,
                Self::RetransmissionsExhausted => RETRANSMISSIONS_EXHAUSTED,
                Self::ServerBusy => SERVER_BUSY,
                Self::Unknown(code) => code,
            }
        }
    }

    impl From<VarInt> for ResetError {
        fn from(code: VarInt) -> Self {
            match code {
                STREAM_ID_ERROR => Self::StreamIdError,
                ACCEPTOR_NOT_FOUND => Self::AcceptorNotFound,
                STALE_STATE => Self::StaleState,
                FRAME_DECODE_ERROR => Self::FrameDecodeError,
                ABNORMAL_TERMINATION => Self::AbnormalTermination,
                STOP_SENDING => Self::StopSending,
                RETRANSMISSIONS_EXHAUSTED => Self::RetransmissionsExhausted,
                SERVER_BUSY => Self::ServerBusy,
                _ => Self::Unknown(code),
            }
        }
    }

    impl fmt::Display for ResetError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                Self::StreamIdError => {
                    write!(
                        f,
                        "STREAM_ID_ERROR: stream ID reused before previous flow completed"
                    )
                }
                Self::AcceptorNotFound => {
                    write!(f, "ACCEPTOR_NOT_FOUND: acceptor ID not found")
                }
                Self::StaleState => {
                    write!(f, "STALE_STATE: queue state became stale or inconsistent")
                }
                Self::FrameDecodeError => {
                    write!(f, "FRAME_DECODE_ERROR: failed to decode control frames")
                }
                Self::AbnormalTermination => {
                    write!(
                        f,
                        "ABNORMAL_TERMINATION: sender terminated abnormally (panic/crash)"
                    )
                }
                Self::StopSending => {
                    write!(f, "STOP_SENDING: receiver no longer wants to receive data")
                }
                Self::RetransmissionsExhausted => {
                    write!(f, "RETRANSMISSIONS_EXHAUSTED: retransmissions exhausted after repeated transmission failures")
                }
                Self::ServerBusy => {
                    write!(f, "SERVER_BUSY: server accept queue overflowed")
                }
                Self::Unknown(code) => {
                    write!(f, "UNKNOWN({}): unknown reset error code", code.as_u64())
                }
            }
        }
    }

    impl std::error::Error for ResetError {}
}

// ── Worker-Socket Channel ──────────────────────────────────────────────────

/// A specialized channel for distributing batches to sockets within a worker.
///
/// This uses a single sync channel per worker that feeds multiple unsync channels
/// per socket, minimizing lock contention. The sender locks once to push to the
/// appropriate socket queue, and the worker-local receiver locks once to swap out
/// all queues for local dispatch.
mod worker_socket_channel {
    use crate::{datagram::batch::Batch, intrusive_queue::Queue};
    use std::sync::{Arc, Mutex};

    /// Shared state for a worker's socket queues
    struct WorkerQueues {
        /// One queue per socket on this worker
        queues: Mutex<Vec<Queue<Batch>>>,
    }

    /// Sender that targets a specific socket within a worker
    #[derive(Clone)]
    pub struct Sender {
        /// Index of the target socket within the worker
        socket_idx: usize,
        /// Shared queues for this worker
        queues: Arc<WorkerQueues>,
    }

    impl Sender {
        /// Send a batch to the target socket queue
        pub fn send_entry(&self, entry: crate::intrusive_queue::Entry<Batch>) {
            let mut queues = self.queues.queues.lock().unwrap();
            queues[self.socket_idx].push_back(entry);
        }
    }

    impl crate::socket::channel::Sender<crate::intrusive_queue::Entry<Batch>> for Sender {
        fn poll_send(
            &mut self,
            _cx: &mut core::task::Context<'_>,
            value: &mut core::mem::MaybeUninit<crate::intrusive_queue::Entry<Batch>>,
        ) -> core::task::Poll<Result<(), ()>> {
            // SAFETY: We take ownership and replace with uninitialized memory
            let entry = unsafe { value.as_ptr().read() };
            self.send_entry(entry);
            core::task::Poll::Ready(Ok(()))
        }
    }

    /// Receiver that collects from all socket queues for this worker
    #[derive(Clone)]
    pub struct Receiver {
        /// Shared queues for this worker
        queues: Arc<WorkerQueues>,
        /// Number of sockets
        num_sockets: usize,
    }

    impl Receiver {
        /// Drain all socket queues and send to their respective local channels
        ///
        /// This performs a single lock to grab all pending batches for all sockets,
        /// then dispatches them to the provided unsync senders inline.
        pub fn drain_to<S>(&self, senders: &mut [S])
        where
            S: crate::socket::channel::UnboundedSender<Queue<Batch>>,
        {
            debug_assert_eq!(senders.len(), self.num_sockets);

            let mut queues = self.queues.queues.lock().unwrap();
            for (queue, sender) in queues.iter_mut().zip(senders.iter_mut()) {
                if !queue.is_empty() {
                    let mut swapped = Queue::new();
                    core::mem::swap(queue, &mut swapped);
                    let _ = sender.send(swapped);
                }
            }
        }
    }

    /// Create a worker-socket channel with the given number of sockets
    ///
    /// Returns (senders, receiver) where senders[i] sends to socket i
    pub fn new(num_sockets: usize) -> (Vec<Sender>, Receiver) {
        let queues = Arc::new(WorkerQueues {
            queues: Mutex::new((0..num_sockets).map(|_| Queue::new()).collect()),
        });

        let senders = (0..num_sockets)
            .map(|socket_idx| Sender {
                socket_idx,
                queues: queues.clone(),
            })
            .collect();

        let receiver = Receiver {
            queues,
            num_sockets,
        };

        (senders, receiver)
    }
}

// ── Instrumentation ────────────────────────────────────────────────────────

/// Shared counter registry for tracking pipeline metrics
#[derive(Clone, Default)]
pub struct CounterRegistry {
    metrics: Arc<Mutex<HashMap<&'static str, Metric>>>,
}

#[derive(Clone)]
enum Metric {
    Counter(Arc<AtomicU64>),
    Gauge(Arc<AtomicI64>),
    Queue {
        enqueue: Arc<AtomicU64>,
        drain: Arc<AtomicU64>,
        depth: Arc<AtomicI64>,
    },
}

impl Metric {
    fn format(&self, label: &'static str) -> Option<String> {
        match self {
            Metric::Counter(v) => {
                let count = v.swap(0, Ordering::Relaxed);
                if count == 0 {
                    return None;
                }
                if label.ends_with(":bytes") {
                    let mut rate = count as f64 * 8.0;
                    let prefixes = [("G", 1e9), ("M", 1e6), ("K", 1e3)];
                    let mut prefix = "";
                    for (p, divisor) in prefixes {
                        if rate >= divisor {
                            rate /= divisor;
                            prefix = p;
                            break;
                        }
                    }
                    let label_without_suffix = label.trim_end_matches(":bytes");
                    Some(format!("{}={:.2}{}bps", label_without_suffix, rate, prefix))
                } else {
                    Some(format!("{}={}", label, count))
                }
            }
            Metric::Gauge(v) => {
                let depth = v.load(Ordering::Relaxed);
                if depth == 0 {
                    return None;
                }
                Some(format!("{}={}", label, depth))
            }
            Metric::Queue {
                enqueue,
                drain,
                depth,
            } => {
                let enq = enqueue.swap(0, Ordering::Relaxed);
                let drn = drain.swap(0, Ordering::Relaxed);
                let dep = depth.load(Ordering::Relaxed);
                if enq == 0 && drn == 0 && dep == 0 {
                    return None;
                }
                if dep == 0 {
                    Some(format!("{label}={enq}/{drn}"))
                } else {
                    Some(format!("{label}={enq}/{drn}({dep})"))
                }
            }
        }
    }
}

impl CounterRegistry {
    pub fn new() -> Self {
        Self {
            metrics: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Register a counter with the given label, returning a handle to increment it
    pub fn register(&self, label: &'static str) -> Counter {
        let mut metrics = self.metrics.lock().unwrap();
        let inner = match metrics.entry(label) {
            std::collections::hash_map::Entry::Occupied(e) => match e.get() {
                Metric::Counter(v) => v.clone(),
                _ => panic!("label {label:?} already registered as a different metric type"),
            },
            std::collections::hash_map::Entry::Vacant(e) => {
                let v = Arc::new(AtomicU64::new(0));
                e.insert(Metric::Counter(v.clone()));
                v
            }
        };
        Counter::new(inner)
    }

    /// Register a queue gauge that tracks both throughput and current depth.
    ///
    /// Register a queue metric that tracks enqueue rate, drain rate, and depth.
    ///
    /// Formats as `label=enqueue/drain(depth)` in log output. When depth is 0,
    /// the parenthetical is omitted: `label=enqueue/drain`.
    pub fn register_queue_gauge(&self, label: &'static str) -> QueueGauge {
        let mut metrics = self.metrics.lock().unwrap();
        match metrics.entry(label) {
            std::collections::hash_map::Entry::Occupied(e) => match e.get() {
                Metric::Queue {
                    enqueue,
                    drain,
                    depth,
                } => QueueGauge {
                    throughput: Counter::new(enqueue.clone()),
                    drain: Counter::new(drain.clone()),
                    depth: Gauge(depth.clone()),
                },
                _ => panic!("label {label:?} already registered as a different metric type"),
            },
            std::collections::hash_map::Entry::Vacant(e) => {
                let enqueue = Arc::new(AtomicU64::new(0));
                let drain = Arc::new(AtomicU64::new(0));
                let depth = Arc::new(AtomicI64::new(0));
                e.insert(Metric::Queue {
                    enqueue: enqueue.clone(),
                    drain: drain.clone(),
                    depth: depth.clone(),
                });
                QueueGauge {
                    throughput: Counter::new(enqueue),
                    drain: Counter::new(drain),
                    depth: Gauge(depth),
                }
            }
        }
    }

    /// Register a gauge with the given label, returning a handle to add/sub.
    ///
    /// Gauges track a current value that is never reset by the reporter.
    pub fn register_gauge(&self, label: &'static str) -> Gauge {
        let mut metrics = self.metrics.lock().unwrap();
        let inner = match metrics.entry(label) {
            std::collections::hash_map::Entry::Occupied(e) => match e.get() {
                Metric::Gauge(v) => v.clone(),
                _ => panic!("label {label:?} already registered as a different metric type"),
            },
            std::collections::hash_map::Entry::Vacant(e) => {
                let v = Arc::new(AtomicI64::new(0));
                e.insert(Metric::Gauge(v.clone()));
                v
            }
        };
        Gauge(inner)
    }

    /// Spawn a task that periodically logs all metrics in a single sorted line
    pub fn spawn_reporter(&self, interval: Duration) {
        let metrics = self.metrics.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(interval);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            loop {
                interval.tick().await;

                let metrics = metrics.lock().unwrap();
                if metrics.is_empty() {
                    continue;
                }

                let mut labels: Vec<&'static str> = metrics.keys().copied().collect();
                labels.sort();

                let parts: Vec<String> = labels
                    .into_iter()
                    .filter_map(|label| metrics[label].format(label))
                    .collect();

                if !parts.is_empty() {
                    tracing::info!("{}", parts.join(" "));
                }
            }
        });
    }
}

#[derive(Clone)]
pub struct Counter(Arc<AtomicU64>);

impl Counter {
    #[inline]
    pub fn new(inner: Arc<AtomicU64>) -> Self {
        Self(inner)
    }
}

impl Counter {
    #[inline]
    pub fn add(&self, v: u64) {
        self.0.fetch_add(v, Ordering::Relaxed);
    }
}

impl core::ops::AddAssign<u64> for Counter {
    #[inline]
    fn add_assign(&mut self, rhs: u64) {
        self.add(rhs);
    }
}

#[derive(Clone)]
pub struct Gauge(Arc<AtomicI64>);

impl Gauge {
    #[inline]
    pub fn add(&self, v: i64) {
        self.0.fetch_add(v, Ordering::Relaxed);
    }

    #[inline]
    pub fn sub(&self, v: i64) {
        self.0.fetch_sub(v, Ordering::Relaxed);
    }
}

/// Tracks enqueue rate, dequeue rate, and current depth for a queue.
#[derive(Clone)]
pub struct QueueGauge {
    pub throughput: Counter,
    pub drain: Counter,
    pub depth: Gauge,
}

impl QueueGauge {
    #[inline]
    pub fn enqueue(&self, count: u64) {
        self.throughput.add(count);
        self.depth.add(count as i64);
    }

    #[inline]
    pub fn dequeue(&self) {
        self.drain.add(1);
        self.depth.sub(1);
    }
}

/// Like `FlattenQueue`, but tracks throughput and current queue depth via a `QueueGauge`.
pub struct GaugedQueue<T, R> {
    inner: R,
    queue: crate::intrusive_queue::Queue<T>,
    gauge: QueueGauge,
}

impl<T, R> GaugedQueue<T, R> {
    pub fn new(inner: R, gauge: QueueGauge) -> Self {
        Self {
            inner,
            queue: Default::default(),
            gauge,
        }
    }
}

impl<T, R> channel::Receiver<crate::intrusive_queue::Entry<T>> for GaugedQueue<T, R>
where
    R: channel::Receiver<crate::intrusive_queue::Queue<T>>,
{
    fn poll_recv(
        &mut self,
        cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Option<crate::intrusive_queue::Entry<T>>> {
        loop {
            if let Some(entry) = self.queue.pop_front() {
                self.gauge.dequeue();
                return core::task::Poll::Ready(Some(entry));
            }

            match self.inner.poll_recv(cx) {
                core::task::Poll::Ready(Some(queue)) => {
                    if queue.is_empty() {
                        cx.waker().wake_by_ref();
                        return core::task::Poll::Pending;
                    }
                    self.gauge.enqueue(queue.len() as u64);
                    self.queue = queue;
                }
                core::task::Poll::Ready(None) => return core::task::Poll::Ready(None),
                core::task::Poll::Pending => return core::task::Poll::Pending,
            }
        }
    }

    fn on_consumed(&mut self, bytes: u64) {
        self.inner.on_consumed(bytes);
    }
}

// ── Testing Helpers ────────────────────────────────────────────────────────

/// Fast consistent hash for routing packets by credentials and sender
///
/// Combines credentials.id (good entropy) with source_sender_id using a mixing function.
/// Uses source_sender_id instead of key_id because it's stable across key rotations.
#[inline]
fn hash_credentials_and_sender(credentials: &Credentials, source_sender_id: VarInt) -> u64 {
    hash_id_and_sender(&credentials.id, source_sender_id)
}

#[inline]
fn hash_id_and_sender(id: &credentials::Id, sender_id: VarInt) -> u64 {
    let mut hash = id.to_hash();

    let sender_id = sender_id.as_u64();
    hash ^= sender_id.wrapping_mul(0x9e3779b97f4a7c15); // Golden ratio
    hash = hash.rotate_left(32) ^ sender_id;
    hash = hash.wrapping_mul(0x517cc1b727220a95);

    hash
}

// ── Sender Routing ────────────────────────────────────────────────────────

trait SenderRoute: Clone + Copy + Send + 'static {
    fn new(count: usize) -> Self;
    fn route(&self, hash: u64) -> usize;

    #[inline]
    fn sender_id(&self, credentials_id: &credentials::Id, source_sender_id: VarInt) -> VarInt {
        let hash = hash_id_and_sender(credentials_id, source_sender_id);
        unsafe { VarInt::new_unchecked(self.route(hash) as u64) }
    }

    #[inline]
    fn worker_id(&self, credentials: &Credentials, source_sender_id: VarInt) -> usize {
        let hash = hash_credentials_and_sender(credentials, source_sender_id);
        self.route(hash)
    }
}

#[derive(Clone, Copy)]
struct PowerOfTwoRoute {
    mask: u64,
}

impl SenderRoute for PowerOfTwoRoute {
    fn new(count: usize) -> Self {
        debug_assert!(count.is_power_of_two());
        Self {
            mask: (count - 1) as u64,
        }
    }

    #[inline]
    fn route(&self, hash: u64) -> usize {
        (hash & self.mask) as usize
    }
}

#[derive(Clone, Copy)]
struct ModuloRoute {
    divisor: u64,
}

impl SenderRoute for ModuloRoute {
    fn new(count: usize) -> Self {
        Self {
            divisor: count as u64,
        }
    }

    #[inline]
    fn route(&self, hash: u64) -> usize {
        (hash % self.divisor) as usize
    }
}

// ── PTO Probe Generation ───────────────────────────────────────────────────

/// Generate a PING control packet for PTO probe
///
/// TODO: Piggyback any pending ACK frames with the PING to save a separate control
/// packet transmission. This would require:
/// - Looking up the peer state from the shared_sender_cache by credentials
/// - Checking if sender_state.should_transmit()
/// - Encoding both the ACK and PING frames into the same control packet
/// - Marking the ACK as transmitted in the peer state
fn generate_pto_probe(
    context: &socket::channel::PathContext<crate::crypto::awslc::seal::Application>,
) -> PartialDatagram {
    use s2n_quic_core::frame;

    // PING frame
    let control_data = const { Bytes::from_static(const { &[frame::Ping.tag()] }) }.into();

    // Create control packet with PING
    PartialDatagram::new_control(
        packet::control::RoutingInfo::None,
        control_data,
        context.path_secret_entry.clone(),
    )
}

type RcPathContext =
    Rc<RefCell<socket::channel::PathContext<crate::crypto::awslc::seal::Application>>>;

/// Process a PTO wheel timeout for a path context
///
/// Returns Some(batch) if a probe should be sent, None otherwise.
/// Reinserts into the wheel if the context still needs scheduling.
fn process_pto_timeout<Clk, S>(
    worker_id: usize,
    context_rc: RcPathContext,
    clock: &Clk,
    pto_wheel_tx: &mut S,
) -> Option<Entry<Batch>>
where
    Clk: crate::clock::precision::Clock + ?Sized,
    S: UnboundedSender<RcPathContext>,
{
    let mut context_ref = context_rc.borrow_mut();
    let context = &mut *context_ref;
    let has_inflight = socket::channel::Pto::has_inflight_packets(&context.packet_number_map);

    // Check if we should actually send a probe
    let should_send_probe = context.pto.on_timeout(has_inflight);

    // Extract data address for probe generation if needed
    // Control packets (including PTO probes) go to the data port, not handshake port
    let data_addr = context.path_secret_entry.data_addr();

    let probe_batch = if should_send_probe {
        tracing::debug!(
            worker_id,
            credentials_id = ?context.credentials.id,
            backoff = context.pto.backoff,
            inflight = context.packet_number_map.iter().count(),
            "PTO timeout - sending probe"
        );

        // Generate probe datagram (immutable borrow of context)
        let probe_datagram = generate_pto_probe(&*context);

        // Create a batch for the probe
        let mut batch = Batch::new(None, data_addr);
        batch.push(probe_datagram.into());

        Some(Entry::new(batch))
    } else {
        None
    };

    // Reinsert if still has inflight packets (needs rescheduling)
    if has_inflight {
        context.pto.update_target(clock, &context.rtt_estimator);
        drop(context_ref); // Release borrow before sending
        let _ = pto_wheel_tx.send(context_rc);
    }

    probe_batch
}

// ── Control Packet Processing ──────────────────────────────────────────────

/// Process control frames in a control packet and update send state
fn process_control_frames<Clk, Rand>(
    worker_id: usize,
    packet: &mut Entry<packet::control::decoder::Packet<descriptor::Filled>>,
    context: &mut socket::channel::PathContext<crate::crypto::awslc::seal::Application>,
    acked: &mut impl channel::UnboundedSender<Queue<PartialDatagram>>,
    lost: &mut impl channel::UnboundedSender<Queue<PartialDatagram>>,
    clock: &Clk,
    random: &mut Rand,
) where
    Clk: s2n_quic_core::time::Clock + ?Sized,
    Rand: random::Generator,
{
    let now = clock.get_time();

    // Track ACK processing state
    let mut max_acked_pn = None;
    let mut max_acked_tx_time = None;
    let mut bytes_acked = 0;
    let mut cca_args = None;
    let mut ack_delay = Duration::MAX;

    // Process all ACK frames in the control packet
    for frame in packet.control_frames_mut() {
        let Ok(frame) = frame else {
            tracing::warn!(worker_id, "Failed to decode control frame");
            continue;
        };

        match frame {
            frame::Frame::Padding(_) => {
                // Padding frames are ignored
            }
            frame::Frame::Ping(_) => {
                // PING frames are implicitly ACKed by their reception
                // No further action needed here
            }
            frame::Frame::Ack(ack) => {
                // Process ACK ranges - remove ACKed packets and track metadata
                ack_delay = ack_delay.min(ack.ack_delay());
                process_ack_ranges(
                    &ack,
                    &mut context.packet_number_map,
                    &mut max_acked_pn,
                    &mut max_acked_tx_time,
                    &mut bytes_acked,
                    &mut cca_args,
                    acked,
                );
            }
            frame => {
                tracing::warn!(worker_id, ?frame, "Unexpected control frame type")
            }
        }
    }

    // Update RTT estimator with ACK information
    if let Some((time_sent, cc_info)) = cca_args {
        let rtt_sample = now
            .saturating_duration_since(time_sent)
            .saturating_sub(ack_delay)
            .max(Duration::from_micros(1));

        context.rtt_estimator.update_rtt(
            Duration::ZERO,
            rtt_sample,
            now,
            true,
            PacketNumberSpace::ApplicationData,
        );

        context.cca.on_packet_ack(
            cc_info.first_sent_time,
            bytes_acked,
            cc_info,
            &context.rtt_estimator,
            random,
            now,
        );
    }

    // Perform loss detection if we ACKed any packets
    if let Some(max_acked_pn) = max_acked_pn {
        if let Some(max_tx_time) = max_acked_tx_time {
            tracing::trace!(
                worker_id,
                max_acked = max_acked_pn.as_u64(),
                bytes_acked,
                ack_delay_us = ack_delay.as_micros(),
                "Processing ACK frame"
            );
            detect_and_retransmit_lost_packets(
                context,
                max_acked_pn,
                max_tx_time,
                lost,
                now,
                random,
            );
        }
    }

    // Update PTO state after ACK processing
    let has_remaining_inflight =
        socket::channel::Pto::has_inflight_packets(&context.packet_number_map);
    context.pto.on_ack_received(has_remaining_inflight);
}

/// Process ACK ranges and remove ACKed packets from the packet number map
fn process_ack_ranges(
    ack: &frame::Ack<impl frame::ack::AckRanges>,
    packet_number_map: &mut s2n_quic_core::packet::number::Map<Entry<PartialDatagram>>,
    max_acked_pn: &mut Option<VarInt>,
    max_acked_tx_time: &mut Option<s2n_quic_core::time::Timestamp>,
    bytes_acked: &mut usize,
    cca_args: &mut Option<(s2n_quic_core::time::Timestamp, congestion::PacketInfo)>,
    acked: &mut impl channel::UnboundedSender<Queue<PartialDatagram>>,
) {
    // Process each ACK range
    let mut queue = Queue::new();
    for range in ack.ack_ranges() {
        let pmin = PacketNumberSpace::Initial.new_packet_number(*range.start());
        let pmax = PacketNumberSpace::Initial.new_packet_number(*range.end());
        let range = PacketNumberRange::new(pmin, pmax);

        // Remove ACKed packets from the packet number map
        let mut queue_range = Queue::new();
        for (num, mut entry) in packet_number_map.remove_range(range) {
            let num_varint = unsafe { VarInt::new_unchecked(num.as_u64()) };
            *max_acked_pn = (*max_acked_pn).max(Some(num_varint));

            // Extract transmission metadata
            if let Some(tx_info) = entry.transmission_info.take() {
                let time_sent = tx_info.time_sent;
                *max_acked_tx_time = (*max_acked_tx_time).max(Some(time_sent));

                // Track CCA info from most recent packet
                if cca_args
                    .as_ref()
                    .map_or(true, |(prev_time, _)| *prev_time < time_sent)
                {
                    *cca_args = Some((time_sent, tx_info.cc_info));
                }

                *bytes_acked += tx_info.sent_bytes as usize;
            }

            tracing::trace!(packet_number = num.as_u64(), "Packet ACKed");

            entry.status = crate::packet::datagram::partial::TransmissionStatus::Acknowledged;
            queue_range.push_back(entry);
        }

        queue.prepend(&mut queue_range);
    }

    let _ = acked.send(queue);
}

/// Detect lost packets using QUIC loss detection algorithm and queue for retransmission
///
/// TODO: Implement retransmission attempt limits. Currently, lost packets are retransmitted
/// indefinitely which could cause them to get stuck in the system permanently if they never
/// get ACKed. We need to:
/// - Track retransmission count per packet (add field to PartialDatagram or TransmissionInfo)
/// - Set a maximum retransmission limit (e.g., 10 attempts)
/// - When limit is reached, mark transmission as Failed(FailureReason::TransmissionError)
/// - Send completion notification with failure status to abandon the stream
///
/// TODO: Handle UnknownPathSecret failure. When a packet is rejected with UnknownPathSecret,
/// we need to:
/// - Detect this condition (likely from a control packet or lack of ACKs)
/// - Mark all pending transmissions for that path as Failed(FailureReason::UnknownPathSecret)
/// - Send completion notifications so streams can abandon gracefully
/// - Possibly send FlowReset to peer (though not for UnknownPathSecret - peer doesn't know us)
fn detect_and_retransmit_lost_packets<Rand>(
    context: &mut socket::channel::PathContext<crate::crypto::awslc::seal::Application>,
    max_acked_pn: VarInt,
    max_tx_time: s2n_quic_core::time::Timestamp,
    lost: &mut impl channel::UnboundedSender<Queue<PartialDatagram>>,
    now: s2n_quic_core::time::Timestamp,
    random: &mut Rand,
) where
    Rand: random::Generator,
{
    // Calculate loss delay using QUIC loss detection algorithm
    let loss_delay = {
        let rtt = context
            .rtt_estimator
            .smoothed_rtt()
            .max(context.rtt_estimator.latest_rtt());
        // kTimeThreshold is typically 9/8 per RFC
        let time_threshold = rtt + rtt / 8;
        // kGranularity is typically 1ms
        time_threshold.max(Duration::from_millis(1))
    };

    let loss_time = max_tx_time.checked_sub(loss_delay);

    // Packet number threshold: packets <= max_acked_pn - 3 are considered lost
    let pn_threshold = max_acked_pn.checked_sub(VarInt::from_u8(3));

    // Find the maximum lost packet number
    let lost_min = PacketNumberSpace::Initial.new_packet_number(VarInt::ZERO);
    let lost_max = pn_threshold.map(|v| PacketNumberSpace::Initial.new_packet_number(v));

    let mut lost_queue = Queue::new();

    // Remove lost packets and batch them for retransmission (enables GSO)
    if let Some(lost_max) = lost_max {
        let range = PacketNumberRange::new(lost_min, lost_max);
        let mut lost_count = 0usize;
        for (num, mut entry) in context.packet_number_map.remove_range(range) {
            // Update CCA for packet loss
            let tx_info = entry.transmission_info.take().unwrap();

            tracing::trace!(
                pn = num.as_u64(),
                max_acked = max_acked_pn.as_u64(),
                time_sent = ?tx_info.time_sent,
                "Packet lost by PN threshold"
            );

            context
                .cca
                .on_packet_lost(tx_info.sent_bytes as u32, tx_info.cc_info, random, now);

            lost_count += 1;
            lost_queue.push_back(entry);
        }

        if lost_count > 0 {
            tracing::debug!(
                lost_count,
                max_acked = max_acked_pn.as_u64(),
                threshold = pn_threshold.map(|v| v.as_u64()),
                rtt = ?context.rtt_estimator.smoothed_rtt(),
                "Loss detection triggered"
            );
        }
    };

    let _ = lost.send(lost_queue);

    // TODO also do time-based loss detection
    // for (num, packet) in context.packet_number_map.iter() {
    //     if let Some(tx_info) = &packet.transmission_info {
    //         // A packet is considered lost if it meets either condition:
    //         // 1. Time threshold: sent before loss_time
    //         // 2. Packet number threshold: packet number <= max_acked_pn - 3
    //         let lost_by_time = loss_time.map_or(false, |loss_time| tx_info.time_sent <= loss_time);
    //         let lost_by_pn =
    //             pn_threshold.map_or(false, |threshold| num.as_u64() <= threshold.as_u64());

    //         if lost_by_time || lost_by_pn {
    //             lost_max = Some(num);
    //             continue;
    //         }
    //     }

    //     break;
    // }
}

// ── Flow Initialization ────────────────────────────────────────────────────

/// Placeholder type for stream data in flow queues
pub enum StreamMsg {
    /// Received after a flow has been validated
    FlowValidated,
    Data {
        offset: VarInt,
        fin: bool,
        payload: BytesMut,
    },
    Reset {
        error_code: VarInt,
    },
}

/// Placeholder type for control data in flow queues
pub enum ControlMsg {
    Frames { payload: BytesMut },
    Reset { error_code: VarInt },
}

/// Flow initialization message delivered to acceptors
pub struct FlowInit {
    /// Stream ID from the client (global identifier)
    pub stream_id: VarInt,
    /// Queue ID from the peer (for routing responses)
    pub peer_queue_id: VarInt,
    /// Path secret entry for encrypting packets to this peer
    /// (use path_entry.data_addr() to get the peer's data address)
    pub path_entry: Arc<PathSecretEntry>,
    /// Sender for transmitting batches (shared wheel input)
    pub wheel_tx: intrusive_queue::sync::Sender<Batch>,
    /// Control handle for the flow queue
    pub queue_control: queue::Control<StreamMsg, ControlMsg, flow::Handle>,
    /// Stream handle for the flow queue
    pub queue_stream: queue::Stream<StreamMsg, ControlMsg, flow::Handle>,
}

// ── Datagram Processing ────────────────────────────────────────────────────

enum ProcessError {
    PeerStateLookup {
        credentials: Credentials,
        control_out: Vec<u8>,
    },
    Decryption {
        credentials: Credentials,
        packet_number: VarInt,
    },
    Duplicate {
        credentials: Credentials,
        packet_number: VarInt,
    },
    MissingSenderId,
}

#[derive(Clone)]
struct ProcessDatagramCounters {
    rx_none: Counter,
    rx_init: Counter,
    rx_validate: Counter,
    rx_init_validate: Counter,
    rx_data: Counter,
    rx_control: Counter,
    rx_reset: Counter,

    rx_init_dup: Counter,
    rx_init_too_old: Counter,
    rx_init_retx: Counter,
    rx_init_accepted: Counter,
    rx_init_accepted_retry: Counter,
    rx_init_reject: Counter,
    rx_init_no_acceptor: Counter,
    rx_init_acceptor_reset: Counter,

    rx_validate_ok: Counter,
    rx_validate_failed: Counter,
    rx_init_validate_ok: Counter,
    rx_init_validate_validation_failed: Counter,
    rx_init_validate_dispatch_failed: Counter,

    rx_data_ok: Counter,
    rx_data_unallocated: Counter,
    rx_data_half_closed: Counter,
    rx_data_fully_closed: Counter,
    rx_data_perm_closed: Counter,

    rx_control_ok: Counter,
    rx_control_unallocated: Counter,
    rx_control_half_closed: Counter,
    rx_control_fully_closed: Counter,
    rx_control_perm_closed: Counter,

    rx_reset_both: Counter,
    rx_reset_stream: Counter,
    rx_reset_control: Counter,

    tx_validate: Counter,
    tx_init_validate: Counter,
    tx_reset: Counter,
    tx_reset_both: Counter,
    tx_reset_stream: Counter,
    tx_reset_control: Counter,

    resp_ack_only: Counter,
    resp_ack_and_routing: Counter,
    resp_routing_only: Counter,
    resp_suppressed: Counter,
    resp_entries: Counter,

    flow_accepted: Counter,
    flow_pending: Counter,
}

impl ProcessDatagramCounters {
    fn new(counters: &CounterRegistry) -> Self {
        Self {
            rx_none: counters.register("!rx.none"),
            rx_init: counters.register("rx.init"),
            rx_validate: counters.register("rx.validate"),
            rx_init_validate: counters.register("rx.init_validate"),
            rx_data: counters.register("rx.data"),
            rx_control: counters.register("rx.control"),
            rx_reset: counters.register("rx.reset"),

            rx_init_dup: counters.register("!rx.init.dup"),
            rx_init_too_old: counters.register("!rx.init.too_old"),
            rx_init_retx: counters.register("rx.init.retx"),
            rx_init_accepted: counters.register("rx.init.accepted"),
            rx_init_accepted_retry: counters.register("rx.init.accepted_retry"),
            rx_init_reject: counters.register("!rx.init.reject"),
            rx_init_no_acceptor: counters.register("!rx.init.no_acceptor"),
            rx_init_acceptor_reset: counters.register("!rx.init.acceptor_reset"),

            rx_validate_ok: counters.register("rx.validate.ok"),
            rx_validate_failed: counters.register("!rx.validate.failed"),
            rx_init_validate_ok: counters.register("rx.init_validate.ok"),
            rx_init_validate_validation_failed: counters
                .register("!rx.init_validate.validation_failed"),
            rx_init_validate_dispatch_failed: counters
                .register("!rx.init_validate.dispatch_failed"),

            rx_data_ok: counters.register("rx.data.ok"),
            rx_data_unallocated: counters.register("!rx.data.unallocated"),
            rx_data_half_closed: counters.register("!rx.data.half_closed"),
            rx_data_fully_closed: counters.register("!rx.data.fully_closed"),
            rx_data_perm_closed: counters.register("rx.data.perm_closed"),

            rx_control_ok: counters.register("rx.control.ok"),
            rx_control_unallocated: counters.register("!rx.control.unallocated"),
            rx_control_half_closed: counters.register("!rx.control.half_closed"),
            rx_control_fully_closed: counters.register("!rx.control.fully_closed"),
            rx_control_perm_closed: counters.register("rx.control.perm_closed"),

            rx_reset_both: counters.register("rx.reset.both"),
            rx_reset_stream: counters.register("rx.reset.stream"),
            rx_reset_control: counters.register("rx.reset.control"),

            tx_validate: counters.register("tx.validate"),
            tx_init_validate: counters.register("tx.init_validate"),
            tx_reset: counters.register("tx.reset"),
            tx_reset_both: counters.register("tx.reset.both"),
            tx_reset_stream: counters.register("tx.reset.stream"),
            tx_reset_control: counters.register("tx.reset.control"),

            resp_ack_only: counters.register("resp.ack_only"),
            resp_ack_and_routing: counters.register("resp.ack+routing"),
            resp_routing_only: counters.register("resp.routing_only"),
            resp_suppressed: counters.register("resp.suppressed"),
            resp_entries: counters.register("resp.entries"),

            flow_accepted: counters.register("flow.accepted"),
            flow_pending: counters.register("flow.pending"),
        }
    }

    #[inline]
    fn on_received_routing(&self, routing_info: &RoutingInfo) {
        match routing_info {
            RoutingInfo::None => self.rx_none.add(1),
            RoutingInfo::FlowInit { .. } => self.rx_init.add(1),
            RoutingInfo::FlowValidateRequest { .. } => self.rx_validate.add(1),
            RoutingInfo::FlowInitValidate { .. } => self.rx_init_validate.add(1),
            RoutingInfo::FlowData { .. } => self.rx_data.add(1),
            RoutingInfo::FlowControl { .. } => self.rx_control.add(1),
            RoutingInfo::FlowReset { .. } => self.rx_reset.add(1),
        };
    }

    #[inline]
    fn on_sent_routing(&self, routing_info: &RoutingInfo) {
        match routing_info {
            RoutingInfo::FlowValidateRequest { .. } => self.tx_validate.add(1),
            RoutingInfo::FlowInitValidate { .. } => self.tx_init_validate.add(1),
            RoutingInfo::FlowReset { reset_target, .. } => {
                self.tx_reset.add(1);
                match reset_target {
                    ResetTarget::Both => self.tx_reset_both.add(1),
                    ResetTarget::Stream => self.tx_reset_stream.add(1),
                    ResetTarget::Control => self.tx_reset_control.add(1),
                };
            }
            _ => {}
        };
    }
}

/// Process a received datagram packet - authenticate, deduplicate, dispatch, and generate ACK.
///
/// This does common packet processing (decrypt, packet number dedup, ACK recording),
/// then dispatches to type-specific handlers based on routing_info, and generates the ACK batch.
/// Returns a batch containing the ACK packet with the correct peer address.
fn process_datagram<Clk, R: SenderRoute>(
    packet: Entry<packet::datagram::decoder::Packet<descriptor::Filled>>,
    sender_cache: &mut SenderStateCache,
    path_secret_map: &path::secret::Map,
    acceptor_registry: &acceptor::Registry<FlowInit>,
    wheel_tx: &intrusive_queue::sync::Sender<Batch>,
    response_tx: &mut impl channel::UnboundedSender<Queue<PartialDatagram>>,
    queue_dispatcher: &mut queue::Dispatch<StreamMsg, ControlMsg, flow::Handle>,
    clock: &Clk,
    sender_id_route: R,
    counters: &ProcessDatagramCounters,
) -> Result<(), ProcessError>
where
    Clk: s2n_quic_core::time::Clock + ?Sized,
{
    let credentials = *packet.credentials();
    let packet_number = packet.packet_number();
    let routing_info = packet.routing_info();
    let idle_timeout = sender_cache.idle_timeout;

    // Extract peer address for ACK routing
    let mut peer_addr = packet.storage().remote_address().get();
    let source_control_port = packet.meta().source_control_port();
    if source_control_port > 0 {
        peer_addr.set_port(source_control_port);
    }

    // Extract source_sender_id for sender state lookup
    let Some(source_sender_id) = routing_info.source_sender_id() else {
        return Err(ProcessError::MissingSenderId);
    };

    // Get or create sender state (single lookup keyed by credentials + sender_id)
    let mut control_out = Vec::new();
    let Some(sender_state) = sender_cache.get_or_insert(
        &credentials,
        source_sender_id,
        path_secret_map,
        clock,
        &mut control_out,
    ) else {
        return Err(ProcessError::PeerStateLookup {
            credentials,
            control_out,
        });
    };

    // Authenticate the packet by decrypting into a buffer
    // Allocate buffer for application header + payload
    let len = packet.decrypt_into_len();
    let mut buf = bytes::BytesMut::with_capacity(len);

    // Decrypt into buffer
    let written = packet
        .decrypt_into(&sender_state.opener, bytes::BufMut::chunk_mut(&mut buf))
        .map_err(|_| ProcessError::Decryption {
            credentials,
            packet_number,
        })?;

    unsafe {
        debug_assert_eq!(written, len);
        buf.set_len(len);
    }

    // Check packet number deduplication
    if sender_state
        .ack_space
        .filter
        .on_packet_number(packet_number)
        .is_err()
    {
        return Err(ProcessError::Duplicate {
            credentials,
            packet_number,
        });
    }

    // Update activity and ACK tracking
    sender_state.update_activity(clock, idle_timeout);
    let ecn = packet.storage().ecn();
    sender_state.ecn_counts.increment(ecn);
    sender_state
        .ack_space
        .on_packet_received(packet_number, clock.get_time());
    sender_state.transmission_state = AckTransmissionState::Queued;

    // Track response packet to send (only one per incoming packet)
    let mut response_routing: Option<RoutingInfo> = None;

    // Dispatch based on routing_info for type-specific processing
    let routing_info = packet.routing_info();
    counters.on_received_routing(&routing_info);
    match routing_info {
        RoutingInfo::None => {
            tracing::warn!("RoutingInfo::None - dropping packet");
        }
        RoutingInfo::FlowInit {
            source_sender_id: _,
            source_queue_id: peer_queue_id,
            dest_acceptor_id: acceptor_id,
            attempt_id,
            stream_id,
            is_fin,
        } => {
            // Check attempt_id deduplication
            match sender_state.attempt_dedup.check_attempt_id(attempt_id) {
                Ok(()) => {
                    // New attempt_id, proceed with flow creation

                    // Record the flow in the sender's flow map and allocate queue handles
                    let create_queue = |handle| {
                        // Server-side allocation knows the remote queue ID from FlowInit
                        let (queue_control, queue_stream) =
                            queue_dispatcher.alloc_or_grow(handle, Some(peer_queue_id));
                        let queue_id = queue_control.queue_id();
                        (queue_id, (queue_control, queue_stream))
                    };

                    match sender_state.flows.try_register(stream_id, create_queue) {
                        Ok((queue_control, queue_stream)) => {
                            // Inject the payload into the stream so the application can read it
                            if is_fin || !buf.is_empty() {
                                queue_stream.push(
                                    StreamMsg::Data {
                                        offset: VarInt::ZERO,
                                        fin: is_fin,
                                        payload: buf,
                                    }
                                    .into(),
                                );
                            }

                            let local_queue_id = queue_control.queue_id();

                            // Create the FlowInit message for the acceptor
                            let flow_init = FlowInit {
                                stream_id,
                                path_entry: sender_state.path_entry.clone(),
                                peer_queue_id,
                                wheel_tx: wheel_tx.clone(),
                                queue_control,
                                queue_stream,
                            };

                            // Dispatch to the acceptor
                            match acceptor_registry.dispatch(acceptor_id, flow_init) {
                                Ok(()) => {
                                    counters.flow_accepted.add(1);
                                    tracing::debug!(
                                        attempt_id = attempt_id.as_u64(),
                                        stream_id = stream_id.as_u64(),
                                        acceptor_id = acceptor_id.as_u64(),
                                        server_queue_id = local_queue_id.as_u64(),
                                        "FlowInit accepted - dispatched to acceptor"
                                    );
                                }
                                Err(acceptor::DispatchError::AcceptorNotFound) => {
                                    tracing::debug!(
                                        attempt_id = attempt_id.as_u64(),
                                        stream_id = stream_id.as_u64(),
                                        acceptor_id = acceptor_id.as_u64(),
                                        "FlowInit rejected - acceptor not found"
                                    );

                                    response_routing = Some(RoutingInfo::FlowReset {
                                        source_sender_id: VarInt::MAX,
                                        dest_queue_id: peer_queue_id,
                                        stream_id,
                                        reset_target: ResetTarget::Both,
                                        error_code: reset_error::ACCEPTOR_NOT_FOUND,
                                    });
                                }
                                Err(acceptor::DispatchError::Reset { reset_code }) => {
                                    tracing::debug!(
                                        attempt_id = attempt_id.as_u64(),
                                        stream_id = stream_id.as_u64(),
                                        acceptor_id = acceptor_id.as_u64(),
                                        reset_code = reset_code.as_u64(),
                                        "FlowInit rejected - acceptor requested reset"
                                    );

                                    response_routing = Some(RoutingInfo::FlowReset {
                                        source_sender_id: VarInt::MAX,
                                        dest_queue_id: peer_queue_id,
                                        stream_id,
                                        reset_target: ResetTarget::Both,
                                        error_code: reset_code,
                                    });
                                }
                            }
                        }
                        Err(local_queue_id) => {
                            tracing::debug!(
                                attempt_id = attempt_id.as_u64(),
                                stream_id = stream_id.as_u64(),
                                local_queue_id = local_queue_id.as_u64(),
                                "FlowInit rejected - stream_id reused by client"
                            );

                            response_routing = Some(RoutingInfo::FlowReset {
                                source_sender_id: VarInt::MAX,
                                dest_queue_id: peer_queue_id,
                                stream_id,
                                reset_target: ResetTarget::Both,
                                error_code: reset_error::STREAM_ID_ERROR,
                            });
                        }
                    }
                }
                Err(AttemptDedupError::Duplicate) => {
                    counters.rx_init_dup.add(1);
                    // Already seen this attempt_id - silently drop
                    tracing::trace!(
                        attempt_id = attempt_id.as_u64(),
                        stream_id = stream_id.as_u64(),
                        "Duplicate FlowInit attempt_id - dropping"
                    );
                    // Still generate ACK below
                }
                Err(AttemptDedupError::TooOld) => {
                    counters.rx_init_too_old.add(1);
                    // Attempt ID outside window - check DashMap (medium path)
                    let create_queue = |handle| {
                        // Server-side allocation knows the remote queue ID from FlowInit
                        let (queue_control, queue_stream) =
                            queue_dispatcher.alloc_or_grow(handle, Some(peer_queue_id));
                        let queue_id = queue_control.queue_id();
                        (queue_id, (queue_control, queue_stream))
                    };

                    match sender_state.flows.try_register(stream_id, create_queue) {
                        Ok((queue_control, queue_stream)) => {
                            // Not in window and not in DashMap - can't guarantee deduplication
                            // Inject the payload into the stream so the application can read it if accepted
                            if is_fin || !buf.is_empty() {
                                queue_stream.push(
                                    StreamMsg::Data {
                                        offset: VarInt::ZERO,
                                        fin: is_fin,
                                        payload: buf,
                                    }
                                    .into(),
                                );
                            }

                            let local_queue_id = queue_control.queue_id();

                            // Create the FlowInit message for the acceptor
                            let flow_init = FlowInit {
                                stream_id,
                                path_entry: sender_state.path_entry.clone(),
                                peer_queue_id,
                                wheel_tx: wheel_tx.clone(),
                                queue_control,
                                queue_stream,
                            };

                            // Dispatch as pending since we can't guarantee it's not a duplicate
                            match acceptor_registry.dispatch_pending(acceptor_id, flow_init) {
                                Ok(acceptor::PendingAction::Accepted) => {
                                    counters.rx_init_accepted.add(1);
                                    counters.flow_accepted.add(1);
                                    tracing::debug!(
                                        attempt_id = attempt_id.as_u64(),
                                        stream_id = stream_id.as_u64(),
                                        acceptor_id = acceptor_id.as_u64(),
                                        server_queue_id = local_queue_id.as_u64(),
                                        "FlowInit accepted without retry - acceptor doesn't require dedup"
                                    );
                                }
                                Ok(acceptor::PendingAction::AcceptedWithRetry) => {
                                    counters.rx_init_accepted_retry.add(1);
                                    counters.flow_pending.add(1);
                                    tracing::debug!(
                                        attempt_id = attempt_id.as_u64(),
                                        stream_id = stream_id.as_u64(),
                                        acceptor_id = acceptor_id.as_u64(),
                                        server_queue_id = local_queue_id.as_u64(),
                                        "FlowInit accepted with retry - requesting validation from client"
                                    );

                                    // Send FlowValidateRequest to have client confirm this queue state
                                    let queue_pair = QueuePair {
                                        source_queue_id: local_queue_id,
                                        dest_queue_id: peer_queue_id,
                                    };

                                    response_routing = Some(RoutingInfo::FlowValidateRequest {
                                        source_sender_id: VarInt::MAX,
                                        dest_sender_id: source_sender_id,
                                        queue_pair,
                                        attempt_id,
                                        stream_id,
                                    });
                                }
                                Ok(acceptor::PendingAction::Reject { reset_code }) => {
                                    counters.rx_init_reject.add(1);
                                    tracing::debug!(
                                        attempt_id = attempt_id.as_u64(),
                                        stream_id = stream_id.as_u64(),
                                        acceptor_id = acceptor_id.as_u64(),
                                        reset_code = reset_code.as_u64(),
                                        "FlowInit rejected - acceptor rejected pending request"
                                    );

                                    response_routing = Some(RoutingInfo::FlowReset {
                                        source_sender_id: VarInt::MAX,
                                        dest_queue_id: peer_queue_id,
                                        stream_id,
                                        reset_target: ResetTarget::Both,
                                        error_code: reset_code,
                                    });
                                }
                                Err(acceptor::DispatchError::AcceptorNotFound) => {
                                    counters.rx_init_no_acceptor.add(1);
                                    tracing::debug!(
                                        attempt_id = attempt_id.as_u64(),
                                        stream_id = stream_id.as_u64(),
                                        acceptor_id = acceptor_id.as_u64(),
                                        "FlowInit rejected - acceptor not found"
                                    );

                                    response_routing = Some(RoutingInfo::FlowReset {
                                        source_sender_id: VarInt::MAX,
                                        dest_queue_id: peer_queue_id,
                                        stream_id,
                                        reset_target: ResetTarget::Both,
                                        error_code: reset_error::ACCEPTOR_NOT_FOUND,
                                    });
                                }
                                Err(acceptor::DispatchError::Reset { reset_code }) => {
                                    counters.rx_init_acceptor_reset.add(1);
                                    tracing::debug!(
                                        attempt_id = attempt_id.as_u64(),
                                        stream_id = stream_id.as_u64(),
                                        acceptor_id = acceptor_id.as_u64(),
                                        reset_code = reset_code.as_u64(),
                                        "FlowInit rejected - acceptor requested reset"
                                    );

                                    response_routing = Some(RoutingInfo::FlowReset {
                                        source_sender_id: VarInt::MAX,
                                        dest_queue_id: peer_queue_id,
                                        stream_id,
                                        reset_target: ResetTarget::Both,
                                        error_code: reset_code,
                                    });
                                }
                            }
                        }
                        Err(local_queue_id) => {
                            counters.rx_init_retx.add(1);
                            // Flow already exists, this is a retransmission
                            tracing::trace!(
                                attempt_id = attempt_id.as_u64(),
                                stream_id = stream_id.as_u64(),
                                queue_id = local_queue_id.as_u64(),
                                "FlowInit retransmission of existing flow - dropping"
                            );
                            // Still generate ACK below, then return
                        }
                    }
                }
            }
        }
        RoutingInfo::FlowValidateRequest {
            source_sender_id: _,
            dest_sender_id,
            queue_pair,
            attempt_id,
            stream_id,
        } => {
            // Client receives FlowValidateRequest from server when server cannot guarantee deduplication.
            // We need to validate our local queue state and respond with FlowInitValidate or FlowReset.

            let local_queue_id = queue_pair.dest_queue_id; // Our queue

            // Create validation parameters
            let request = flow::Request {
                credential_id: credentials.id,
                stream_id,
            };

            // Validate that our local stream queue has matching credentials and stream_id
            match queue_dispatcher.validate_stream(local_queue_id, &request) {
                Ok(()) => {
                    counters.rx_validate_ok.add(1);
                    // Validation passed - respond with FlowInitValidate
                    tracing::debug!(
                        attempt_id = attempt_id.as_u64(),
                        stream_id = stream_id.as_u64(),
                        dest_sender_id = dest_sender_id.as_u64(),
                        local_queue_id = local_queue_id.as_u64(),
                        server_queue_id = queue_pair.source_queue_id.as_u64(),
                        "FlowValidateRequest validated - sending FlowInitValidate"
                    );

                    // Create FlowInitValidate routing info, reversing the queue pair for the response
                    response_routing = Some(RoutingInfo::FlowInitValidate {
                        source_sender_id: VarInt::MAX,
                        queue_pair: queue_pair.reverse(), // Reverse for response routing
                        attempt_id,
                        stream_id,
                    });
                }
                Err(_) => {
                    counters.rx_validate_failed.add(1);
                    // Validation failed - client may have abandoned the stream
                    // Send FlowReset to let server release its allocated queue_id
                    tracing::warn!(
                        attempt_id = attempt_id.as_u64(),
                        stream_id = stream_id.as_u64(),
                        local_queue_id = local_queue_id.as_u64(),
                        "FlowValidateRequest validation failed - sending FlowReset"
                    );

                    response_routing = Some(RoutingInfo::FlowReset {
                        source_sender_id: VarInt::MAX,
                        dest_queue_id: queue_pair.source_queue_id, // Reset the peer's queue
                        stream_id,
                        reset_target: ResetTarget::Both,
                        error_code: reset_error::STALE_STATE,
                    });
                }
            }
        }
        RoutingInfo::FlowInitValidate {
            source_sender_id: _,
            queue_pair,
            attempt_id,
            stream_id,
        } => {
            // Server receives FlowInitValidate from client in response to FlowValidateRequest.
            // We need to validate that the queue still has the correct credentials and stream_id.

            let local_queue_id = queue_pair.dest_queue_id; // Our queue

            // Create validation parameters
            let request = flow::Request {
                credential_id: credentials.id,
                stream_id,
            };

            // Validate the queue has matching credentials and stream_id
            match queue_dispatcher.validate_stream(local_queue_id, &request) {
                Ok(()) => {
                    counters.rx_init_validate_ok.add(1);
                    // Flow validation succeeded - send FlowValidated message to wake up the acceptor
                    let stream_entry = StreamMsg::FlowValidated.into();

                    match queue_dispatcher.send_stream(
                        local_queue_id,
                        Some(queue_pair.source_queue_id),
                        &request,
                        stream_entry,
                    ) {
                        Ok(()) => {
                            tracing::debug!(
                                attempt_id = attempt_id.as_u64(),
                                stream_id = stream_id.as_u64(),
                                queue_id = local_queue_id.as_u64(),
                                "FlowInitValidate validated successfully - FlowValidated message sent"
                            );
                        }
                        Err(_) => {
                            counters.rx_init_validate_dispatch_failed.add(1);
                            // Failed to send FlowValidated - queue may have been closed or unallocated
                            // Send FlowReset to client
                            tracing::warn!(
                                attempt_id = attempt_id.as_u64(),
                                stream_id = stream_id.as_u64(),
                                queue_id = local_queue_id.as_u64(),
                                "FlowInitValidate failed to send FlowValidated message - sending FlowReset"
                            );

                            response_routing = Some(RoutingInfo::FlowReset {
                                source_sender_id: VarInt::MAX,
                                dest_queue_id: queue_pair.source_queue_id,
                                stream_id,
                                reset_target: ResetTarget::Both,
                                error_code: reset_error::STALE_STATE,
                            });
                        }
                    }
                }
                Err(_) => {
                    counters.rx_init_validate_validation_failed.add(1);
                    // Validation failed - send FlowReset
                    tracing::warn!(
                        attempt_id = attempt_id.as_u64(),
                        stream_id = stream_id.as_u64(),
                        queue_id = local_queue_id.as_u64(),
                        "FlowInitValidate validation failed - sending FlowReset"
                    );

                    response_routing = Some(RoutingInfo::FlowReset {
                        source_sender_id: VarInt::MAX,
                        dest_queue_id: queue_pair.source_queue_id, // Reset the peer's queue
                        stream_id,
                        reset_target: ResetTarget::Both,
                        error_code: reset_error::STALE_STATE,
                    });
                }
            }
        }
        RoutingInfo::FlowData {
            source_sender_id: _,
            queue_pair,
            stream_id,
            offset,
            is_fin,
        } => {
            // Use dest_queue_id directly from the packet for routing
            let local_queue_id = queue_pair.dest_queue_id;

            // Create validation parameters for the queue
            let request = flow::Request {
                credential_id: credentials.id,
                stream_id,
            };

            // Dispatch to the queue with validation
            let entry = StreamMsg::Data {
                offset,
                fin: is_fin,
                payload: buf,
            }
            .into();

            match queue_dispatcher.send_stream(
                local_queue_id,
                Some(queue_pair.source_queue_id),
                &request,
                entry,
            ) {
                Ok(()) => {
                    counters.rx_data_ok.add(1);
                    tracing::trace!(
                        stream_id = stream_id.as_u64(),
                        queue_id = local_queue_id.as_u64(),
                        offset = offset.as_u64(),
                        is_fin,
                        "FlowData dispatched to queue"
                    );
                }
                Err(queue::Error::Unallocated(_)) => {
                    counters.rx_data_unallocated.add(1);
                    tracing::warn!(
                        stream_id = stream_id.as_u64(),
                        queue_id = local_queue_id.as_u64(),
                        "FlowData for unallocated queue - sending reset"
                    );

                    response_routing = Some(RoutingInfo::FlowReset {
                        source_sender_id: VarInt::MAX,
                        dest_queue_id: queue_pair.source_queue_id,
                        stream_id,
                        reset_target: ResetTarget::Both,
                        error_code: reset_error::STALE_STATE,
                    });
                }
                Err(queue::Error::HalfClosed(_)) => {
                    counters.rx_data_half_closed.add(1);
                    tracing::debug!(
                        stream_id = stream_id.as_u64(),
                        queue_id = local_queue_id.as_u64(),
                        "FlowData for half-closed stream - sending reset"
                    );

                    response_routing = Some(RoutingInfo::FlowReset {
                        source_sender_id: VarInt::MAX,
                        dest_queue_id: queue_pair.source_queue_id,
                        stream_id,
                        reset_target: ResetTarget::Stream,
                        error_code: reset_error::STALE_STATE,
                    });
                }
                Err(queue::Error::FullyClosed(_)) => {
                    counters.rx_data_fully_closed.add(1);
                    tracing::debug!(
                        stream_id = stream_id.as_u64(),
                        queue_id = local_queue_id.as_u64(),
                        "FlowData for fully closed queue - sending reset"
                    );

                    response_routing = Some(RoutingInfo::FlowReset {
                        source_sender_id: VarInt::MAX,
                        dest_queue_id: queue_pair.source_queue_id,
                        stream_id,
                        reset_target: ResetTarget::Both,
                        error_code: reset_error::STALE_STATE,
                    });
                }
                Err(queue::Error::PermanentlyClosed) => {
                    counters.rx_data_perm_closed.add(1);
                    tracing::trace!(
                        stream_id = stream_id.as_u64(),
                        queue_id = local_queue_id.as_u64(),
                        "FlowData for permanently closed queue - dropping"
                    );
                    // Don't send reset - the sender is gone so no point
                }
            }
        }
        RoutingInfo::FlowControl {
            source_sender_id: _,
            queue_pair,
            stream_id,
        } => {
            // Use dest_queue_id directly from the packet for routing
            let local_queue_id = queue_pair.dest_queue_id;

            // Create validation parameters for the queue
            let request = flow::Request {
                credential_id: credentials.id,
                stream_id,
            };

            // Dispatch to the queue with validation
            let entry = ControlMsg::Frames { payload: buf }.into();

            match queue_dispatcher.send_control(
                local_queue_id,
                Some(queue_pair.source_queue_id),
                &request,
                entry,
            ) {
                Ok(()) => {
                    counters.rx_control_ok.add(1);
                    tracing::trace!(
                        stream_id = stream_id.as_u64(),
                        queue_id = local_queue_id.as_u64(),
                        "FlowControl dispatched to queue"
                    );
                }
                Err(queue::Error::Unallocated(_)) => {
                    counters.rx_control_unallocated.add(1);
                    tracing::debug!(
                        stream_id = stream_id.as_u64(),
                        queue_id = local_queue_id.as_u64(),
                        "FlowControl for unallocated queue - sending reset"
                    );

                    response_routing = Some(RoutingInfo::FlowReset {
                        source_sender_id: VarInt::MAX,
                        dest_queue_id: queue_pair.source_queue_id,
                        stream_id,
                        reset_target: ResetTarget::Both,
                        error_code: reset_error::STALE_STATE,
                    });
                }
                Err(queue::Error::HalfClosed(_)) => {
                    counters.rx_control_half_closed.add(1);
                    tracing::debug!(
                        stream_id = stream_id.as_u64(),
                        queue_id = local_queue_id.as_u64(),
                        "FlowControl for half-closed control - sending reset"
                    );

                    response_routing = Some(RoutingInfo::FlowReset {
                        source_sender_id: VarInt::MAX,
                        dest_queue_id: queue_pair.source_queue_id,
                        stream_id,
                        reset_target: ResetTarget::Control,
                        error_code: reset_error::STALE_STATE,
                    });
                }
                Err(queue::Error::FullyClosed(_)) => {
                    counters.rx_control_fully_closed.add(1);
                    tracing::debug!(
                        stream_id = stream_id.as_u64(),
                        queue_id = local_queue_id.as_u64(),
                        "FlowControl for fully closed queue - sending reset"
                    );

                    response_routing = Some(RoutingInfo::FlowReset {
                        source_sender_id: VarInt::MAX,
                        dest_queue_id: queue_pair.source_queue_id,
                        stream_id,
                        reset_target: ResetTarget::Both,
                        error_code: reset_error::STALE_STATE,
                    });
                }
                Err(queue::Error::PermanentlyClosed) => {
                    counters.rx_control_perm_closed.add(1);
                    tracing::trace!(
                        stream_id = stream_id.as_u64(),
                        queue_id = local_queue_id.as_u64(),
                        "FlowControl for permanently closed queue - dropping"
                    );
                    // Don't send reset - the sender is gone so no point
                }
            }
        }
        RoutingInfo::FlowReset {
            source_sender_id: _,
            dest_queue_id,
            stream_id,
            reset_target,
            error_code,
        } => {
            // Use dest_queue_id directly from the packet for routing
            let local_queue_id = dest_queue_id;

            // Create validation parameters for the queue
            let request = flow::Request {
                credential_id: credentials.id,
                stream_id,
            };

            // Send reset based on target — FlowReset doesn't carry the sender's queue ID
            match reset_target {
                ResetTarget::Both => {
                    counters.rx_reset_both.add(1);
                    let stream_entry = StreamMsg::Reset { error_code }.into();
                    let control_entry = ControlMsg::Reset { error_code }.into();
                    queue_dispatcher.send_both(
                        local_queue_id,
                        None,
                        &request,
                        stream_entry,
                        control_entry,
                    );

                    tracing::debug!(
                        stream_id = stream_id.as_u64(),
                        queue_id = local_queue_id.as_u64(),
                        error_code = error_code.as_u64(),
                        "FlowReset(Both) dispatched to queue"
                    );
                }
                ResetTarget::Stream => {
                    counters.rx_reset_stream.add(1);
                    let stream_entry = StreamMsg::Reset { error_code }.into();
                    let _ =
                        queue_dispatcher.send_stream(local_queue_id, None, &request, stream_entry);

                    tracing::debug!(
                        stream_id = stream_id.as_u64(),
                        queue_id = local_queue_id.as_u64(),
                        error_code = error_code.as_u64(),
                        "FlowReset(Stream) dispatched to queue"
                    );
                }
                ResetTarget::Control => {
                    counters.rx_reset_control.add(1);
                    let control_entry = ControlMsg::Reset { error_code }.into();
                    let _ = queue_dispatcher.send_control(
                        local_queue_id,
                        None,
                        &request,
                        control_entry,
                    );

                    tracing::debug!(
                        stream_id = stream_id.as_u64(),
                        queue_id = local_queue_id.as_u64(),
                        error_code = error_code.as_u64(),
                        "FlowReset(Control) dispatched to queue"
                    );
                }
            }
        }
    }

    // Push response packets to channel for batching
    let mut entries = Queue::new();

    let local_sender_id = sender_id_route.sender_id(&credentials.id, source_sender_id);
    let ack_routing_info = packet::control::RoutingInfo::Sender {
        source_sender_id: local_sender_id,
        dest_sender_id: source_sender_id,
    };

    let has_ack = if let Some(ack_packet) = sender_state.generate_ack_packet(clock, ack_routing_info) {
        sender_state.transmission_state = AckTransmissionState::Idle;
        entries.push_back(Entry::new(ack_packet));
        true
    } else {
        false
    };

    // Send response packet if needed (FlowValidateRequest, FlowReset, etc.)
    let has_routing = if let Some(mut routing) = response_routing {
        counters.on_sent_routing(&routing);
        routing.set_source_sender_id(local_sender_id);
        let packet = PartialDatagram::new_datagram(
            routing,
            crate::byte_vec::ByteVec::new(), // No application header
            crate::byte_vec::ByteVec::new(), // No payload
            sender_state.path_entry.clone(),
            None, // No completion tracking
        );
        entries.push_back(Entry::new(packet));
        true
    } else {
        false
    };

    match (has_ack, has_routing) {
        (true, false) => {
            counters.resp_ack_only.add(1);
            counters.resp_entries.add(1);
        }
        (true, true) => {
            counters.resp_ack_and_routing.add(1);
            counters.resp_entries.add(2);
        }
        (false, true) => {
            counters.resp_routing_only.add(1);
            counters.resp_entries.add(1);
        }
        (false, false) => counters.resp_suppressed.add(1),
    }

    let _ = response_tx.send(entries);

    Ok(())
}

enum ProcessControlError {
    PeerStateLookup {
        credentials: Credentials,
        control_out: Vec<u8>,
    },
    Verification {
        credentials: Credentials,
        packet_number: VarInt,
    },
    MissingSenderId,
}

/// Process a received control packet for ACK processing.
///
/// This authenticates the packet by verifying its MAC tag.
fn process_control<Clk>(
    packet: Entry<packet::control::decoder::Packet<descriptor::Filled>>,
    peer_cache: &mut SenderStateCache,
    path_secret_map: &path::secret::Map,
    clock: &Clk,
) -> Result<Entry<packet::control::decoder::Packet<descriptor::Filled>>, ProcessControlError>
where
    Clk: s2n_quic_core::time::Clock + ?Sized,
{
    let credentials = *packet.credentials();
    let routing_info = packet.routing_info();
    let idle_timeout = peer_cache.idle_timeout;

    // Extract source_sender_id for sender state lookup
    let Some(source_sender_id) = routing_info.source_sender_id() else {
        // Control packets without sender_id can't be processed
        return Err(ProcessControlError::MissingSenderId);
    };

    // Get or create sender state
    let mut control_out = Vec::new();
    let Some(sender_state) = peer_cache.get_or_insert(
        &credentials,
        source_sender_id,
        path_secret_map,
        clock,
        &mut control_out,
    ) else {
        return Err(ProcessControlError::PeerStateLookup {
            credentials,
            control_out,
        });
    };

    let packet_number = packet.packet_number();

    // Authenticate the packet by verifying the MAC tag
    if packet.verify(&sender_state.opener).is_err() {
        return Err(ProcessControlError::Verification {
            credentials,
            packet_number,
        });
    }

    // Update activity timestamp
    sender_state.update_activity(clock, idle_timeout);

    // Record the ECN marking from this control packet so the ACK frames we
    // send back include accurate ECN counts covering both datagram and control
    // packet types that share the same ack_space.
    let ecn = packet.storage().ecn();
    sender_state.ecn_counts.increment(ecn);

    // Record the packet for ACK
    sender_state
        .ack_space
        .on_packet_received(packet_number, clock.get_time());

    Ok(packet)
}

// ── Peer State Management ──────────────────────────────────────────────────

/// Attempt deduplication window for tracking seen attempt_ids
///
/// Uses a sliding window to efficiently deduplicate FlowInit packets within
/// a bounded memory footprint. This is the fast path for recent attempt_ids.
struct AttemptDedup {
    /// Sliding window for recent attempt_ids (same as packet number dedup)
    window: s2n_quic_core::packet::number::SlidingWindow,
}

impl AttemptDedup {
    fn new() -> Self {
        Self {
            window: Default::default(),
        }
    }

    /// Check if an attempt_id has been seen before in the recent window
    ///
    /// Returns:
    /// - Ok(()) if attempt_id is new and within window
    /// - Err(AttemptDedupError::Duplicate) if already seen in window
    /// - Err(AttemptDedupError::TooOld) if outside window (check DashMap or retry)
    fn check_attempt_id(&mut self, attempt_id: VarInt) -> Result<(), AttemptDedupError> {
        use s2n_quic_core::packet::number::{PacketNumberSpace, SlidingWindowError};

        let packet_number = PacketNumberSpace::Initial.new_packet_number(attempt_id);
        match self.window.insert(packet_number) {
            Ok(()) => Ok(()),
            Err(SlidingWindowError::TooOld) => Err(AttemptDedupError::TooOld),
            Err(SlidingWindowError::Duplicate) => Err(AttemptDedupError::Duplicate),
        }
    }
}

#[derive(Debug)]
enum AttemptDedupError {
    /// Attempt ID already seen (duplicate)
    Duplicate,
    /// Attempt ID too old (outside window) - need to check DashMap or send retry
    TooOld,
}

/// Cached crypto state and ACK tracking for a single sender
///
/// This is keyed by (credentials.id, source_sender_id) because ACK spaces and
/// deduplication windows are per-sender, not per-peer.
struct SenderState {
    /// Path secret entry for this peer
    path_entry: Arc<PathSecretEntry>,
    /// Opener for decrypting datagrams from this peer
    /// TODO: Support key rotation by maintaining multiple openers indexed by key_id.
    /// Currently we only track the latest key, which means packets with old key_ids
    /// after rotation will fail to decrypt. Need to maintain a small cache of recent
    /// openers (e.g., HashMap<VarInt, Opener>) to handle in-flight packets during rotation.
    opener: crate::crypto::awslc::open::Application,
    /// The key_id this opener corresponds to
    current_key_id: VarInt,
    /// ACK space for tracking received packets (spans all key_ids for this peer)
    ack_space: crate::stream::recv::ack::Space,
    /// Accumulated ECN counts for received packets, reported back to the sender
    /// in each ACK frame so the sender can validate ECN support and detect congestion.
    ecn_counts: EcnCounts,
    /// Timer for idle timeout
    idle_timer: s2n_quic_core::time::Timer,
    /// Last activity timestamp
    last_activity: s2n_quic_core::time::Timestamp,
    /// Transmission state for ACKs
    transmission_state: AckTransmissionState,
    /// Attempt deduplication window for flow initialization
    attempt_dedup: AttemptDedup,
    /// Map from stream_id to allocated queue_id for this sender
    /// Shared with queue handles so they can remove entries when closed
    flows: crate::flow::Tracker,
}

/// Simplified ACK transmission state for datagrams
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AckTransmissionState {
    Idle,
    Queued,
}

impl SenderState {
    fn new<Clk>(
        path_entry: Arc<PathSecretEntry>,
        opener: crate::crypto::awslc::open::Application,
        key_id: VarInt,
        clock: &Clk,
        idle_timeout: Duration,
    ) -> Self
    where
        Clk: s2n_quic_core::time::Clock + ?Sized,
    {
        let now = clock.get_time();
        let mut idle_timer = s2n_quic_core::time::Timer::default();
        idle_timer.set(now + idle_timeout);

        let flows = flow::Tracker::new(*path_entry.id());

        Self {
            path_entry,
            opener,
            current_key_id: key_id,
            ack_space: Default::default(),
            ecn_counts: Default::default(),
            idle_timer,
            last_activity: now,
            transmission_state: AckTransmissionState::Idle,
            attempt_dedup: AttemptDedup::new(),
            flows,
        }
    }

    fn update_activity<Clk>(&mut self, clock: &Clk, idle_timeout: Duration)
    where
        Clk: s2n_quic_core::time::Clock + ?Sized,
    {
        let now = clock.get_time();
        self.last_activity = now;
        self.idle_timer.set(now + idle_timeout);
    }

    fn is_expired<Clk>(&mut self, clock: &Clk) -> bool
    where
        Clk: s2n_quic_core::time::Clock + ?Sized,
    {
        self.idle_timer.poll_expiration(clock.get_time()).is_ready()
    }

    fn should_transmit(&self) -> bool {
        self.transmission_state == AckTransmissionState::Queued
    }

    /// Generate an ACK control packet for this peer
    fn generate_ack_packet<Clk>(
        &mut self,
        clock: &Clk,
        routing_info: packet::control::RoutingInfo,
    ) -> Option<PartialDatagram>
    where
        Clk: s2n_quic_core::time::Clock + ?Sized,
    {
        // Generate ACK frame from the ACK space.  Only include ECN counts when
        // at least one ECN-marked packet has been seen; this avoids forcing the
        // wider ACK-with-ECN frame encoding (which drops more ACK ranges to fit
        // the MTU) when the counts would all be zero anyway.
        let mtu = 1400u16;
        let (ack_frame, encoding_size) =
            self.ack_space
                .encoding(VarInt::ZERO, self.ecn_counts.as_option(), mtu, clock);

        let ack_frame = ack_frame?;

        // Allocate a Vec<u8> for the encoded frame
        let mut buffer = vec![0u8; encoding_size.as_u64() as usize];
        let mut encoder_buf = EncoderBuffer::new(&mut buffer);
        encoder_buf.encode(&ack_frame);

        let control_data = buffer.into();

        // Create and return the control packet
        Some(PartialDatagram::new_control(
            routing_info,
            control_data,
            self.path_entry.clone(),
        ))
    }
}

/// Key for sender state lookup — keyed by peer identity (stable) + sender_id,
/// NOT by full Credentials (which includes the per-packet key_id).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct SenderKey {
    id: credentials::Id,
    sender_id: VarInt,
}

/// Per-worker sender state cache
struct SenderStateCache {
    /// Map from (credentials, sender_id) to sender state
    senders: std::collections::HashMap<SenderKey, SenderState>,
    /// Idle timeout for sender states
    idle_timeout: Duration,
    worker_id: usize,
}

impl SenderStateCache {
    fn new(idle_timeout: Duration, worker_id: usize) -> Self {
        Self {
            senders: std::collections::HashMap::new(),
            idle_timeout,
            worker_id,
        }
    }

    #[track_caller]
    fn get_or_insert<Clk>(
        &mut self,
        credentials: &Credentials,
        sender_id: VarInt,
        path_secret_map: &crate::path::secret::map::Map,
        clock: &Clk,
        control_out: &mut Vec<u8>,
    ) -> Option<&mut SenderState>
    where
        Clk: s2n_quic_core::time::Clock + ?Sized,
    {
        let key = SenderKey {
            id: credentials.id,
            sender_id,
        };

        // Use entry API for single hash lookup
        Some(match self.senders.entry(key) {
            hash_map::Entry::Occupied(entry) => entry.into_mut(),
            hash_map::Entry::Vacant(entry) => {
                // Slow path: derive opener from map
                tracing::debug!(%credentials, %sender_id, caller = %core::panic::Location::caller(), worker_id = self.worker_id, "opener_for_credentials");
                let (opener, path_entry) = path_secret_map.opener_for_credentials(
                    credentials,
                    None, // queue_id is None for datagrams
                    control_out,
                )?;

                entry.insert(SenderState::new(
                    path_entry,
                    opener,
                    credentials.key_id,
                    clock,
                    self.idle_timeout,
                ))
            }
        })
    }

    fn cleanup_expired<Clk>(&mut self, clock: &Clk)
    where
        Clk: s2n_quic_core::time::Clock + ?Sized,
    {
        self.senders.retain(|_, state| !state.is_expired(clock));
    }
}

// ── Receive Pipeline Components ────────────────────────────────────────────

/// Helper to assert a type implements Receiver<T>
fn assert_receiver<T>(_r: &impl channel::Receiver<T>) {}

/// Packet router that routes packets to channels for processing
struct ChannelRouter<D, C> {
    datagram_tx: D,
    control_tx: C,
    decode_error_counter: Counter,
}

impl<D, C> Router for ChannelRouter<D, C>
where
    D: channel::UnboundedSender<Entry<packet::datagram::decoder::Packet<descriptor::Filled>>>,
    C: channel::UnboundedSender<Entry<packet::control::decoder::Packet<descriptor::Filled>>>,
{
    fn is_open(&self) -> bool {
        true
    }

    #[inline]
    fn dispatch_datagram_packet(
        &mut self,
        packet: packet::datagram::decoder::Packet<descriptor::Filled>,
    ) {
        let _ = self.datagram_tx.send(packet.into());
    }

    #[inline]
    fn handle_datagram_packet(
        &mut self,
        _remote_address: s2n_quic_core::inet::SocketAddress,
        _ecn: s2n_quic_core::inet::ExplicitCongestionNotification,
        _packet: packet::datagram::decoder::Packet<&mut [u8]>,
    ) {
    }

    #[inline]
    fn dispatch_control_packet(
        &mut self,
        packet: packet::control::decoder::Packet<descriptor::Filled>,
    ) {
        let _ = self.control_tx.send(packet.into());
    }

    #[inline]
    fn handle_control_packet(
        &mut self,
        _remote_address: s2n_quic_core::inet::SocketAddress,
        _ecn: s2n_quic_core::inet::ExplicitCongestionNotification,
        _packet: packet::control::decoder::Packet<&mut [u8]>,
    ) {
    }

    fn on_decode_error(
        &mut self,
        error: s2n_codec::DecoderError,
        remote_address: s2n_quic_core::inet::SocketAddress,
        segment: descriptor::Filled,
    ) {
        self.decode_error_counter.add(1);
        tracing::debug!(
            ?error,
            %remote_address,
            packet_len = segment.len(),
            "failed to decode packet"
        );
    }
}

// ── Send Pipeline Components ───────────────────────────────────────────────

/// Simple batch sender that drains a channel and sends to a socket
pub async fn batch_sender<S, R, Ctx>(socket: S, rx: R)
where
    S: socket::send::Socket,
    R: channel::Receiver<Entry<Batch<Ctx>>>,
{
    let local_addr = socket.local_addr().unwrap();
    let socket = socket::send::Tracing(socket);

    let rx = channel::SocketSender::new(rx, socket);
    let rx = channel::InspectErr::new(rx, |(error, batch)| {
        tracing::warn!(%error, meta = ?batch.meta, segments = ?batch.encoded.as_ref(), "socket send error");
    });

    // Map to () after successful send
    let rx = channel::Map::new(rx, |batch| {
        debug_assert!(batch.encoded.is_some(), "batch should have encoded data");
        debug_assert!(batch.datagrams.is_empty(), "datagrams should be consumed");
        // Drop the batch
    });

    rx.drain().await;

    info!(%local_addr, "Socket sender shutting down");
}

/// Per-socket path context storage
///
/// Each send socket maintains its own packet number space, CCA state, and RTT estimator.
struct SocketPathContexts {
    /// Map from credentials ID to path contexts
    contexts: RefCell<
        std::collections::HashMap<
            credentials::Id,
            Rc<RefCell<socket::channel::PathContext<crate::crypto::awslc::seal::Application>>>,
        >,
    >,
}

impl SocketPathContexts {
    fn new() -> Self {
        Self {
            contexts: RefCell::new(std::collections::HashMap::new()),
        }
    }

    /// Get or create a path context for the given path entry
    fn get_or_insert(
        &self,
        entry: &Arc<PathSecretEntry>,
    ) -> Rc<RefCell<socket::channel::PathContext<crate::crypto::awslc::seal::Application>>> {
        let credentials_id = *entry.id();

        let mut contexts = self.contexts.borrow_mut();
        if let Some(context) = contexts.get(&credentials_id) {
            return context.clone();
        }

        // Create new context - call reusable_sealer() only once
        let (sealer, credentials) = entry.reusable_sealer();

        // Create a new CCA controller using the entry's negotiated MTU
        let cca = congestion::Controller::new(entry.max_datagram_size());

        // Create a new RTT estimator
        let rtt_estimator = s2n_quic_core::recovery::RttEstimator::new(Duration::from_millis(2));

        // Create a new packet number map
        let packet_number_map = s2n_quic_core::packet::number::Map::default();

        let context = socket::channel::PathContext {
            path_secret_entry: entry.clone(),
            sealer,
            credentials,
            next_packet_number: VarInt::ZERO,
            flow_attempt_id_counter: VarInt::ZERO,
            cca,
            rtt_estimator,
            packet_number_map,
            pto: socket::channel::Pto::default(),
            pending_batches: 0,
        };

        let context = Rc::new(RefCell::new(context));
        contexts.insert(credentials_id, context.clone());
        context
    }
}

/// Simple cached path context resolver for demo purposes
///
/// In production, this should use proper LRU caching and idle timers.
pub struct SimplePathContextResolver {
    socket_contexts: Rc<SocketPathContexts>,
}

impl SimplePathContextResolver {
    fn new(socket_contexts: Rc<SocketPathContexts>) -> Self {
        Self { socket_contexts }
    }
}

impl socket::channel::PathContextResolver for SimplePathContextResolver {
    type Sealer = crate::crypto::awslc::seal::Application;

    fn resolve(
        &self,
        entry: &Arc<PathSecretEntry>,
    ) -> Option<Rc<RefCell<socket::channel::PathContext<Self::Sealer>>>> {
        Some(self.socket_contexts.get_or_insert(entry))
    }
}

// ── Socket Creation ────────────────────────────────────────────────────────

/// Creates send sockets with GSO support
pub fn create_send_sockets(
    num_sockets: usize,
    mut bind_addr: SocketAddr,
    gso: features::Gso,
) -> io::Result<Vec<GsoSocket<BusyPoll<std::net::UdpSocket>>>> {
    let mut sockets = Vec::with_capacity(num_sockets);

    // Bind to the address with port 0 to get an ephemeral port
    bind_addr.set_port(0);

    for _ in 0..num_sockets {
        let mut opts = Options::default();
        opts.addr = bind_addr;
        opts.blocking = false;
        opts.send_buffer = Some(200 * 1024 * 1024); // 200MB per socket
        opts.recv_buffer = Some(0);
        let socket = opts.build_udp()?;

        // Wrap with busy poll support then GSO
        let socket = BusyPoll(socket);
        let socket = GsoSocket(socket, gso.clone());
        sockets.push(socket);
    }

    Ok(sockets)
}

/// Creates receive sockets with REUSEPORT for load balancing
pub fn create_recv_sockets(
    num_sockets: usize,
    bind_addr: SocketAddr,
) -> io::Result<Vec<BusyPoll<std::net::UdpSocket>>> {
    let mut sockets = Vec::with_capacity(num_sockets);

    // First socket - binds the address (will get ephemeral port if port is 0)
    let mut opts = Options::default();
    opts.addr = bind_addr;
    if num_sockets > 1 {
        opts.reuse_address = true;
        opts.reuse_port = ReusePort::AfterBind;
    }
    opts.gro = true;
    opts.blocking = false;
    opts.recv_buffer = Some(200 * 1024 * 1024);
    opts.send_buffer = Some(0);
    let first_socket = opts.build_udp()?;
    sockets.push(BusyPoll(first_socket));

    // If we have more than one socket, use REUSEPORT to share the port
    if num_sockets > 1 {
        // Get the actual bound address from the first socket
        let bound_addr = sockets[0].0.local_addr()?;

        assert_ne!(bound_addr.port(), 0);
        opts.reuse_port = ReusePort::BeforeBind;

        // Remaining sockets share the same port
        opts.addr = bound_addr;
        for _ in 1..num_sockets {
            sockets.push(BusyPoll(opts.build_udp()?));
        }
    }

    Ok(sockets)
}

/// Complete bidirectional pipeline setup - the shared infrastructure for a process
pub struct Endpoint {
    /// Input sender for the wheel (producers send batches here)
    pub wheel_input_tx: intrusive_queue::sync::Sender<Batch>,
    /// GSO configuration (for querying max segments)
    pub gso: features::Gso,
    /// Path secrets for the endpoint
    pub path_secret_map: path::secret::Map,
    /// Queue allocator for flow-based routing
    pub queue_allocator: queue::Allocator<StreamMsg, ControlMsg, flow::Handle>,
    /// Acceptor registry for flow initialization
    pub acceptor_registry: acceptor::Registry<FlowInit>,
    /// Endpoint-wide stream ID counter
    pub next_stream_id: std::sync::atomic::AtomicU64,
    /// The port that recv sockets are bound to (advertised to peers via PSK handshake)
    pub data_port: u16,
}

pub struct EndpointConfig<'a, S> {
    pub overall_send_rate: Rate,
    pub per_socket_send_rate: Rate,
    pub spawner: &'a S,
    pub clock: BusyPollClock<TokioClock>,
    pub send_pool: pool::Pool,
    pub recv_pool: pool::Pool,
    pub counters: CounterRegistry,
    /// Path secrets for the endpoint
    pub path_secret_map: path::secret::Map,
    /// GSO configuration for max segments per batch
    pub gso: features::Gso,
    /// Acceptor registry for flow initialization
    pub acceptor_registry: acceptor::Registry<FlowInit>,
    /// When true, emit per-socket queue metrics (`q.send.{id}`) in addition to the aggregate
    pub verbose_socket_metrics: bool,
}

struct SendSocketInfo<S> {
    sender_id: usize,
    socket: S,
}

struct RecvSocketInfo<S> {
    socket_id: usize,
    socket: S,
}

struct Worker<SendSocket, RecvSocket> {
    id: usize,
    send_sockets: Vec<SendSocketInfo<SendSocket>>,
    recv_sockets: Vec<RecvSocketInfo<RecvSocket>>,
    // One datagram handler per worker
    datagram_rx:
        intrusive_queue::sync::Receiver<packet::datagram::decoder::Packet<descriptor::Filled>>,
    // Phase 1: control packets routed by remote sender for verification + ACK recording
    control_rx:
        intrusive_queue::sync::Receiver<packet::control::decoder::Packet<descriptor::Filled>>,
    // Phase 2: verified control packets routed by dest_sender_id for ACK frame processing
    verified_control_rx:
        intrusive_queue::sync::Receiver<packet::control::decoder::Packet<descriptor::Filled>>,
}

/// Sets up a complete bidirectional pipeline with send and receive paths
///
/// Send sockets: wheel -> encoder -> paced sender -> socket
/// Receive sockets: socket -> decoder -> router (for ACKs, etc)
///
/// The receive path can route parsed packets back to specific send workers
/// using the WorkerId routing info.
pub fn setup_endpoint<SendSocket, RecvSocket, G, S>(
    config: EndpointConfig<S>,
    send_sockets: Vec<SendSocket>,
    recv_sockets: Vec<RecvSocket>,
    create_rand: impl Fn() -> G,
) -> Endpoint
where
    SendSocket: socket::send::Socket + Send + Sync + 'static,
    RecvSocket: socket::recv::Socket + Send + 'static,
    G: random::Generator,
    S: crate::stream2::Spawner,
{
    let num_workers = config.spawner.worker_count().saturating_sub(1).max(1);
    let num_send_sockets = send_sockets.len();

    match (
        num_workers.is_power_of_two(),
        num_send_sockets.is_power_of_two(),
    ) {
        (true, true) => setup_endpoint_inner::<_, _, _, _, PowerOfTwoRoute, PowerOfTwoRoute>(
            config,
            send_sockets,
            recv_sockets,
            create_rand,
        ),
        (true, false) => setup_endpoint_inner::<_, _, _, _, PowerOfTwoRoute, ModuloRoute>(
            config,
            send_sockets,
            recv_sockets,
            create_rand,
        ),
        (false, true) => setup_endpoint_inner::<_, _, _, _, ModuloRoute, PowerOfTwoRoute>(
            config,
            send_sockets,
            recv_sockets,
            create_rand,
        ),
        (false, false) => setup_endpoint_inner::<_, _, _, _, ModuloRoute, ModuloRoute>(
            config,
            send_sockets,
            recv_sockets,
            create_rand,
        ),
    }
}

fn setup_endpoint_inner<SendSocket, RecvSocket, G, S, WorkerRoute, SenderIdRoute>(
    config: EndpointConfig<S>,
    send_sockets: Vec<SendSocket>,
    recv_sockets: Vec<RecvSocket>,
    create_rand: impl Fn() -> G,
) -> Endpoint
where
    SendSocket: socket::send::Socket + Send + Sync + 'static,
    RecvSocket: socket::recv::Socket + Send + 'static,
    G: random::Generator,
    S: crate::stream2::Spawner,
    WorkerRoute: SenderRoute,
    SenderIdRoute: SenderRoute,
{
    let EndpointConfig {
        overall_send_rate,
        per_socket_send_rate,
        spawner,
        clock,
        send_pool,
        recv_pool,
        path_secret_map,
        counters,
        gso,
        acceptor_registry,
        verbose_socket_metrics,
    } = config;

    let num_send_sockets = send_sockets.len();
    let sender_id_route = SenderIdRoute::new(num_send_sockets);

    // Create counter registry for instrumentation
    counters.spawn_reporter(Duration::from_secs(1));

    // Create queue allocator for flow-based routing
    let allocator = queue::Allocator::<StreamMsg, ControlMsg, flow::Handle>::new();
    let queue_dispatcher = allocator.dispatcher();

    // Get the control port from the first receive socket (all receive sockets share the same port with REUSEPORT)
    let source_control_port = recv_sockets
        .get(0)
        .and_then(|s| s.local_addr().ok())
        .map(|addr| addr.port())
        .unwrap_or(0);

    // Group send sockets, recv sockets, and worker channels by spawner workers
    let num_workers = spawner.worker_count().saturating_sub(1).max(1);
    let worker_route = WorkerRoute::new(num_workers);

    // Create channel for wheel input from generators
    let (wheel_input_tx, wheel_input_rx) = intrusive_queue::sync::new();

    // Create error channel for failed batches
    let (error_tx, error_rx) = intrusive_queue::sync::new();

    // TODO: Read idle timeout from `PathSecretEntry::idle_timeout()` and use it to evict
    // stale sender/receiver states. Currently we hardcode 60s and only reset the timer on
    // activity, but never actually remove entries when the timer fires.
    let path_idle_timeout = Duration::from_secs(60);

    // Create worker channels (one datagram and one control channel per worker)
    let mut workers = Vec::with_capacity(num_workers);
    let mut datagram_receiver_tx = Vec::with_capacity(num_workers);
    let mut control_packet_tx = Vec::with_capacity(num_workers);
    let mut verified_control_tx = Vec::with_capacity(num_workers);

    // Also create per-worker socket channel infrastructure
    let mut worker_socket_senders = Vec::with_capacity(num_workers);
    let mut worker_socket_receivers = Vec::with_capacity(num_workers);

    for id in 0..num_workers {
        let (datagram_tx, datagram_rx) = intrusive_queue::sync::new();
        let (control_tx, control_rx) = intrusive_queue::sync::new();
        let (verified_tx, verified_rx) = intrusive_queue::sync::new();
        datagram_receiver_tx.push(datagram_tx);
        control_packet_tx.push(control_tx);
        verified_control_tx.push(verified_tx);
        workers.push(Worker {
            id,
            send_sockets: Vec::new(),
            recv_sockets: Vec::new(),
            datagram_rx,
            control_rx,
            verified_control_rx: verified_rx,
        });
    }

    // Build sender_id to worker_id mapping (for control packet routing)
    let mut sender_id_to_worker = Vec::with_capacity(num_send_sockets);

    // Distribute send sockets across workers and create their socket channels
    for (sender_id, socket) in send_sockets.into_iter().enumerate() {
        let worker_idx = sender_id % num_workers;
        sender_id_to_worker.push(worker_idx);
        workers[worker_idx]
            .send_sockets
            .push(SendSocketInfo { sender_id, socket });
    }

    // Create worker-socket channels after we know socket counts per worker
    for worker in &workers {
        let num_sockets = worker.send_sockets.len();
        let (senders, receiver) = worker_socket_channel::new(num_sockets);
        worker_socket_senders.push(senders);
        worker_socket_receivers.push(receiver);
    }

    // Build a flat list of senders for the wheel distributor, maintaining sender_id order
    let mut flat_socket_senders = Vec::with_capacity(num_send_sockets);
    for sender_id in 0..num_send_sockets {
        let worker_idx = sender_id_to_worker[sender_id];
        let socket_idx_in_worker = workers[worker_idx]
            .send_sockets
            .iter()
            .position(|s| s.sender_id == sender_id)
            .unwrap();
        flat_socket_senders.push(worker_socket_senders[worker_idx][socket_idx_in_worker].clone());
    }

    let sender_id_to_worker = Arc::new(sender_id_to_worker);

    // Spawn wheel ticker + distributor on worker 0
    spawner.spawn_local(0, {
        let clock = clock.clone();
        let socket_senders = flat_socket_senders.clone();
        let wheel_input_rx = wheel_input_rx;
        let counters = counters.clone();
        move |mut spawner| {
            info!("Starting wheel worker on worker 0");

            let mut priority_output_txs = Vec::with_capacity(BatchPriority::LEVELS);
            let mut priority_output_rxs = Vec::with_capacity(BatchPriority::LEVELS);

            for i in 0..BatchPriority::LEVELS {
                let (priority_output_tx, priority_output_rx) = intrusive_queue::unsync::new();
                priority_output_txs.push(priority_output_tx);
                let gauge = counters.register_queue_gauge(
                    Box::leak(format!("q.priority.{i}").into_boxed_str()),
                );
                priority_output_rxs
                    .push(GaugedQueue::new(priority_output_rx.into_list_receiver(), gauge));
            }

            let wheel_timer = clock.timer();
            let wheel: Wheel<_, _, _, 1> = Wheel::new(wheel_input_rx, wheel_timer);
            let wheel_gauge = counters.register_queue_gauge("q.wheel");
            spawner.spawn(async move {
                let rx = GaugedQueue::new(wheel, wheel_gauge);
                let rx = channel::Map::new(rx, move |entry: Entry<Batch>| {
                    // Route each wheel output batch into a dedicated priority lane. The
                    // downstream `channel::Priority` receiver polls these outputs in order.
                    let priority_idx = entry.meta.priority.as_index();
                    // SAFETY: batch priority is an enum with values in 0..BatchPriority::LEVELS.
                    let priority_sender =
                        unsafe { priority_output_txs.get_unchecked_mut(priority_idx) };
                    if priority_sender.send(entry).is_err() {
                        tracing::warn!(
                            priority = priority_idx,
                            "Priority output lane is closed; dropping batch"
                        );
                    }
                });

                rx.drain().await;
                info!("Wheel priority router task shutting down");
            });

            // Task 2: Overall bandwidth limiter + sticky routing + round robin distributor
            let rx = channel::Priority::new(priority_output_rxs);
            let rx = Paced::new(rx, clock.clone(), overall_send_rate);

            // Intercept sticky batches (sender_id != MAX) and route them directly
            let rx = channel::FilterMap::new(rx, {
                let socket_senders = socket_senders.clone();
                move |entry: Entry<Batch>| {
                    if entry.meta.sender_id != VarInt::MAX {
                        // Sticky routing - send directly to specific sender
                        let target_idx = entry.meta.sender_id.as_u64() as usize;
                        if target_idx < socket_senders.len() {
                            let _ = socket_senders[target_idx].send_entry(entry);
                        } else {
                            tracing::warn!(
                                sender_id = target_idx,
                                num_senders = socket_senders.len(),
                                "Sticky sender_id out of bounds"
                            );
                        }
                        None // Filtered out - already routed
                    } else {
                        Some(entry) // Pass through to round-robin
                    }
                }
            });

            spawner.spawn(channel::round_robin(rx, socket_senders));
            info!("Finished spawning wheel tasks");
        }
    });

    // Distribute recv sockets across workers
    for (socket_id, socket) in recv_sockets.into_iter().enumerate() {
        let worker_idx = socket_id % num_workers;
        workers[worker_idx]
            .recv_sockets
            .push(RecvSocketInfo { socket_id, socket });
    }

    // Spawn all tasks for each busy poll worker
    for (worker, worker_tx_receiver) in workers.into_iter().zip(worker_socket_receivers) {
        let busy_worker_idx = 1 + worker.id;
        let clock = clock.clone();
        let send_pool = send_pool.clone();
        let recv_pool = recv_pool.clone();
        let datagram_receiver_tx = datagram_receiver_tx.clone();
        let control_packet_tx = control_packet_tx.clone();
        let verified_control_tx = verified_control_tx.clone();
        let error_tx = error_tx.clone();
        let path_secret_map = path_secret_map.clone();
        let acceptor_registry = acceptor_registry.clone();
        let wheel_input_tx = wheel_input_tx.clone();
        let counters = counters.clone();
        let control_generator = create_rand();
        let sender_id_to_worker = sender_id_to_worker.clone();
        let queue_dispatcher = queue_dispatcher.clone();

        spawner.spawn_local(busy_worker_idx, move |mut spawner| {
            // Create per-worker state
            let shared_sender_cache = Rc::new(RefCell::new(SenderStateCache::new(
                path_idle_timeout,
                worker.id,
            )));

            // Map sender_id to path contexts for control packet processing
            // Control worker needs to look up contexts for all sockets on this worker
            let sender_contexts: Rc<RefCell<HashMap<usize, Rc<SocketPathContexts>>>> =
                Rc::new(RefCell::new(HashMap::new()));

            // Create channels for ACKed and lost packets (shared across control worker)
            let (acked_tx, acked_rx) = channel::intrusive_queue::unsync::new();
            let mut acked_tx = acked_tx.into_list_sender();
            let acked_rx = acked_rx.into_list_receiver();
            let (lost_tx, lost_rx) = channel::intrusive_queue::unsync::new();
            let mut lost_tx = lost_tx.into_list_sender();
            let lost_rx = lost_rx.into_list_receiver();

            // Create channel for response packets (ACKs, FlowValidate, FlowReset, etc.)
            let (response_tx, response_rx) = channel::intrusive_queue::unsync::new();
            let mut response_tx = response_tx.into_list_sender();
            let response_rx = response_rx.into_list_receiver();

            // Create channel for PTO wheel input (using PtoAdapter)
            let (pto_wheel_tx, pto_wheel_rx) = channel::intrusive_queue::unsync::new_with_adapter::<
                channel::PtoAdapter<crate::crypto::awslc::seal::Application>,
            >();

            // Spawn PTO wheel processor task
            spawner.spawn({
                let worker_id = worker.id;
                let clock = clock.clone();
                let wheel_input_tx = wheel_input_tx.clone();
                let mut pto_wheel_tx = pto_wheel_tx.clone();
                let pto_probe_counter = counters.register("pkt.pto");

                // Create PTO timing wheel (1µs granularity)
                let pto_wheel_timer = clock.timer();
                let pto_wheel: Wheel<_, _, _, 1> =
                    Wheel::new(pto_wheel_rx.into_list_receiver(), pto_wheel_timer);

                async move {
                    // Drain expired PTO entries and generate probes
                    let rx = channel::FlattenList::new(pto_wheel);
                    let rx = channel::FilterMap::new(rx, move |context_rc: RcPathContext| {
                        let probe_batch =
                            process_pto_timeout(worker_id, context_rc, &clock, &mut pto_wheel_tx);
                        if probe_batch.is_some() {
                            pto_probe_counter.add(1);
                        }
                        probe_batch
                    });

                    // Pump probe batches into the wheel for transmission
                    channel::pump(rx, wheel_input_tx).await;
                    tracing::info!(worker_id, "PTO wheel processor shutting down");
                }
            });

            // Spawn ACKed packet handler with batched completion notifications
            spawner.spawn({
                let ack_gauge = counters.register_queue_gauge("q.ack");
                async move {
                    let rx = GaugedQueue::new(acked_rx, ack_gauge);
                    let rx = channel::CompletionBatcher::new(rx);
                    rx.drain().await;
                }
            });

            // Spawn response packet handler - batch ACKs and flow control responses
            spawner.spawn({
                let wheel_input_tx = wheel_input_tx.clone();
                let response_gauge = counters.register_queue_gauge("q.response");
                let rx = response_rx;
                async move {
                    let rx = GaugedQueue::new(rx, response_gauge);

                    // Batch response packets by peer address for efficient transmission with GSO
                    let rx = channel::RetransmissionBatcher::new(rx);

                    // Pump response batches into the wheel for transmission.
                    // Budget allows draining multiple batches per poll to keep up with
                    // the datagram processor which generates responses synchronously.
                    channel::pump_budgeted(rx, wheel_input_tx, Some(64)).await;
                }
            });

            // Spawn lost packet handler - batch and retransmit
            spawner.spawn({
                let wheel_input_tx = wheel_input_tx.clone();
                let lost_gauge = counters.register_queue_gauge("!q.lost");
                let lost_dgm_counter = counters.register("!pkt.lost.dgm");
                let lost_ctl_counter = counters.register("!pkt.lost.ctl");
                async move {
                    let rx = GaugedQueue::new(lost_rx, lost_gauge);

                    // Filter out control packets - they don't need retransmission
                    let rx =
                        channel::FilterMap::new(rx, |entry: Entry<PartialDatagram>| {
                            match entry.packet_type {
                                packet::datagram::partial::PacketType::Datagram { .. } => {
                                    lost_dgm_counter.add(1);
                                    Some(entry)
                                }
                                packet::datagram::partial::PacketType::Control { .. } => {
                                    lost_ctl_counter.add(1);
                                    tracing::trace!("Skipping control packet retransmission");
                                    None
                                }
                            }
                        });

                    // Batch lost packets by peer address for efficient retransmission with GSO
                    let rx = channel::RetransmissionBatcher::new(rx);

                    // Inspect batches before retransmission
                    let rx = channel::Inspect::new(rx, |batch: &Entry<Batch>| {
                        tracing::debug!(
                            peer_addr = ?batch.meta.peer_addr,
                            count = batch.datagrams.len(),
                            "Retransmitting lost packets"
                        );
                    });

                    // Pump retransmission batches back into the wheel
                    channel::pump(rx, wheel_input_tx).await;
                }
            });

            // Create unsync channels for each socket on this worker
            let mut socket_batch_senders = Vec::with_capacity(worker.send_sockets.len());
            let mut socket_batch_receivers = Vec::with_capacity(worker.send_sockets.len());

            for _ in 0..worker.send_sockets.len() {
                let (tx, rx) = intrusive_queue::unsync::new();
                socket_batch_senders.push(tx.into_list_sender());
                socket_batch_receivers.push(rx.into_list_receiver());
            }

            // Spawn worker dispatcher task that drains from sync channel to unsync channels
            {
                let mut socket_senders = socket_batch_senders;

                spawner.spawn(core::future::poll_fn(move |cx| {
                    // Single lock to drain all socket queues and dispatch to unsync channels
                    worker_tx_receiver.drain_to(&mut socket_senders);

                    cx.waker().wake_by_ref();
                    core::task::Poll::Pending
                }));
            }

            // Spawn send socket tasks for each socket on this worker
            for (socket_info, batch_rx) in
                worker.send_sockets.into_iter().zip(socket_batch_receivers)
            {
                let SendSocketInfo { sender_id, socket } = socket_info;

                // Create per-socket path contexts
                let socket_contexts = Rc::new(SocketPathContexts::new());

                // Register this socket's contexts for control packet processing
                sender_contexts
                    .borrow_mut()
                    .insert(sender_id, socket_contexts.clone());

                let error_tx = error_tx.clone();
                let pool = send_pool.clone();
                let clock = clock.clone();
                let local_addr = socket.local_addr().unwrap();
                let source_sender_id = VarInt::new(sender_id as u64).unwrap();

                // Create channel between Paced and PacketRegistrar
                let (paced_tx, paced_rx) = intrusive_queue::unsync::new();

                // Task 1: Encoder + PacketRegistrar + Paced -> pump to channel
                spawner.spawn({
                    let clock = clock.clone();
                    let send_gauge = if verbose_socket_metrics {
                        counters.register_queue_gauge(
                            Box::leak(format!("q.send.{sender_id}").into_boxed_str()),
                        )
                    } else {
                        counters.register_queue_gauge("q.send")
                    };
                    async move {
                        // Build the channel adapter pipeline with timing instrumentation
                        let rx = GaugedQueue::new(batch_rx, send_gauge);
                        let rx = channel::Timing::new(rx, "flatten");

                        let resolver = SimplePathContextResolver::new(socket_contexts);
                        let rx = channel::PathResolver::new(rx, resolver, error_tx);
                        let rx = channel::Timing::new(rx, "path_resolver");

                        let rx =
                            channel::Encoder::new(rx, pool, source_control_port, source_sender_id);
                        let rx = channel::Timing::new(rx, "encoder");

                        let rx = channel::PacketRegistrar::new(rx, clock.clone());
                        let rx = channel::Timing::new(rx, "packet_registrar");

                        channel::pump(rx, paced_tx).await;
                        tracing::info!(sender_id, "Paced pump shutting down");
                    }
                });

                // Task 2: Channel -> Socket
                spawner.spawn({
                    let tx_counter = counters.register("socket.tx");
                    let tx_bytes_counter = counters.register("socket.tx:bytes");
                    async move {
                        let rx = paced_rx;

                        let rx = Paced::new(rx, clock.clone(), per_socket_send_rate);
                        let rx = channel::Timing::new(rx, "paced");

                        // Count bytes right before socket transmission
                        let rx = channel::Inspect::new(rx, move |batch: &Entry<Batch<_>>| {
                            use channel::ByteCost;
                            tx_counter.add(1);
                            tx_bytes_counter.add(batch.byte_cost());
                        });

                        batch_sender(socket, rx).await;
                        info!(sender_id, ?local_addr, "Socket sender shutting down");
                    }
                });
            }

            // Phase 1: Verify control packets and forward to dest worker.
            //
            // Control packets are routed here by remote sender hash (same as datagrams),
            // so the SenderState cache is shared with the datagram path — no duplicates.
            // After verification and ACK recording, packets are forwarded to the worker
            // that owns the dest_sender_id for ACK frame processing (phase 2).
            {
                let worker_id = worker.id;
                let control_rx = worker.control_rx;
                let path_secret_map = path_secret_map.clone();
                let shared_sender_cache = shared_sender_cache.clone();
                let clock = clock.clone();
                let recv_control_counter = counters.register("rx.control_pkt");
                let verified_control_tx = verified_control_tx.clone();
                let sender_id_to_worker = sender_id_to_worker.clone();
                let control_input_gauge = counters.register_queue_gauge("q.control");

                spawner.spawn(async move {
                    // Verify and record in ACK space
                    let rx = GaugedQueue::new(control_rx, control_input_gauge);
                    let rx = Map::new(rx, {
                        let clock = clock.clone();
                        let recv_control_counter = recv_control_counter.clone();
                        move |packet: Entry<
                            packet::control::decoder::Packet<descriptor::Filled>,
                        >| {
                            recv_control_counter.add(1);
                            process_control(
                                packet,
                                &mut shared_sender_cache.borrow_mut(),
                                &path_secret_map,
                                &clock,
                            )
                        }
                    });
                    assert_receiver(&rx);

                    let rx = InspectErr::new(rx, move |err| match err {
                        ProcessControlError::PeerStateLookup {
                            credentials,
                            control_out,
                        } => {
                            // TODO transmit this
                            let _ = control_out;
                            tracing::warn!(
                                worker_id,
                                ?credentials,
                                "Failed to get or create peer state for control packet"
                            );
                        }
                        ProcessControlError::Verification {
                            credentials,
                            packet_number,
                        } => {
                            tracing::debug!(
                                worker_id,
                                ?credentials,
                                pn = packet_number.as_u64(),
                                "Failed to verify control packet - authentication failed"
                            );
                        }
                        ProcessControlError::MissingSenderId => {
                            tracing::warn!(
                                worker_id,
                                "Control packet missing source_sender_id in routing info"
                            );
                        }
                    });
                    assert_receiver(&rx);

                    // Forward verified packets to the worker that owns dest_sender_id
                    let rx = Map::new(rx, move |packet| {
                        let Some(dest_sender_id) = packet.routing_info().dest_sender_id() else {
                            tracing::warn!(
                                worker_id,
                                "Verified control packet without dest_sender_id"
                            );
                            return;
                        };
                        let dest_sender_id = dest_sender_id.as_u64() as usize;

                        let Some(&dest_worker) = sender_id_to_worker.get(dest_sender_id) else {
                            tracing::warn!(worker_id, dest_sender_id, "Unknown dest_sender_id");
                            return;
                        };

                        let Some(sender) = verified_control_tx.get(dest_worker) else {
                            return;
                        };
                        let _ = sender.send_entry(packet);
                    });

                    rx.drain_budgeted(Some(64)).await;
                    tracing::info!(worker_id, "Control verification worker shutting down");
                });
            }

            // Phase 2: Process ACK frames from verified control packets.
            //
            // These arrive routed by dest_sender_id, so we have access to the
            // correct PathContext for the local send socket being acknowledged.
            {
                let worker_id = worker.id;
                let verified_control_rx = worker.verified_control_rx;
                let sender_contexts = sender_contexts.clone();
                let clock = clock.clone();
                let mut generator = control_generator;
                let verified_control_gauge = counters.register_queue_gauge("q.verified_control");

                spawner.spawn(async move {
                    // Process ACK frames and update send state using sender_contexts
                    let rx = GaugedQueue::new(verified_control_rx, verified_control_gauge);
                    let rx = Map::new(rx, {
                        let clock = clock.clone();
                        move |mut packet: Entry<
                            packet::control::decoder::Packet<descriptor::Filled>,
                        >| {
                            // Extract dest_sender_id from routing info
                            let Some(dest_sender_id) = packet.routing_info().dest_sender_id()
                            else {
                                tracing::warn!(
                                    worker_id,
                                    "Control packet without dest_sender_id - cannot process ACK"
                                );
                                return;
                            };
                            let dest_sender_id = dest_sender_id.as_u64() as usize;

                            // Look up the socket's path contexts
                            let sender_contexts_ref = sender_contexts.borrow();
                            let Some(socket_contexts) = sender_contexts_ref.get(&dest_sender_id)
                            else {
                                tracing::warn!(
                                    worker_id,
                                    dest_sender_id,
                                    "No socket contexts for sender_id"
                                );
                                return;
                            };

                            // Get the path context for this peer
                            let ack_credentials = *packet.credentials();
                            let credentials_id = ack_credentials.id;

                            let contexts = socket_contexts.contexts.borrow_mut();
                            let Some(context_rc) = contexts.get(&credentials_id) else {
                                tracing::warn!(
                                    worker_id,
                                    dest_sender_id,
                                    ?credentials_id,
                                    "No path context for credentials"
                                );
                                return;
                            };

                            let mut context = context_rc.borrow_mut();
                            process_control_frames(
                                worker_id,
                                &mut packet,
                                &mut context,
                                &mut acked_tx,
                                &mut lost_tx,
                                &clock,
                                &mut generator,
                            );

                            // TODO: Clear ACK ranges for packets that have been acknowledged
                            // When we receive an ACK from the peer, we should call
                            // sender_state.ack_space.on_largest_delivered_packet(largest_delivered)
                            // to prevent re-sending ACKs for packets the peer has confirmed.
                            // This requires access to shared_sender_cache here.
                        }
                    });
                    assert_receiver::<()>(&rx);

                    rx.drain().await;
                    tracing::info!(worker_id, "Control worker shutting down");
                });
            }

            // Spawn ACK worker (datagram processor) for this worker
            {
                let worker_id = worker.id;
                let datagram_rx = worker.datagram_rx;
                let path_secret_map = path_secret_map.clone();
                let clock = clock.clone();
                let wheel_input_tx = wheel_input_tx.clone();
                let recv_data_counter = counters.register("rx.data_pkt");
                let process_datagram_counters = ProcessDatagramCounters::new(&counters);
                let queue_dispatcher = queue_dispatcher.clone();
                let datagram_input_gauge = counters.register_queue_gauge("q.datagram");

                spawner.spawn(async move {
                    // Process datagrams for ACK generation
                    let rx = GaugedQueue::new(datagram_rx, datagram_input_gauge);

                    let rx = channel::Map::new(rx, {
                        let recv_data_counter = recv_data_counter.clone();
                        let shared_sender_cache = shared_sender_cache.clone();
                        let path_secret_map = path_secret_map.clone();
                        let acceptor_registry = acceptor_registry.clone();
                        let wheel_input_tx = wheel_input_tx.clone();
                        let clock = clock.clone();
                        let process_datagram_counters = process_datagram_counters.clone();
                        let mut queue_dispatcher = queue_dispatcher.clone();
                        move |packet: Entry<
                            packet::datagram::decoder::Packet<descriptor::Filled>,
                        >| {
                            recv_data_counter.add(1);
                            process_datagram(
                                packet,
                                &mut shared_sender_cache.borrow_mut(),
                                &path_secret_map,
                                &acceptor_registry,
                                &wheel_input_tx,
                                &mut response_tx,
                                &mut queue_dispatcher,
                                &clock,
                                sender_id_route,
                                &process_datagram_counters,
                            )
                        }
                    });
                    let rx = InspectErr::new(rx, move |err| match err {
                        ProcessError::PeerStateLookup {
                            credentials,
                            control_out,
                        } => {
                            // TODO transmit this
                            let _ = control_out;
                            tracing::warn!(
                                worker_id,
                                ?credentials,
                                "Failed to get or create peer state"
                            );
                        }
                        ProcessError::Decryption {
                            credentials,
                            packet_number,
                        } => {
                            tracing::debug!(
                                worker_id,
                                ?credentials,
                                pn = packet_number.as_u64(),
                                "Failed to decrypt packet - authentication failed"
                            );
                        }
                        ProcessError::Duplicate {
                            credentials,
                            packet_number,
                        } => {
                            tracing::trace!(
                                worker_id,
                                ?credentials,
                                pn = packet_number.as_u64(),
                                "Duplicate packet filtered"
                            );
                        }
                        ProcessError::MissingSenderId => {
                            tracing::warn!(
                                worker_id,
                                "Packet missing source_sender_id in routing info"
                            );
                        }
                    });

                    rx.drain_budgeted(Some(64)).await;

                    tracing::info!(worker_id, "Datagram processor shutting down");
                });
            }

            // Spawn recv socket tasks for this worker
            for recv_socket_info in worker.recv_sockets {
                let RecvSocketInfo { socket_id, socket } = recv_socket_info;
                let clock = clock.clone();
                let recv_pool = recv_pool.clone();
                let datagram_receiver_tx = datagram_receiver_tx.clone();
                let control_packet_tx = control_packet_tx.clone();

                // Create local channels for datagram and control packet processing
                let (datagram_tx, datagram_rx) = intrusive_queue::unsync::new();
                let (control_tx, control_rx) = intrusive_queue::unsync::new();

                let local_addr = socket.local_addr().unwrap();

                // Spawn socket receiver task
                spawner.spawn({
                    let recv_counter = counters.register("socket.rx");
                    let recv_bytes_counter = counters.register("socket.rx:bytes");
                    let decode_error_counter = counters.register("!rx.decode_err");
                    async move {
                        // Build the receive pipeline
                        let rx = SocketReceiver::new(socket, recv_pool);
                        // let rx = Paced::new(rx, clock.clone(), overall_send_rate);

                        let rx = InspectErr::new(rx, |err| {
                            tracing::warn!(socket_id, %err, "Socket recv error");
                        });
                        let rx = FlattenSegments::new(rx);

                        // Track received bytes
                        let rx = channel::Inspect::new(rx, move |segment: &descriptor::Filled| {
                            recv_counter.add(1);
                            recv_bytes_counter.add(segment.len() as u64);
                        });

                        let router = ChannelRouter {
                            datagram_tx,
                            control_tx,
                            decode_error_counter,
                        };
                        let pipeline = RouterAdapter::new(rx, router);

                        pipeline.drain().await;

                        info!(socket_id, ?local_addr, "Socket receiver shutting down");
                    }
                });

                // Spawn datagram router task
                spawner.spawn(async move {
                    let rx = Map::new(
                        datagram_rx,
                        move |packet: Entry<
                            packet::datagram::decoder::Packet<descriptor::Filled>,
                        >| {
                            let credentials = packet.credentials();
                            let routing_info = packet.routing_info();
                            let Some(source_sender_id) = routing_info.source_sender_id() else {
                                tracing::warn!(
                                    socket_id,
                                    "Datagram without source_sender_id - cannot route"
                                );
                                return;
                            };
                            let worker_id = worker_route.worker_id(credentials, source_sender_id);
                            let _ = datagram_receiver_tx[worker_id].send_entry(packet);
                        },
                    );
                    rx.drain().await;
                    tracing::info!(socket_id, "Datagram router shutting down");
                });

                // Spawn control packet router task
                spawner.spawn(async move {
                    let rx = Map::new(
                        control_rx,
                        move |packet: Entry<
                            packet::control::decoder::Packet<descriptor::Filled>,
                        >| {
                            let credentials = packet.credentials();
                            let Some(source_sender_id) = packet.routing_info().source_sender_id()
                            else {
                                tracing::warn!(
                                    socket_id,
                                    "Control packet without source_sender_id routing info"
                                );
                                return;
                            };
                            let worker_id = worker_route.worker_id(credentials, source_sender_id);
                            let Some(sender) = control_packet_tx.get(worker_id) else {
                                return;
                            };
                            let _ = sender.send_entry(packet);
                        },
                    );
                    rx.drain().await;
                    tracing::info!(socket_id, "Control router shutting down");
                });
            }
        });
    }

    // Spawn error handler to track failed batches
    // TODO make this not tokio
    tokio::spawn({
        let error_counter = counters.register("!tx.path_err");
        async move {
            let rx = error_rx;
            let rx = channel::Map::new(rx, move |batch: Entry<Batch>| {
                error_counter.add(1);
                tracing::warn!(
                    peer_addr = ?batch.meta.peer_addr,
                    total_bytes = batch.meta.total_bytes,
                    num_datagrams = batch.datagrams.len(),
                    "Batch failed path resolution"
                );
            });
            rx.drain().await;
            tracing::info!("Error handler shutting down");
        }
    });

    Endpoint {
        wheel_input_tx,
        gso,
        path_secret_map,
        queue_allocator: allocator,
        acceptor_registry,
        next_stream_id: std::sync::atomic::AtomicU64::new(0),
        data_port: source_control_port,
    }
}

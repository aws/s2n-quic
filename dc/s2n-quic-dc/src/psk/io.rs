// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{client, probe, server};
use crate::path::secret::{self, map::HandshakeProbeOutcome};
use rand::RngExt;
use s2n_quic::{
    provider::{
        dc::{ConfirmComplete, MtuConfirmComplete},
        event::Subscriber as Sub,
        tls::Provider as Prov,
    },
    server::Name,
};
use s2n_quic_core::{endpoint::Type, inet::SocketAddress};
use s2n_quic_dc_metrics::TaskMonitor;
use std::{
    any::Any,
    hash::BuildHasher,
    io,
    net::SocketAddr,
    sync::{
        atomic::{AtomicU16, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tokio::{
    runtime::Runtime,
    sync::{Notify, OwnedSemaphorePermit, Semaphore, SemaphorePermit},
    time::Instant as TokioInstant,
};

pub use crate::stream::DEFAULT_IDLE_TIMEOUT;
pub const DEFAULT_MAX_DATA: u64 = 1u64 << 25;
pub const DEFAULT_BASE_MTU: u16 = 1450;
#[cfg(target_os = "linux")]
pub const DEFAULT_MTU: u16 = 8940;
#[cfg(not(target_os = "linux"))]
pub const DEFAULT_MTU: u16 = DEFAULT_BASE_MTU;
/// Jitter PTO probes by 33% to prevent synchronized timeouts across multiple connections
pub const DEFAULT_PTO_JITTER_PERCENTAGE: u8 = 33;
const DEFAULT_INITIAL_RTT: Duration = Duration::from_millis(1);
const DC_QUIC_VERSION: u32 = 0;
/// Application error codes the client uses to close a connection whose dcQUIC handshake did not
/// complete. Both must be non-zero so the close is emitted as an application `CONNECTION_CLOSE`
/// rather than the clean, no-error close produced by dropping the connection handle.
/// Distinct codes let the peer/operator tell the two failure modes apart.
///
/// `ConfirmComplete::wait_ready` reported an error before the dc handshake completed.
const DC_HANDSHAKE_INCOMPLETE_ERROR: u32 = 1;
/// `ConfirmComplete::wait_ready` did not resolve before the handshake deadline elapsed.
const DC_HANDSHAKE_TIMEOUT_ERROR: u32 = 2;

/// Number of threads used to make progress on the TLS handshake
pub const DEFAULT_THREAD_COUNT: usize = 0;

const BUFFER_SIZE: usize = 16 * 1024;

pub type Error = Box<dyn std::error::Error + Send + Sync + 'static>;

pub type Result<T = (), E = Error> = core::result::Result<T, E>;

struct TokioExecutor {
    runtime: Runtime,
    monitor: Option<TaskMonitor>,
}
impl s2n_quic::provider::tls::offload::Executor for TokioExecutor {
    fn spawn(&self, task: impl core::future::Future<Output = ()> + Send + 'static) {
        if let Some(monitor) = &self.monitor {
            self.runtime.spawn(monitor.instrument(task));
        } else {
            self.runtime.spawn(task);
        }
    }
}
#[derive(Clone)]
struct DCExporter {
    map: secret::Map,
    dc_version: u32,
    endpoint_type: s2n_quic_core::endpoint::Type,
}
impl s2n_quic::provider::tls::offload::ExporterHandler for DCExporter {
    fn on_tls_exporter_ready(
        &self,
        session: &impl s2n_quic_core::crypto::tls::TlsSession,
    ) -> Option<Box<dyn Any + Send>> {
        let result = crate::path::secret::map::handshake::on_path_secrets_ready(
            self.dc_version,
            self.endpoint_type,
            &self.map,
            session,
        );

        let boxed_result: Box<dyn Any + Send> = Box::new(result);
        Some(boxed_result)
    }

    fn on_client_application_params(
        &mut self,
        client_params: s2n_quic_core::crypto::tls::ApplicationParameters,
        server_params: &mut Vec<u8>,
    ) -> Option<std::result::Result<(), s2n_quic_core::transport::Error>> {
        Some(s2n_quic_core::dc::append_dc_versions(
            client_params,
            server_params,
        ))
    }
}

pub struct Server {
    server: s2n_quic::Server,
}

impl Server {
    pub fn bind<
        Provider: Prov + Send + Sync + 'static,
        Subscriber: Sub + Send + Sync + 'static,
        Event: s2n_quic::provider::event::Subscriber,
    >(
        addr: SocketAddr,
        map: secret::Map,
        tls_materials_provider: Provider,
        subscriber: Subscriber,
        builder: server::Builder<Event>,
    ) -> Result<Self, Error> {
        // If the initial packet exceeds base MTU, s2n-quic's ability to recover from losing that
        // packet is impaired on both client and server. This is especially true if the ClientHello
        // is larger than the base MTU.
        //
        // We are turning off probing fully (base = initial = max MTU) while we work through
        // improved test coverage.
        let io = s2n_quic::provider::io::default::Builder::default()
            .with_receive_address(addr)?
            .with_internal_recv_buffer_size(BUFFER_SIZE)?
            .with_base_mtu(DEFAULT_BASE_MTU.min(builder.mtu))?
            .with_initial_mtu(DEFAULT_BASE_MTU.min(builder.mtu))?
            .with_max_mtu(DEFAULT_BASE_MTU.min(builder.mtu))?
            .build()?;
        let io = probe::Provider::new(io);

        let initial_max_data = builder.initial_data_window.unwrap_or_else(|| {
            // default to only receive 10 packet worth before the application accepts the connection
            builder.mtu as u64 * 10
        });

        let connection_limits = s2n_quic::provider::limits::Limits::new()
            .with_max_idle_timeout(builder.max_idle_timeout)?
            .with_data_window(initial_max_data)?
            // After the connection is established we increase the data window to the configured value
            .with_bidirectional_local_data_window(builder.data_window)?
            .with_bidirectional_remote_data_window(initial_max_data)?
            .with_pto_jitter_percentage(builder.pto_jitter_percentage)?
            .with_initial_round_trip_time(DEFAULT_INITIAL_RTT)?;

        let event = ((ConfirmComplete, MtuConfirmComplete), subscriber);

        macro_rules! build_and_start {
            ($tls:expr, $limits:expr, $io:expr) => {{
                let s = s2n_quic::Server::builder()
                    .with_io($io)?
                    .with_connection_close_formatter(crate::connection_close::TransparentTransport)?
                    .with_limits($limits)?
                    .with_dc(map.clone())?
                    .with_event((event, builder.event_subscriber))?
                    .with_tls($tls)?;
                #[cfg(any(test, feature = "testing"))]
                let started = if let Some(limiter) = builder.endpoint_limits {
                    s.with_endpoint_limits(limiter)?.start()?
                } else {
                    s.start()?
                };
                #[cfg(not(any(test, feature = "testing")))]
                let started = s.start()?;
                started
            }};
        }

        let server = if builder.thread_offload_count > 0 {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                // Hs=handshake, s=server, offload
                .thread_name("hs-s-offload")
                .worker_threads(builder.thread_offload_count)
                .enable_all()
                .build()?;

            let monitor = builder
                .registry
                .map(|registry| registry.register_task_monitor("HsOffload"));

            let tls = s2n_quic::provider::tls::offload::OffloadBuilder::new()
                .with_endpoint(tls_materials_provider)
                .with_exporter(DCExporter {
                    dc_version: DC_QUIC_VERSION,
                    endpoint_type: Type::Server,
                    map: map.clone(),
                })
                .with_executor(TokioExecutor { runtime, monitor })
                .build();

            // We need packet storage when offloading is turned on due to this issue:
            // https://github.com/aws/s2n-quic/issues/2601. The size needs to be large enough
            // to store a packet with the given MTU.
            let connection_limits =
                connection_limits.with_packet_buffer_size(DEFAULT_MTU as u32)?;

            build_and_start!(tls, connection_limits, io)
        } else {
            build_and_start!(tls_materials_provider, connection_limits, io)
        };

        Ok(Self { server })
    }

    #[allow(dead_code)]
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.server.local_addr()
    }
}

pub(super) async fn server<
    Provider: Prov + Send + Sync + 'static,
    Subscriber: Sub + Send + Sync + 'static,
    Event: s2n_quic::provider::event::Subscriber,
>(
    address: SocketAddr,
    map: secret::Map,
    builder: server::Builder<Event>,
    tls_materials_provider: Provider,
    subscriber: Subscriber,
    on_ready: tokio::sync::oneshot::Sender<Result<SocketAddr, Error>>,
) {
    let mut server = match Server::bind::<Provider, Subscriber, Event>(
        address,
        map.clone(),
        tls_materials_provider,
        subscriber,
        builder,
    ) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("failed to bind server to {:?}: {:?}", address, e);
            let _ = on_ready.send(Err(e));
            // Bail early, we're failing startup.
            return;
        }
    };

    match server.local_addr() {
        Ok(addr) => {
            let _ = on_ready.send(Ok(addr));
        }
        Err(err) => {
            let _ = on_ready.send(Err(err.into()));
            // Bail early, we're failing startup.
            return;
        }
    }

    while let Some(mut connection) = server.server.accept().await {
        let map_clone = map.clone();
        tokio::spawn(async move {
            // The accepted connection must remain open until the client has finished inserting
            // the entry into its map. The client indicates this by sending a ConnectionClose
            // when it is done.
            //
            // A 10 second timeout is specified to avoid spawned tasks piling up when the
            // ConnectionClose from the client is lost. This timeout covers both the dc handshake
            // confirmation and MTU probing completion.
            let result = tokio::time::timeout(Duration::from_secs(10), async {
                // FIXME: add more logging information if the subscriber is not registered with the endpoint.
                if ConfirmComplete::wait_ready(&mut connection).await.is_ok() {
                    MtuConfirmComplete::wait_ready(&mut connection).await;
                }
            })
            .await;

            // Emit event if timeout occurred
            if result.is_err() {
                if let Ok(peer_address) = connection.remote_addr() {
                    map_clone.on_dc_connection_timeout(&peer_address);
                }
            }
        });
    }

    // accept() returning None means the s2n-quic endpoint has shut down. New
    // path secrets will no longer be provisioned, but other components of the
    // process (e.g. SaltyLibMetrics, the stream acceptor) may continue to
    // appear healthy, leaving operators with no signal that handshakes are
    // silently failing. Log loudly so this condition is observable.
    tracing::error!("QUIC handshake server accept loop exited unexpectedly");
}

#[derive(Clone)]
pub struct Client {
    client: s2n_quic::Client,
    map: secret::Map,
    queue: Arc<HandshakeQueue>,
}

impl Client {
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.client.local_addr()
    }

    pub fn bind<
        Provider: Prov + Send + Sync + 'static,
        Subscriber: Sub + Send + Sync + 'static,
        Event: s2n_quic::provider::event::Subscriber,
    >(
        addr: SocketAddr,
        map: secret::Map,
        tls_materials_provider: Provider,
        subscriber: Subscriber,
        builder: client::Builder<Event>,
    ) -> Result<Self, Error> {
        // For MTU configuration, see the comment on Server's io configuration.
        let io = s2n_quic::provider::io::default::Builder::default()
            .with_receive_address(addr)?
            .with_base_mtu(DEFAULT_BASE_MTU.min(builder.mtu))?
            .with_initial_mtu(DEFAULT_BASE_MTU.min(builder.mtu))?
            .with_max_mtu(DEFAULT_BASE_MTU.min(builder.mtu))?
            .with_internal_recv_buffer_size(BUFFER_SIZE)?
            .build()?;

        let client = s2n_quic::Client::builder().with_io(io)?;

        let connection_limits = s2n_quic::provider::limits::Limits::new()
            .with_max_idle_timeout(builder.max_idle_timeout)?
            .with_data_window(builder.data_window)?
            .with_bidirectional_local_data_window(builder.data_window)?
            .with_bidirectional_remote_data_window(builder.data_window)?
            .with_pto_jitter_percentage(builder.pto_jitter_percentage)?
            .with_initial_round_trip_time(DEFAULT_INITIAL_RTT)?
            // Packet buffering on the client avoids dropping LossRecoveryProbing handshake frames
            //
            // This is primarily needed with large ServerHellos (e.g., with PQ), but should be
            // harmless even without it.
            .with_packet_buffer_size(DEFAULT_MTU as u32)?;

        let event = ((ConfirmComplete, MtuConfirmComplete), subscriber);

        let client = client
            .with_connection_close_formatter(crate::connection_close::TransparentTransport)?
            .with_limits(connection_limits)?
            .with_dc(map.clone())?
            .with_event((event, builder.event_subscriber))?
            .with_tls(tls_materials_provider)?
            .start()?;

        Ok(Self {
            client,
            map: map.clone(),
            queue: Arc::new(HandshakeQueue::new(builder.handshake_queue)),
        })
    }

    pub(super) async fn connect(
        &self,
        peer: SocketAddr,
        reason: HandshakeReason,
        server_name: Name,
    ) -> Result<(), HandshakeFailed> {
        self.queue
            .clone()
            .handshake(&self.client, &self.map, peer, reason, server_name)
            .await
    }
}

#[cfg(test)]
impl Client {
    /// Returns true if there's a pending handshake entry for this peer.
    /// This is only available in test builds.
    pub fn has_pending_entry(&self, peer: SocketAddr) -> bool {
        let peer: SocketAddress = peer.into();
        let peer_hash = self.queue.hasher.hash_one(peer);
        let guard = self.queue.inner.lock().unwrap();
        guard.table.find(peer_hash, |e| e.peer == peer).is_some()
    }
}

struct Entry {
    peer: SocketAddress,
    handshaker: tokio::sync::OnceCell<Result<(), HandshakeFailed>>,
    by_reason: [AtomicU16; REASON_COUNT],
}

#[derive(Default)]
struct HandshakeQueueInner {
    table: hashbrown::HashTable<Arc<Entry>>,
}

struct InflightPermit {
    permit: Option<OwnedSemaphorePermit>,
    capacity_changed: Arc<Notify>,
}

impl Drop for InflightPermit {
    fn drop(&mut self) {
        drop(self.permit.take());
        self.capacity_changed.notify_waiters();
    }
}

struct StartPermit<'a> {
    permit: Option<SemaphorePermit<'a>>,
    capacity_changed: Arc<Notify>,
}

impl Drop for StartPermit<'_> {
    fn drop(&mut self) {
        drop(self.permit.take());
        self.capacity_changed.notify_waiters();
    }
}

type HandshakeCapacity<'a> = (InflightPermit, StartPermit<'a>);

pub(crate) struct HandshakeQueueConfig {
    /// Upper bound on the jitter delay after a successful handshake before allowing
    /// another handshake with the same peer.
    pub(crate) success_jitter: Duration,
    /// Upper bound on the jitter delay after a failed handshake before allowing
    /// another handshake with the same peer.
    pub(crate) error_jitter: Duration,
    /// Maximum number of TLS handshakes that can be started concurrently.
    ///
    /// TLS handshakes have high CPU cost (~1ms) which stalls out the endpoint, so we
    /// don't want too many to build up at the same time since that increases baseline
    /// latency ~linearly. For example, 5 translates to ~5ms avg handshake latency.
    pub(crate) start_limit: usize,
    /// Maximum number of in-flight handshake connections.
    ///
    /// Keeping this bounded helps avoid unbounded work ongoing in s2n-quic (which
    /// implies unbounded packet transmit/receive work).
    pub(crate) inflight_limit: usize,
    /// Optional timeout for a pre-handshake dcQUIC liveness probe.
    pub(crate) probe_timeout: Option<Duration>,
    /// Maximum number of concurrent liveness probes.
    pub(crate) probe_limit: usize,
    pub(crate) await_dedup_removal: bool,
}

impl Default for HandshakeQueueConfig {
    fn default() -> Self {
        Self {
            success_jitter: Duration::from_secs(60),
            error_jitter: Duration::from_secs(120),
            start_limit: 5,
            inflight_limit: 750,
            probe_timeout: None,
            probe_limit: 0,
            await_dedup_removal: false,
        }
    }
}

/// One deduplicated handshake per peer moves through these states:
///
/// 1. Wait for capacity. With probing enabled, the first waiter waits for handshake capacity
///    directly. Once there is contention, waiters may probe; a probe holds only a
///    `limiter_probe` permit while sending and waiting for a response. Newly available handshake
///    capacity can interrupt a probe.
/// 2. Acquire `limiter_inflight`, then `limiter_start`. Acquiring the second may wait while the
///    first is held. Both permits are held during connection setup and handshake confirmation.
/// 3. After handshake confirmation, release `limiter_start`. Hold `limiter_inflight` until MTU
///    confirmation or its deadline, then release it before the deduplication delay.
///
/// Failed or cancelled attempts release any held permits through their drop guards. Permit drops
/// notify probing waiters to retry capacity; waiter-count changes notify a lone waiter to start
/// probing when contention appears. Those notifications are registered before checking capacity
/// and waiter count so a transition between a check and a wait cannot be missed.
struct HandshakeQueue {
    inner: Mutex<HandshakeQueueInner>,
    limiter_start: Semaphore,
    limiter_inflight: Arc<Semaphore>,
    limiter_probe: Semaphore,
    capacity_changed: Arc<Notify>,
    probe_waiters: AtomicUsize,
    probe_waiters_changed: Notify,
    #[cfg(test)]
    probe_observer:
        Mutex<Option<tokio::sync::mpsc::UnboundedSender<(SocketAddr, HandshakeProbeOutcome)>>>,
    success_jitter: Duration,
    error_jitter: Duration,
    probe_timeout: Option<Duration>,
    await_dedup_removal: bool,
    hasher: std::collections::hash_map::RandomState,
}

impl HandshakeQueue {
    fn new(config: HandshakeQueueConfig) -> Self {
        let probe_timeout = if config.probe_limit == 0 {
            None
        } else {
            config.probe_timeout
        };
        HandshakeQueue {
            limiter_start: Semaphore::new(config.start_limit),
            limiter_inflight: Arc::new(Semaphore::new(config.inflight_limit)),
            limiter_probe: Semaphore::new(config.probe_limit),
            capacity_changed: Arc::new(Notify::new()),
            probe_waiters: AtomicUsize::new(0),
            probe_waiters_changed: Notify::new(),
            #[cfg(test)]
            probe_observer: Mutex::new(None),
            success_jitter: config.success_jitter,
            inner: Default::default(),
            hasher: Default::default(),
            error_jitter: config.error_jitter,
            probe_timeout,
            await_dedup_removal: config.await_dedup_removal,
        }
    }

    fn try_acquire_capacity(&self) -> Option<HandshakeCapacity<'_>> {
        // Do not wrap or notify for a partial acquisition: those permits were already available,
        // so dropping one after the other limiter rejects us is not a new capacity transition.
        let inflight = self.limiter_inflight.clone().try_acquire_owned().ok()?;
        let start = match self.limiter_start.try_acquire() {
            Ok(start) => start,
            Err(_) => {
                drop(inflight);
                return None;
            }
        };
        let inflight = InflightPermit {
            permit: Some(inflight),
            capacity_changed: self.capacity_changed.clone(),
        };
        let start = StartPermit {
            permit: Some(start),
            capacity_changed: self.capacity_changed.clone(),
        };
        Some((inflight, start))
    }

    async fn acquire_capacity(&self) -> Option<HandshakeCapacity<'_>> {
        let inflight = InflightPermit {
            permit: Some(self.limiter_inflight.clone().acquire_owned().await.ok()?),
            capacity_changed: self.capacity_changed.clone(),
        };
        let start = StartPermit {
            permit: Some(self.limiter_start.acquire().await.ok()?),
            capacity_changed: self.capacity_changed.clone(),
        };
        Some((inflight, start))
    }

    fn register_probe_waiter(self: &Arc<Self>) -> (ProbeWaiterGuard, usize) {
        let waiter_count = self.probe_waiters.fetch_add(1, Ordering::AcqRel) + 1;
        self.probe_waiters_changed.notify_waiters();
        (
            ProbeWaiterGuard {
                queue: self.clone(),
            },
            waiter_count,
        )
    }

    fn on_probe_complete(
        &self,
        map: &secret::Map,
        peer: SocketAddr,
        latency: Duration,
        outcome: HandshakeProbeOutcome,
    ) {
        map.on_dc_handshake_probe(&peer, latency, outcome);
        #[cfg(test)]
        if let Some(observer) = self
            .probe_observer
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .as_ref()
        {
            let _ = observer.send((peer, outcome));
        }
    }

    /// Acquires handshake capacity while using probes only when multiple pending handshakes need
    /// to be ranked. Newly available capacity takes precedence over unfinished probes so the
    /// endpoint does not sit idle; completed probes queue for capacity in result order.
    async fn acquire_prioritized_capacity<'a>(
        self: &'a Arc<Self>,
        map: &secret::Map,
        peer: SocketAddr,
        timeout: Duration,
    ) -> Option<(HandshakeCapacity<'a>, Duration)> {
        let queue_start = std::time::Instant::now();
        let (waiter, waiter_count) = self.register_probe_waiter();
        let mut can_try_capacity = waiter_count == 1;
        let mut probe_started = false;
        let probe = async {
            let Ok(permit_probe) = self.limiter_probe.acquire().await else {
                // A closed limiter must not allow a probe without a permit.
                return (Duration::ZERO, HandshakeProbeOutcome::Error);
            };
            let start = std::time::Instant::now();
            let outcome = match Box::pin(probe::probe(peer, timeout)).await {
                Ok(probe::Result::Responsive) => {
                    tracing::debug!(
                        %peer,
                        ?timeout,
                        "handshake liveness probe received response"
                    );
                    HandshakeProbeOutcome::Responsive
                }
                Ok(probe::Result::Unresponsive) => {
                    tracing::debug!(
                        %peer,
                        ?timeout,
                        "handshake liveness probe received no response; proceeding"
                    );
                    HandshakeProbeOutcome::Unresponsive
                }
                Err(error) => {
                    // The probe is an optimization. Local resource or socket setup failures
                    // should not prevent an otherwise valid handshake from proceeding.
                    tracing::warn!(
                        %peer,
                        %error,
                        "handshake liveness probe failed; proceeding without probe result"
                    );
                    HandshakeProbeOutcome::Error
                }
            };
            let latency = start.elapsed();
            drop(permit_probe);
            (latency, outcome)
        };
        tokio::pin!(probe);

        loop {
            // Register for notifications before checking state so concurrent waiter and
            // capacity transitions cannot be missed.
            let waiters_changed = self.probe_waiters_changed.notified();
            let capacity_changed = self.capacity_changed.notified();
            tokio::pin!(waiters_changed);
            tokio::pin!(capacity_changed);
            waiters_changed.as_mut().enable();
            capacity_changed.as_mut().enable();

            if can_try_capacity {
                if let Some(capacity) = self.try_acquire_capacity() {
                    drop(waiter);
                    return Some((capacity, queue_start.elapsed()));
                }
            }

            if self.probe_waiters.load(Ordering::Acquire) == 1 && !probe_started {
                let capacity = self.acquire_capacity();
                tokio::pin!(capacity);
                tokio::select! {
                    biased;
                    capacity = &mut capacity => {
                        drop(waiter);
                        return capacity.map(|capacity| (capacity, queue_start.elapsed()));
                    }
                    _ = &mut waiters_changed => {
                        can_try_capacity = true;
                    }
                }
            } else {
                probe_started = true;
                tokio::select! {
                    biased;
                    (latency, outcome) = &mut probe => {
                        self.on_probe_complete(map, peer, latency, outcome);
                        let capacity = self
                            .acquire_capacity()
                            .await
                            .map(|capacity| (capacity, queue_start.elapsed()));
                        drop(waiter);
                        return capacity;
                    }
                    _ = &mut capacity_changed => {
                        can_try_capacity = true;
                    }
                    _ = &mut waiters_changed => {
                        can_try_capacity = true;
                    }
                }
            }
        }
    }

    /// Allocate an entry that will let us wait for the handshake to complete.
    /// This entry also stores the result of the handshake (success or failure).
    fn allocate_entry(&self, peer: SocketAddr, reason: HandshakeReason) -> Arc<Entry> {
        let peer: SocketAddress = peer.into();
        // FIXME: Maybe limit the size of the map?
        // It's not clear what we'd do if we exceeded the limit -- at least today, we only track
        // actively pending handshakes, so that implies dropping handshake requests entirely. But
        // it's not clear that has any real value, we're near guaranteed to want to handshake with
        // them eventually.
        let peer_hash = self.hasher.hash_one(peer);
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let inner = &mut *guard;
        let entry = match inner.table.entry(
            peer_hash,
            |e| e.peer == peer,
            |e| self.hasher.hash_one(e.peer),
        ) {
            hashbrown::hash_table::Entry::Occupied(o) => o.get().clone(),
            hashbrown::hash_table::Entry::Vacant(v) => v
                .insert(Arc::new(Entry {
                    peer,
                    handshaker: tokio::sync::OnceCell::new(),
                    by_reason: [const { AtomicU16::new(0) }; REASON_COUNT],
                }))
                .get()
                .clone(),
        };
        entry.by_reason[reason as usize]
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |v| {
                Some(v.saturating_add(1))
            })
            .expect("Some means always OK");
        entry
    }

    /// Remove a specific entry from the map. This will *not* remove any newly inserted entry (even
    /// if for the same peer address).
    fn remove_entry(&self, entry: &Arc<Entry>) {
        let peer_hash = self.hasher.hash_one(entry.peer);
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let inner = &mut *guard;
        match inner.table.find_entry(peer_hash, |e| e.peer == entry.peer) {
            Ok(o) => {
                if Arc::ptr_eq(o.get(), entry) {
                    o.remove();
                }
            }
            Err(_) => {
                // no further action to take
            }
        }
    }

    /// Handshake with a peer while rate limiting and de-duplicating handshakes.
    ///
    /// This ensures that in-flight handshakes are bounded to a fixed amount (adjusted to maximize
    /// throughput while avoiding unbounded latencies *within* the handshake itself, which causes
    /// timeouts and can cause congestive collapse under enough load).
    async fn handshake(
        self: Arc<Self>,
        client: &s2n_quic::Client,
        map: &secret::Map,
        peer: SocketAddr,
        reason: HandshakeReason,
        server_name: Name,
    ) -> Result<(), HandshakeFailed> {
        let entry = self.allocate_entry(peer, reason);
        let entry2 = entry.clone();
        let entry3 = entry.clone();

        let handshake = async {
            // We've de-duplicated above already so the handshaker is unique per SocketAddr, so
            // these permits will only be used for the current handshake.
            let capacity = if let Some(timeout) = self.probe_timeout {
                self.acquire_prioritized_capacity(map, peer, timeout).await
            } else {
                let start = std::time::Instant::now();
                self.acquire_capacity()
                    .await
                    .map(|capacity| (capacity, start.elapsed()))
            };
            let Some(((permit_inflight, permit_start), limiter_duration)) = capacity else {
                return Err(io::Error::other("handshake concurrency limiter closed"));
            };

            let mut attempt =
                client.connect(s2n_quic::client::Connect::new(peer).with_server_name(server_name));

            // Note that this provides counts at the time of starting the connection attempt.
            // Technically, this omits counts that happen after this point while the deduplication
            // is still active.
            let mut reason_counts = [
                (HandshakeReason::User, 0),
                (HandshakeReason::Periodic, 0),
                (HandshakeReason::Remote, 0),
                (HandshakeReason::KeyIdExhaustion, 0),
            ];
            for (reason, count) in reason_counts.iter_mut() {
                *count = entry.by_reason[*reason as usize].load(Ordering::Relaxed) as usize;
            }

            attempt.set_application_context(Box::new(ConnectionContext {
                limiter_latency: limiter_duration,
                reason_counts,
            }));

            let mut connection = attempt.await?;

            // A 10 second deadline is used to bound both ConfirmComplete and MtuConfirmComplete
            // wait operations, avoiding unbounded waits if the server is slow or unresponsive.
            let deadline = TokioInstant::now() + Duration::from_secs(10);

            // We need to wait for confirmation that the dcQUIC handshake is complete.
            // TODO: This will not be needed if https://github.com/aws/s2n-quic/issues/2273 is addressed
            match tokio::time::timeout_at(deadline, ConfirmComplete::wait_ready(&mut connection))
                .await
            {
                Ok(Ok(())) => {
                    // ConfirmComplete succeeded within the deadline - continue
                }
                Ok(Err(e)) => {
                    // ConfirmComplete::wait_ready failed. We should treat the handshake as failed.
                    //
                    // Explicitly close instead of letting `connection` drop, which would emit a
                    // clean (no-error) CONNECTION_CLOSE. A clean close is the signal the server
                    // uses to complete the dc handshake when the token ACK is lost; since the
                    // handshake did not complete here, we must not send it. Any explicit close is
                    // emitted as an application CONNECTION_CLOSE (`connection::Error::Application`),
                    // which the server does not treat as completion. If the connection is already
                    // closed this is a no-op.
                    connection.close(DC_HANDSHAKE_INCOMPLETE_ERROR.into());
                    return Err(e);
                }
                Err(_elapsed) => {
                    // Handshake timeout occurred. We should treat the handshake as failed.
                    //
                    // Close with an explicit error, as in the failure case above, but with a
                    // distinct code so a timeout can be distinguished from other failures.
                    connection.close(DC_HANDSHAKE_TIMEOUT_ERROR.into());
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "ConfirmComplete handshake timeout",
                    ));
                }
            }

            // Don't wait for the connection to fully close, just wait until dc.complete to
            // drop the permit.
            drop(permit_start);

            // Spawn a task to leave the connection open for MTU probing to complete.
            // The 1-second wait for peers that don't support MtuProbingComplete
            // is handled inside wait_ready() when the connection closes gracefully.
            //
            // This task also owns pruning our de-duplication tracking.
            let this = self.clone();
            let map_clone = map.clone();
            let cleanup = tokio::spawn(async move {
                // Use the same deadline for MTU probing - any remaining time from the 10s budget
                if tokio::time::timeout_at(
                    deadline,
                    MtuConfirmComplete::wait_ready(&mut connection),
                )
                .await
                .is_err()
                {
                    map_clone.on_dc_connection_timeout(&peer);
                }

                drop(connection);
                drop(permit_inflight);

                // Delay deleting the entry by a random time, up to 1 minute.
                //
                // The specific duration is not chosen with any particular rationale, mostly
                // intended to be a relatively small amount while still significantly reducing
                // handshake volume if we're repeatedly handshaking in a short period of time
                // (e.g., due to replay protection packets repeatedly arriving). It's unlikely that
                // handshaking more than roughly once per minute with a given peer actually
                // produces meaningfully better results than allowing a more normal rate of
                // handshakes.
                //
                // Note that we've already dropped the connection and permit above, so we're not
                // blocking any other peer from handshaking.
                let duration = {
                    let mut rng = rand::rng();
                    rng.random_range(0..=(this.success_jitter.as_millis() as u64))
                };
                tokio::time::sleep(Duration::from_millis(duration)).await;

                this.remove_entry(&entry);
            });

            if self.await_dedup_removal {
                let _ = cleanup.await;
            }

            Ok::<_, io::Error>(())
        };

        entry2
            .handshaker
            .get_or_init(|| async {
                // This ensures we only log the error once, even if the handshake was de-duplicated
                // many times.
                if let Err(e) = handshake.await {
                    // We may want to remove this in favor of only relying on the service log
                    // eventually, but keeping it for parity for now.
                    tracing::error!("handshake with {peer} failed: {e}");

                    // Delay deleting the entry by a random time, up to 2 minutes.
                    //
                    // This avoids aggressively reconnecting to a given peer if handshakes
                    // fail (instead we keep returning the cached error). This is good both for
                    // fast failure (e.g., certificate issues) and for slow errors (timeouts).
                    // In the first case, it's very unlikely the issue will be fixed within
                    // seconds, so backing off is natural to keep aggregate handshake volume
                    // more bounded. For the latter, backing off avoids generating undue load
                    // on the network or server. The specific duration is not chosen
                    // with any particular rationale, mostly intended to be a relatively small
                    // amount (to avoid significantly extending recovery times if the server
                    // was temporarily overloaded) while still significantly reducing handshake
                    // volume (>60x for fast-failing handshakes and >10x for timeouts).
                    if self.error_jitter.is_zero() {
                        self.remove_entry(&entry3);
                    } else {
                        let this = self.clone();
                        let error_jitter = self.error_jitter;
                        let cleanup = tokio::spawn(async move {
                            let duration = {
                                let mut rng = rand::rng();
                                let min = 1000.min(error_jitter.as_millis() as u64);
                                rng.random_range(min..=error_jitter.as_millis() as u64)
                            };
                            tokio::time::sleep(Duration::from_millis(duration)).await;
                            this.remove_entry(&entry3);
                        });

                        if self.await_dedup_removal {
                            let _ = cleanup.await;
                        }
                    }

                    Err(HandshakeFailed(e))
                } else {
                    Ok(())
                }
            })
            .await
            .as_ref()
            .map(|v| *v)
            .map_err(|e| e.duplicate())
    }
}

struct ProbeWaiterGuard {
    queue: Arc<HandshakeQueue>,
}

impl Drop for ProbeWaiterGuard {
    fn drop(&mut self) {
        self.queue.probe_waiters.fetch_sub(1, Ordering::AcqRel);
        self.queue.probe_waiters_changed.notify_waiters();
    }
}

// This is only created if we've already logged a handshake error.
#[derive(Debug)]
pub struct HandshakeFailed(io::Error);

impl HandshakeFailed {
    fn duplicate(&self) -> Self {
        // Manually create a similar io::Error while preserving information about the inner error if present.
        if let Some(inner) = self.0.get_ref() {
            Self(io::Error::new(self.0.kind(), inner.to_string()))
        } else {
            Self(io::Error::from(self.0.kind()))
        }
    }
}

impl From<HandshakeFailed> for io::Error {
    fn from(e: HandshakeFailed) -> io::Error {
        e.0
    }
}

#[derive(Debug, Copy, Clone)]
pub enum HandshakeReason {
    /// An explicit request by the application owner
    User,
    /// Periodic re-handshaking
    Periodic,
    /// Rehandshaking driven by remote packets (e.g., unknown path secret).
    Remote,
    /// The path secret ran out of key IDs, so it can no longer be used for sending.
    KeyIdExhaustion,
}

/// Widens a background re-handshake reason, as reported on the
/// `path_secret_map:background_handshake_requested` event, into the full set tracked by the
/// handshake queue.
///
/// Only this direction is total. The event-side enum has no user-initiated variant, because
/// user-initiated handshakes call [`Client::connect`] directly and never route through
/// `Map::request_handshake`, so the event's `reason` counter has no bucket that is impossible to
/// increment.
impl From<crate::event::builder::HandshakeReason> for HandshakeReason {
    fn from(reason: crate::event::builder::HandshakeReason) -> Self {
        use crate::event::builder::HandshakeReason as Background;
        match reason {
            Background::Periodic => Self::Periodic,
            Background::Remote => Self::Remote,
            Background::KeyIdExhaustion => Self::KeyIdExhaustion,
        }
    }
}

const REASON_COUNT: usize = 4;

pub struct ConnectionContext {
    pub limiter_latency: Duration,
    pub reason_counts: [(HandshakeReason, usize); REASON_COUNT],
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        path::secret::{stateless_reset::Signer, Map},
        testing::{init_tracing, NoopSubscriber, TestTlsProvider},
    };
    use s2n_quic::provider::{
        endpoint_limits::{ConnectionAttempt, Limiter, Outcome},
        tls::Provider,
    };
    use s2n_quic_core::time::StdClock;
    use std::time::Instant;
    use tokio_util::sync::DropGuard;

    /// A test limiter that closes all incoming connections immediately
    #[derive(Default)]
    struct CloseAllConnectionsLimiter;

    impl Limiter for CloseAllConnectionsLimiter {
        fn on_connection_attempt(&mut self, _info: &ConnectionAttempt) -> Outcome {
            Outcome::close()
        }
    }

    struct CountingCloseConnectionsLimiter(Arc<AtomicUsize>);

    impl Limiter for CountingCloseConnectionsLimiter {
        fn on_connection_attempt(&mut self, _info: &ConnectionAttempt) -> Outcome {
            self.0.fetch_add(1, Ordering::Relaxed);
            Outcome::close()
        }
    }

    fn is_probe_request(packet: &[u8]) -> bool {
        probe::ProbeRequest::from_bytes(packet).is_some()
    }

    /// A test event subscriber that records the maximum MTU reported by `MtuUpdated` events.
    #[derive(Clone, Default)]
    struct MtuRecorder {
        max_mtu: Arc<AtomicU16>,
    }

    impl s2n_quic::provider::event::Subscriber for MtuRecorder {
        type ConnectionContext = ();

        fn create_connection_context(
            &mut self,
            _meta: &s2n_quic::provider::event::ConnectionMeta,
            _info: &s2n_quic::provider::event::ConnectionInfo,
        ) -> Self::ConnectionContext {
        }

        fn on_mtu_updated(
            &mut self,
            _context: &mut Self::ConnectionContext,
            _meta: &s2n_quic::provider::event::ConnectionMeta,
            event: &s2n_quic::provider::event::events::MtuUpdated,
        ) {
            self.max_mtu.fetch_max(event.mtu, Ordering::Relaxed);
        }
    }

    /// Helper to set up a test client and server
    struct TestSetup {
        client: Client,
        server_addr: SocketAddr,
        _server_guard: DropGuard,
    }

    impl TestSetup {
        /// Creates a test setup with an optional endpoint limiter for the server
        async fn new<L, Event>(
            endpoint_limits: Option<L>,
            server_builder: server::Builder<Event>,
        ) -> Self
        where
            L: s2n_quic::provider::endpoint_limits::Limiter + Send + Sync + 'static,
            Event: s2n_quic::provider::event::Subscriber + Send + Sync + 'static,
        {
            Self::new_with_client_builder(
                endpoint_limits,
                server_builder,
                crate::psk::client::Builder::default().with_success_jitter(Duration::ZERO),
            )
            .await
        }

        async fn new_with_client_builder<L, Event>(
            endpoint_limits: Option<L>,
            server_builder: server::Builder<Event>,
            client_builder: crate::psk::client::Builder,
        ) -> Self
        where
            L: s2n_quic::provider::endpoint_limits::Limiter + Send + Sync + 'static,
            Event: s2n_quic::provider::event::Subscriber + Send + Sync + 'static,
        {
            init_tracing();

            let tls = TestTlsProvider {};
            let subscriber = NoopSubscriber {};

            let server_map = Map::new(
                Signer::new(b"default"),
                50_000,
                false,
                StdClock::default(),
                subscriber.clone(),
            );

            let (server_addr_rx, server_guard) = if let Some(limiter) = endpoint_limits {
                crate::psk::server::Provider::setup(
                    "127.0.0.1:0".parse().unwrap(),
                    server_map.clone(),
                    tls.clone(),
                    subscriber.clone(),
                    server_builder.with_endpoint_limits(limiter),
                )
            } else {
                crate::psk::server::Provider::setup(
                    "127.0.0.1:0".parse().unwrap(),
                    server_map.clone(),
                    tls.clone(),
                    subscriber.clone(),
                    server_builder,
                )
            }
            .unwrap();

            let client_map = Map::new(
                Signer::new(b"default"),
                50_000,
                false,
                StdClock::default(),
                subscriber.clone(),
            );

            let client = Client::bind::<
                <TestTlsProvider as Provider>::Client,
                NoopSubscriber,
                s2n_quic::provider::event::default::Subscriber,
            >(
                "0.0.0.0:0".parse().unwrap(),
                client_map,
                tls.start_client().unwrap(),
                subscriber,
                client_builder,
            )
            .unwrap();

            let server_addr = server_addr_rx.await.unwrap().unwrap();

            Self {
                client,
                server_addr,
                _server_guard: server_guard,
            }
        }
    }

    #[tokio::test]
    async fn handshake_server_answers_probe_without_connection_attempt() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let setup = TestSetup::new(
            Some(CountingCloseConnectionsLimiter(attempts.clone())),
            crate::psk::server::Builder::default(),
        )
        .await;

        assert_eq!(
            probe::probe(setup.server_addr, Duration::from_secs(1))
                .await
                .unwrap(),
            probe::Result::Responsive
        );
        assert_eq!(attempts.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn handshake_server_does_not_answer_malformed_probe() {
        let setup = TestSetup::new::<CloseAllConnectionsLimiter, _>(
            None,
            crate::psk::server::Builder::default(),
        )
        .await;
        let socket = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        socket.connect(setup.server_addr).await.unwrap();

        let mut request = *probe::ProbeRequest::new().as_bytes();
        request[10] = 1;
        socket.send(&request).await.unwrap();

        let mut response = [0u8; probe::RECEIVE_BUFFER_SIZE];
        assert!(
            tokio::time::timeout(Duration::from_millis(250), socket.recv(&mut response))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn preflight_probe_prioritizes_responsive_peers() {
        let probe_timeout = Duration::from_secs(2);
        let client_builder = crate::psk::client::Builder::default()
            .with_success_jitter(Duration::ZERO)
            .with_error_jitter(Duration::ZERO)
            .with_handshake_start_limit(1)
            .with_handshake_inflight_limit(1)
            .with_handshake_probe_timeout(probe_timeout)
            .with_handshake_probe_limit(2);
        let setup = TestSetup::new_with_client_builder::<CloseAllConnectionsLimiter, _>(
            None,
            crate::psk::server::Builder::default(),
            client_builder,
        )
        .await;
        let server_name: s2n_quic::server::Name = "localhost".into();
        let capacity = setup.client.queue.try_acquire_capacity().unwrap();
        let (probe_observer, mut probe_results) = tokio::sync::mpsc::unbounded_channel();
        *setup
            .client
            .queue
            .probe_observer
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(probe_observer);

        // Hold a UDP port open without servicing it. This simulates a stale fleet entry without
        // generating an immediate ICMP port-unreachable response.
        let unresponsive = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let unresponsive_addr = unresponsive.local_addr().unwrap();
        let (probe_received_tx, probe_received_rx) = tokio::sync::oneshot::channel();
        let (handshake_received_tx, handshake_received_rx) = tokio::sync::oneshot::channel();
        let unresponsive_sink = tokio::spawn(async move {
            let mut packet = [0u8; 1200];
            let (len, _) = unresponsive.recv_from(&mut packet).await.unwrap();
            assert!(is_probe_request(&packet[..len]));
            probe_received_tx.send(()).unwrap();

            let (len, _) = unresponsive.recv_from(&mut packet).await.unwrap();
            assert!(!is_probe_request(&packet[..len]));
            handshake_received_tx.send(()).unwrap();
        });

        let responsive = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let responsive_addr = responsive.local_addr().unwrap();
        let (responsive_handshake_tx, responsive_handshake_rx) = tokio::sync::oneshot::channel();
        let responsive_sink = tokio::spawn(async move {
            let mut packet = [0u8; 1200];
            let (len, peer) = responsive.recv_from(&mut packet).await.unwrap();
            let request = probe::ProbeRequest::from_bytes(&packet[..len]).unwrap();
            responsive
                .send_to(request.response().as_bytes(), peer)
                .await
                .unwrap();

            let (len, _) = responsive.recv_from(&mut packet).await.unwrap();
            assert!(!is_probe_request(&packet[..len]));
            responsive_handshake_tx.send(()).unwrap();
        });

        let client = setup.client.clone();
        let dead_server_name = server_name.clone();
        let dead_handshake = tokio::spawn(async move {
            client
                .connect(unresponsive_addr, HandshakeReason::User, dead_server_name)
                .await
        });

        // A lone contended handshake does not probe. Wait until the stale peer has registered,
        // then add a second waiter so the queue has peers to rank.
        tokio::time::timeout(Duration::from_secs(1), async {
            while setup.client.queue.probe_waiters.load(Ordering::Acquire) != 1 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("stale peer did not enter the contended queue");
        let client = setup.client.clone();
        let live_handshake = tokio::spawn(async move {
            client
                .connect(responsive_addr, HandshakeReason::User, server_name)
                .await
        });
        probe_received_rx.await.unwrap();

        // Wait until the responsive probe result has been consumed. The stale peer continues
        // probing until its deadline.
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                let (peer, outcome) = probe_results.recv().await.unwrap();
                if peer == responsive_addr {
                    assert_eq!(outcome, HandshakeProbeOutcome::Responsive);
                    break;
                }
            }
        })
        .await
        .expect("responsive probe did not complete");

        // Keep capacity occupied until the silent peer's probe window ends. Both peers then enter
        // the handshake queue in probe-completion order.
        tokio::time::timeout(probe_timeout * 2, async {
            loop {
                let (peer, outcome) = probe_results.recv().await.unwrap();
                if peer == unresponsive_addr {
                    assert_eq!(outcome, HandshakeProbeOutcome::Unresponsive);
                    break;
                }
            }
        })
        .await
        .expect("silent peer probe did not complete");

        // Releasing handshake capacity lets the responsive peer overtake the stale peer.
        drop(capacity);
        tokio::time::timeout(Duration::from_secs(1), responsive_handshake_rx)
            .await
            .expect("responsive peer was blocked behind an unresponsive peer")
            .unwrap();

        // The responsive test peer does not implement a real handshake, so cancel that attempt
        // after observing its Initial to release the single in-flight handshake permit.
        live_handshake.abort();
        let _ = live_handshake.await;

        // A silent peer is delayed, not rejected: once the probe completes it starts its normal
        // handshake and sends a second, supported-version packet.
        tokio::time::timeout(Duration::from_secs(1), handshake_received_rx)
            .await
            .expect("silent peer did not proceed after its probe timeout")
            .unwrap();

        dead_handshake.abort();
        let _ = dead_handshake.await;
        unresponsive_sink.await.unwrap();
        responsive_sink.await.unwrap();
    }

    #[tokio::test]
    async fn preflight_probe_is_skipped_when_handshake_capacity_is_available() {
        let client_builder = crate::psk::client::Builder::default()
            .with_success_jitter(Duration::ZERO)
            .with_error_jitter(Duration::ZERO)
            .with_handshake_inflight_limit(1)
            .with_handshake_start_limit(1)
            .with_handshake_probe_timeout(Duration::from_secs(1))
            .with_handshake_probe_limit(1);
        let setup = TestSetup::new_with_client_builder::<CloseAllConnectionsLimiter, _>(
            None,
            crate::psk::server::Builder::default(),
            client_builder,
        )
        .await;
        let sink = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let sink_addr = sink.local_addr().unwrap();
        let client = setup.client.clone();
        let handshake = tokio::spawn(async move {
            client
                .connect(sink_addr, HandshakeReason::User, "localhost".into())
                .await
        });

        let mut packet = [0u8; 1200];
        let (len, _) = tokio::time::timeout(Duration::from_secs(1), sink.recv_from(&mut packet))
            .await
            .expect("handshake packet was not sent")
            .unwrap();
        assert!(
            !is_probe_request(&packet[..len]),
            "uncontended handshake unexpectedly sent a liveness probe"
        );

        handshake.abort();
        let _ = handshake.await;
    }

    #[tokio::test]
    async fn lone_waiter_skips_probe_and_uses_new_capacity() {
        let client_builder = crate::psk::client::Builder::default()
            .with_success_jitter(Duration::ZERO)
            .with_error_jitter(Duration::ZERO)
            .with_handshake_inflight_limit(1)
            .with_handshake_start_limit(1)
            .with_handshake_probe_timeout(Duration::from_secs(5))
            .with_handshake_probe_limit(1);
        let setup = TestSetup::new_with_client_builder::<CloseAllConnectionsLimiter, _>(
            None,
            crate::psk::server::Builder::default(),
            client_builder,
        )
        .await;
        let capacity = setup.client.queue.try_acquire_capacity().unwrap();
        let sink = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let sink_addr = sink.local_addr().unwrap();
        let client = setup.client.clone();
        let handshake = tokio::spawn(async move {
            client
                .connect(sink_addr, HandshakeReason::User, "localhost".into())
                .await
        });

        tokio::time::timeout(Duration::from_secs(1), async {
            while setup.client.queue.probe_waiters.load(Ordering::Acquire) != 1 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("handshake did not enter the contended queue");
        assert_eq!(
            setup.client.queue.limiter_probe.available_permits(),
            1,
            "a lone waiter unexpectedly started a liveness probe"
        );

        drop(capacity);
        let mut packet = [0u8; 1200];
        let (len, _) = tokio::time::timeout(Duration::from_secs(1), sink.recv_from(&mut packet))
            .await
            .expect("lone waiter did not use newly available handshake capacity")
            .unwrap();
        assert!(!is_probe_request(&packet[..len]));

        handshake.abort();
        let _ = handshake.await;
    }

    #[tokio::test]
    async fn available_capacity_preempts_in_progress_probes() {
        let client_builder = crate::psk::client::Builder::default()
            .with_success_jitter(Duration::ZERO)
            .with_error_jitter(Duration::ZERO)
            .with_handshake_inflight_limit(1)
            .with_handshake_start_limit(1)
            .with_handshake_probe_timeout(Duration::from_secs(5))
            .with_handshake_probe_limit(2);
        let setup = TestSetup::new_with_client_builder::<CloseAllConnectionsLimiter, _>(
            None,
            crate::psk::server::Builder::default(),
            client_builder,
        )
        .await;
        let capacity = setup.client.queue.try_acquire_capacity().unwrap();
        let first_sink = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let second_sink = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let first_addr = first_sink.local_addr().unwrap();
        let second_addr = second_sink.local_addr().unwrap();
        let server_name: s2n_quic::server::Name = "localhost".into();

        let client = setup.client.clone();
        let first_server_name = server_name.clone();
        let first = tokio::spawn(async move {
            client
                .connect(first_addr, HandshakeReason::User, first_server_name)
                .await
        });
        tokio::time::timeout(Duration::from_secs(1), async {
            while setup.client.queue.probe_waiters.load(Ordering::Acquire) != 1 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("first handshake did not enter the contended queue");

        let client = setup.client.clone();
        let second = tokio::spawn(async move {
            client
                .connect(second_addr, HandshakeReason::User, server_name)
                .await
        });

        let mut first_packet = [0u8; 1200];
        let mut second_packet = [0u8; 1200];
        let (first_len, second_len) = tokio::time::timeout(Duration::from_secs(1), async {
            let (first, second) = tokio::join!(
                first_sink.recv_from(&mut first_packet),
                second_sink.recv_from(&mut second_packet)
            );
            (first.unwrap().0, second.unwrap().0)
        })
        .await
        .expect("both probes were not sent");
        assert!(is_probe_request(&first_packet[..first_len]));
        assert!(is_probe_request(&second_packet[..second_len]));

        drop(capacity);
        let mut first_handshake = [0u8; 1200];
        let mut second_handshake = [0u8; 1200];
        let (first_won, handshake_len) = tokio::time::timeout(Duration::from_secs(1), async {
            tokio::select! {
                result = first_sink.recv_from(&mut first_handshake) => {
                    (true, result.unwrap().0)
                }
                result = second_sink.recv_from(&mut second_handshake) => {
                    (false, result.unwrap().0)
                }
            }
        })
        .await
        .expect("new handshake capacity remained idle while probes were in progress");
        let packet = if first_won {
            &first_handshake[..handshake_len]
        } else {
            &second_handshake[..handshake_len]
        };
        assert!(!is_probe_request(packet));

        first.abort();
        second.abort();
        let _ = first.await;
        let _ = second.await;
    }

    #[tokio::test]
    async fn preflight_probe_respects_concurrency_limit() {
        let probe_timeout = Duration::from_millis(200);
        let client_builder = crate::psk::client::Builder::default()
            .with_success_jitter(Duration::ZERO)
            .with_error_jitter(Duration::ZERO)
            .with_handshake_inflight_limit(1)
            .with_handshake_probe_timeout(probe_timeout)
            .with_handshake_probe_limit(1);
        let setup = TestSetup::new_with_client_builder::<CloseAllConnectionsLimiter, _>(
            None,
            crate::psk::server::Builder::default(),
            client_builder,
        )
        .await;
        let capacity = setup.client.queue.try_acquire_capacity().unwrap();
        let first_sink = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let second_sink = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let first_addr = first_sink.local_addr().unwrap();
        let second_addr = second_sink.local_addr().unwrap();
        let server_name: s2n_quic::server::Name = "localhost".into();

        let client = setup.client.clone();
        let first_server_name = server_name.clone();
        let first = tokio::spawn(async move {
            client
                .connect(first_addr, HandshakeReason::User, first_server_name)
                .await
        });

        tokio::time::timeout(Duration::from_secs(1), async {
            while setup.client.queue.probe_waiters.load(Ordering::Acquire) != 1 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("first handshake did not enter the contended queue");

        let client = setup.client.clone();
        let second = tokio::spawn(async move {
            client
                .connect(second_addr, HandshakeReason::User, server_name)
                .await
        });

        let mut first_packet = [0u8; 1200];
        let mut second_packet = [0u8; 1200];
        let first_probe_peer = tokio::time::timeout(Duration::from_secs(1), async {
            tokio::select! {
                result = first_sink.recv_from(&mut first_packet) => {
                    result.unwrap();
                    first_addr
                }
                result = second_sink.recv_from(&mut second_packet) => {
                    result.unwrap();
                    second_addr
                }
            }
        })
        .await
        .expect("no probe was sent");

        let blocked_sink = if first_probe_peer == first_addr {
            &second_sink
        } else {
            &first_sink
        };
        let mut packet = [0u8; 1200];
        assert!(
            tokio::time::timeout(probe_timeout / 2, blocked_sink.recv_from(&mut packet))
                .await
                .is_err(),
            "a second probe exceeded the configured concurrency limit"
        );
        tokio::time::timeout(probe_timeout * 2, blocked_sink.recv_from(&mut packet))
            .await
            .expect("the blocked probe did not start after the first probe window")
            .unwrap();

        first.abort();
        second.abort();
        let _ = first.await;
        let _ = second.await;
        drop(capacity);
    }

    #[tokio::test]
    async fn closed_probe_limiter_skips_probes_and_allows_handshakes() {
        let client_builder = crate::psk::client::Builder::default()
            .with_success_jitter(Duration::ZERO)
            .with_error_jitter(Duration::ZERO)
            .with_handshake_inflight_limit(1)
            .with_handshake_start_limit(1)
            .with_handshake_probe_timeout(Duration::from_secs(5))
            .with_handshake_probe_limit(1);
        let setup = TestSetup::new_with_client_builder::<CloseAllConnectionsLimiter, _>(
            None,
            crate::psk::server::Builder::default(),
            client_builder,
        )
        .await;
        let capacity = setup.client.queue.try_acquire_capacity().unwrap();
        setup.client.queue.limiter_probe.close();
        let (probe_observer, mut probe_results) = tokio::sync::mpsc::unbounded_channel();
        *setup
            .client
            .queue
            .probe_observer
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(probe_observer);

        let first_sink = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let second_sink = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let first_addr = first_sink.local_addr().unwrap();
        let second_addr = second_sink.local_addr().unwrap();
        let server_name: s2n_quic::server::Name = "localhost".into();

        let client = setup.client.clone();
        let first_server_name = server_name.clone();
        let first = tokio::spawn(async move {
            client
                .connect(first_addr, HandshakeReason::User, first_server_name)
                .await
        });
        tokio::time::timeout(Duration::from_secs(1), async {
            while setup.client.queue.probe_waiters.load(Ordering::Acquire) != 1 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("first handshake did not enter the contended queue");

        let client = setup.client.clone();
        let second = tokio::spawn(async move {
            client
                .connect(second_addr, HandshakeReason::User, server_name)
                .await
        });

        tokio::time::timeout(Duration::from_secs(1), async {
            let mut peers = std::collections::HashSet::new();
            for _ in 0..2 {
                let (peer, outcome) = probe_results.recv().await.unwrap();
                assert_eq!(outcome, HandshakeProbeOutcome::Error);
                peers.insert(peer);
            }
            assert_eq!(peers.len(), 2);
            assert!(peers.contains(&first_addr));
            assert!(peers.contains(&second_addr));
        })
        .await
        .expect("closed probe limiter did not resolve both probes");

        let mut packet = [0u8; 1200];
        assert!(first_sink.try_recv_from(&mut packet).is_err());
        assert!(second_sink.try_recv_from(&mut packet).is_err());

        drop(capacity);
        let mut first_packet = [0u8; 1200];
        let mut second_packet = [0u8; 1200];
        let (first_won, len) = tokio::time::timeout(Duration::from_secs(1), async {
            tokio::select! {
                result = first_sink.recv_from(&mut first_packet) => {
                    (true, result.unwrap().0)
                }
                result = second_sink.recv_from(&mut second_packet) => {
                    (false, result.unwrap().0)
                }
            }
        })
        .await
        .expect("handshake did not proceed after the probe limiter closed");
        let packet = if first_won {
            &first_packet[..len]
        } else {
            &second_packet[..len]
        };
        assert!(!is_probe_request(packet));

        first.abort();
        second.abort();
        let _ = first.await;
        let _ = second.await;
    }

    #[test]
    fn probing_is_opt_in() {
        assert_eq!(HandshakeQueueConfig::default().probe_limit, 0);
        let queue = HandshakeQueue::new(HandshakeQueueConfig {
            probe_timeout: Some(Duration::from_secs(1)),
            ..Default::default()
        });
        assert_eq!(queue.probe_timeout, None);
    }

    /// Verifies MtuProbingComplete works correctly (no 1-second fallback delay).
    ///
    /// After a handshake, a cleanup task runs in the background. If MtuProbingComplete
    /// is NOT working, this task sleeps for 1 second before removing the deduplication entry.
    ///
    /// We detect this by waiting 500ms then checking if the entry was removed:
    /// - If entry was removed (MtuProbingComplete works): cleanup completed quickly
    /// - If entry still exists (1-second delay active): cleanup is still sleeping
    #[tokio::test]
    async fn mtu_probing_complete_no_delay_test() {
        let server_builder = crate::psk::server::Builder::default();
        let setup = TestSetup::new::<CloseAllConnectionsLimiter, _>(None, server_builder).await;
        let server_name: s2n_quic::server::Name = "localhost".into();

        // First handshake
        let first_handshake_result = setup
            .client
            .connect(
                setup.server_addr,
                HandshakeReason::User,
                server_name.clone(),
            )
            .await;
        assert!(first_handshake_result.is_ok());

        // Wait 500ms - enough for cleanup if MtuProbingComplete works, but not if 1s delay triggered
        tokio::time::sleep(Duration::from_millis(500)).await;

        // If entry still exists after 500ms, the cleanup task hasn't finished yet,
        // which indicates the 1-second fallback delay is active.
        assert!(!setup.client.has_pending_entry(setup.server_addr));

        // Second handshake to same peer - should succeed since entry was removed
        let second_handshake_start = Instant::now();
        let second_handshake_result = setup
            .client
            .connect(
                setup.server_addr,
                HandshakeReason::User,
                server_name.clone(),
            )
            .await;
        let second_handshake_duration = second_handshake_start.elapsed();
        assert!(second_handshake_result.is_ok());

        // Additional timing check: if entry was properly removed, the second handshake
        // should take at least 1ms (a fresh handshake). If it's <1ms, it was deduplicated.
        assert!(second_handshake_duration >= Duration::from_millis(1));
    }

    /// Verifies that when the server closes a connection immediately (via endpoint limits),
    /// the client connection closes without waiting.
    ///
    /// This test ensures that `MtuConfirmComplete::wait_ready` properly detects the
    /// connection close signal and returns immediately rather than blocking.
    #[tokio::test]
    async fn server_close_connection_no_delay_test() {
        let server_builder = crate::psk::server::Builder::default();
        let setup = TestSetup::new(Some(CloseAllConnectionsLimiter), server_builder).await;
        let server_name: s2n_quic::server::Name = "localhost".into();

        // Attempt to connect - the server should immediately close the connection
        let start = Instant::now();
        let result = setup
            .client
            .connect(setup.server_addr, HandshakeReason::User, server_name)
            .await;
        let duration = start.elapsed();

        // The connection should fail (server rejected it)
        assert!(result.is_err());

        // The failure should be fast - definitely less than the 10-second timeout
        // and less than the 1-second fallback delay
        assert!(
            duration < Duration::from_millis(500),
            "Connection took {:?}, expected < 500ms",
            duration
        );
    }

    #[test]
    fn alloc_entry_increments() {
        let queue = HandshakeQueue::new(HandshakeQueueConfig {
            success_jitter: Duration::ZERO,
            ..Default::default()
        });
        let peer_a = "127.0.0.1:3333".parse().unwrap();
        assert_eq!(
            queue
                .allocate_entry(peer_a, HandshakeReason::User)
                .by_reason[HandshakeReason::User as usize]
                .load(Ordering::Relaxed),
            1
        );
        assert_eq!(
            queue
                .allocate_entry(peer_a, HandshakeReason::User)
                .by_reason[HandshakeReason::User as usize]
                .load(Ordering::Relaxed),
            2
        );
        assert_eq!(
            queue
                .allocate_entry(peer_a, HandshakeReason::Periodic)
                .by_reason[HandshakeReason::Periodic as usize]
                .load(Ordering::Relaxed),
            1
        );
    }

    #[tokio::test]
    async fn transparent_transport_preserves_tls_error_code() {
        use crate::testing::UntrustedClientProvider;

        init_tracing();

        let subscriber = NoopSubscriber {};
        let tls = UntrustedClientProvider;

        let server_map = Map::new(
            Signer::new(b"default"),
            50_000,
            false,
            StdClock::default(),
            subscriber.clone(),
        );

        let server_builder = crate::psk::server::Builder::default();
        let (server_addr_rx, _server_guard) = crate::psk::server::Provider::setup(
            "127.0.0.1:0".parse().unwrap(),
            server_map,
            tls.clone(),
            subscriber.clone(),
            server_builder,
        )
        .unwrap();

        let client_map = Map::new(
            Signer::new(b"default"),
            50_000,
            false,
            StdClock::default(),
            subscriber.clone(),
        );

        let client = Client::bind::<
            <UntrustedClientProvider as Provider>::Client,
            NoopSubscriber,
            s2n_quic::provider::event::default::Subscriber,
        >(
            "0.0.0.0:0".parse().unwrap(),
            client_map,
            tls.start_client().unwrap(),
            subscriber,
            crate::psk::client::Builder::default().with_success_jitter(Duration::ZERO),
        )
        .unwrap();

        let server_addr = server_addr_rx.await.unwrap().unwrap();
        let server_name: s2n_quic::server::Name = "localhost".into();

        let result = client
            .connect(server_addr, HandshakeReason::User, server_name)
            .await;

        let err: io::Error = result.unwrap_err().into();
        let err_msg = err.to_string();

        assert!(
            err_msg.contains("CERTIFICATE_UNKNOWN"),
            "Expected CERTIFICATE_UNKNOWN, got: {err_msg}"
        );
    }

    /// Confirm that without offloading (default configuration) we don't perform MTU probing.
    #[tokio::test]
    async fn no_mtu_probing() {
        const MIN_MTU: u16 = 1200;
        let mtu_recorder = MtuRecorder::default();
        let server_builder =
            crate::psk::server::Builder::default().with_event_subscriber(mtu_recorder.clone());

        let setup = TestSetup::new::<CloseAllConnectionsLimiter, _>(None, server_builder).await;
        let server_name: s2n_quic::server::Name = "localhost".into();

        setup
            .client
            .connect(setup.server_addr, HandshakeReason::User, server_name)
            .await
            .unwrap();

        // With offloading enabled, MTU probing is disabled and the server's MTU is fixed at
        // DEFAULT_BASE_MTU.
        let mtu = mtu_recorder.max_mtu.load(Ordering::Relaxed);

        // The MTU reported by the event is the maximum QUIC datagram size, which excludes the UDP
        // and IP headers, so derive the expected value from DEFAULT_BASE_MTU the same way.
        let peer_address: SocketAddress = setup.server_addr.into();
        let expected_mtu = s2n_quic_core::path::InitialMtu::try_from(DEFAULT_BASE_MTU)
            .unwrap()
            .max_datagram_size(&peer_address);

        if cfg!(target_os = "linux") {
            assert_eq!(mtu, expected_mtu);
        } else {
            assert_eq!(mtu, MIN_MTU);
        }
    }

    /// Sanity check that a server with offloading enabled can successfully complete a dc-quic handshake
    #[tokio::test]
    async fn server_offloading() {
        const MIN_MTU: u16 = 1200;
        let mtu_recorder = MtuRecorder::default();
        let server_builder = crate::psk::server::Builder::default()
            .with_thread_count(2)
            .with_event_subscriber(mtu_recorder.clone());

        let setup = TestSetup::new::<CloseAllConnectionsLimiter, _>(None, server_builder).await;
        let server_name: s2n_quic::server::Name = "localhost".into();

        setup
            .client
            .connect(setup.server_addr, HandshakeReason::User, server_name)
            .await
            .unwrap();

        // With offloading enabled, MTU probing is disabled and the server's MTU is fixed at
        // DEFAULT_BASE_MTU.
        let mtu = mtu_recorder.max_mtu.load(Ordering::Relaxed);

        // The MTU reported by the event is the maximum QUIC datagram size, which excludes the UDP
        // and IP headers, so derive the expected value from DEFAULT_BASE_MTU the same way.
        let peer_address: SocketAddress = setup.server_addr.into();
        let expected_mtu = s2n_quic_core::path::InitialMtu::try_from(DEFAULT_BASE_MTU)
            .unwrap()
            .max_datagram_size(&peer_address);

        if cfg!(target_os = "linux") {
            assert_eq!(mtu, expected_mtu);
        } else {
            assert_eq!(mtu, MIN_MTU);
        }
    }
}

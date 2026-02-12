// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{client, server};
use crate::path::secret;
use rand::Rng;
use s2n_quic::{
    provider::{
        dc::{ConfirmComplete, MtuConfirmComplete},
        event::Subscriber as Sub,
        tls::Provider as Prov,
    },
    server::Name,
};
use s2n_quic_core::inet::SocketAddress;
#[cfg(target_os = "linux")]
use s2n_quic_platform::bpf::cbpf::{abs, and, jeq, ldb, ret, Program};
use s2n_quic_platform::syscall;
use std::{
    hash::BuildHasher,
    io,
    net::SocketAddr,
    sync::{
        atomic::{AtomicU16, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tokio::{sync::Semaphore, time::Instant as TokioInstant};

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

const BUFFER_SIZE: usize = 16 * 1024;

/// cBPF program to route QUIC packets across multiple sockets.
/// Routes Initial packets with DCID length = 8 to socket 0, all other packets to socket 1.
#[cfg(target_os = "linux")]
static ROUTER: Program = Program::new(&[
    // Load byte 0 and check if it's an Initial packet (first 4 bits = 1100)
    ldb(abs(0)),
    and(0b1111_0000), // Mask the last four bits of the first byte. The first four bits can confirm if the packet is a INITIAL packet.
    // If Initial packet, continue; else jump to ret(1)
    jeq(0b1100_0000, 0, 3), // First four bits of INITIAL packet should be 1100.
    // Load byte 5 (DCID length) and check if it equals 8
    ldb(abs(5)),
    // If DCID len = 8, continue to ret(0); else jump to ret(1)
    jeq(0x08, 0, 1),
    // Return 0: socket 0 handles Initial packets with DCID length = 8
    ret(0),
    // Return 1: socket 1 handles all other packets
    ret(1),
]);

pub type Error = Box<dyn std::error::Error + Send + Sync + 'static>;

pub type Result<T = (), E = Error> = core::result::Result<T, E>;

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
        let socket_for_client_hello_packets = syscall::bind_udp(addr, false, false, false)?;

        // Acquire the bound address with a port assigned
        let bound_addr = socket_for_client_hello_packets
            .local_addr()?
            .as_socket()
            .unwrap();

        socket_for_client_hello_packets
            .set_reuse_port(true)
            .unwrap();

        let socket_for_other_packets = syscall::bind_udp(bound_addr, false, true, false)?;

        // Attach ROUTER to both sockets for packet filtering
        #[cfg(target_os = "linux")]
        {
            ROUTER.attach(&socket_for_client_hello_packets)?;
            ROUTER.attach(&socket_for_other_packets)?;
        }

        let io = s2n_quic::provider::io::default::Builder::default()
            .with_rx_socket(socket_for_client_hello_packets.into())?
            .with_rx_socket(socket_for_other_packets.into())?
            .with_base_mtu(DEFAULT_BASE_MTU.min(builder.mtu))?
            .with_initial_mtu(builder.mtu)?
            .with_max_mtu(builder.mtu)?
            .with_internal_recv_buffer_size(BUFFER_SIZE)?
            .build()?;

        let server = s2n_quic::Server::builder().with_io(io)?;

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
            .with_initial_round_trip_time(DEFAULT_INITIAL_RTT)?;

        let event = ((ConfirmComplete, MtuConfirmComplete), subscriber);

        #[cfg(not(any(test, feature = "testing")))]
        let server = server
            .with_limits(connection_limits)?
            .with_dc(map.clone())?
            .with_event(event)?
            .with_tls(tls_materials_provider)?
            .start()?;

        #[cfg(any(test, feature = "testing"))]
        let server = {
            let server = server
                .with_limits(connection_limits)?
                .with_dc(map.clone())?
                .with_event(event)?
                .with_tls(tls_materials_provider)?;
            if let Some(limiter) = builder.endpoint_limits {
                server.with_endpoint_limits(limiter)?.start()?
            } else {
                server.start()?
            }
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
            return;
        }
    };

    let _ = on_ready.send(Ok(server.local_addr().unwrap()));

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
        let io = s2n_quic::provider::io::default::Builder::default()
            .with_receive_address(addr)?
            .with_base_mtu(DEFAULT_BASE_MTU.min(builder.mtu))?
            .with_initial_mtu(builder.mtu)?
            .with_max_mtu(builder.mtu)?
            .with_internal_recv_buffer_size(BUFFER_SIZE)?
            .build()?;

        let client = s2n_quic::Client::builder().with_io(io)?;

        let connection_limits = s2n_quic::provider::limits::Limits::new()
            .with_max_idle_timeout(builder.max_idle_timeout)?
            .with_data_window(builder.data_window)?
            .with_bidirectional_local_data_window(builder.data_window)?
            .with_bidirectional_remote_data_window(builder.data_window)?
            .with_initial_round_trip_time(DEFAULT_INITIAL_RTT)?;

        let event = ((ConfirmComplete, MtuConfirmComplete), subscriber);

        let client = client
            .with_limits(connection_limits)?
            .with_dc(map.clone())?
            .with_event(event)?
            .with_tls(tls_materials_provider)?
            .start()?;

        Ok(Self {
            client,
            map: map.clone(),
            queue: Arc::new(HandshakeQueue::new(builder.success_jitter)),
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
struct HandshakeQueue {
    inner: Mutex<HandshakeQueueInner>,
    limiter_start: Semaphore,
    limiter_inflight: Arc<Semaphore>,
    success_jitter: Duration,
    hasher: std::collections::hash_map::RandomState,
}

impl HandshakeQueue {
    fn new(success_jitter: Duration) -> Self {
        HandshakeQueue {
            // The "start" limiter bounds TLS handshake concurrency.
            //
            // TLS handshakes have high CPU cost (~1ms) which stalls out the endpoint, we don't
            // want too many of those to build up at the same time since that stalls out the
            // endpoint, increasing our baseline latency ~linearly with increases here. For
            // example, 5 here translates to 5ms avg handshake latency in our benchmarks. Lowering
            // it to 2-3 reduces our latencies to the expected 2-3ms (now hitting lowest possible
            // cost for the CPU work needed for a handshake).
            limiter_start: Semaphore::new(5),
            // The inflight limiter bounds the total number of connections we have open. Keeping
            // that bounded helps avoid unbounded work ongoing in s2n-quic (which implies unbounded
            // packet transmit/receive work), and helps our benchmarks exercise the maximum
            // concurrency within s2n-quic. We haven't found a particular stress test for which is
            // meaningful yet though.
            limiter_inflight: Arc::new(Semaphore::new(750)),
            success_jitter,
            inner: Default::default(),
            hasher: Default::default(),
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
        match inner.table.entry(
            peer_hash,
            |e| e.peer == peer,
            |e| self.hasher.hash_one(e.peer),
        ) {
            hashbrown::hash_table::Entry::Occupied(o) => {
                let entry = o.get().clone();
                entry.by_reason[reason as usize]
                    .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |v| {
                        Some(v.saturating_add(1))
                    })
                    .expect("Some means always OK");
                entry
            }
            hashbrown::hash_table::Entry::Vacant(v) => {
                let entry = v
                    .insert(Arc::new(Entry {
                        peer,
                        handshaker: tokio::sync::OnceCell::new(),
                        by_reason: [const { AtomicU16::new(0) }; REASON_COUNT],
                    }))
                    .get()
                    .clone();
                entry
            }
        }
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
            // this permit will only be used for the current handshake.
            let start = std::time::Instant::now();
            let permit_inflight = self.limiter_inflight.clone().acquire_owned().await;
            let permit_start = self.limiter_start.acquire().await;
            let limiter_duration = start.elapsed();

            let mut attempt =
                client.connect(s2n_quic::client::Connect::new(peer).with_server_name(server_name));

            // Note that this provides counts at the time of starting the connection attempt.
            // Technically, this omits counts that happen after this point while the deduplication
            // is still active.
            let mut reason_counts = [
                (HandshakeReason::User, 0),
                (HandshakeReason::Periodic, 0),
                (HandshakeReason::Remote, 0),
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
                    return Err(e);
                }
                Err(_elapsed) => {
                    // Handshake timeout occurred. We should treat the handshake as failed.
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
            tokio::spawn(async move {
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

                    let this = self.clone();
                    tokio::spawn(async move {
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
                        let duration = {
                            let mut rng = rand::rng();
                            rng.random_range(1000..120_000)
                        };
                        tokio::time::sleep(Duration::from_millis(duration)).await;

                        // If the handshake fails, we also remove the entry from the map.
                        // This permits another handshake to start for the same peer.
                        this.remove_entry(&entry3);
                    });

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
}

const REASON_COUNT: usize = 3;

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
    use tokio::net::UdpSocket;
    use tokio_util::sync::DropGuard;

    /// A test limiter that closes all incoming connections immediately
    #[derive(Default)]
    struct CloseAllConnectionsLimiter;

    impl Limiter for CloseAllConnectionsLimiter {
        fn on_connection_attempt(&mut self, _info: &ConnectionAttempt) -> Outcome {
            Outcome::close()
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
        async fn new<L>(endpoint_limits: Option<L>) -> Self
        where
            L: s2n_quic::provider::endpoint_limits::Limiter + Send + Sync + 'static,
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

            let server_builder = crate::psk::server::Builder::default();
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
            };

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
                crate::psk::client::Builder::default().with_success_jitter(Duration::ZERO),
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
        let setup = TestSetup::new::<CloseAllConnectionsLimiter>(None).await;
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
        let setup = TestSetup::new(Some(CloseAllConnectionsLimiter)).await;
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

    // Tests that the ROUTER cBPF filter correctly routes packets to the appropriate socket.
    #[cfg(target_os = "linux")]
    #[tokio::test]
    #[cfg_attr(miri, ignore)]
    async fn router_cbpf_packet_filtering_test() -> io::Result<()> {
        static IPV4_LOCALHOST: &str = "127.0.0.1:0";

        // Create two rx sockets bound to same port with SO_REUSEPORT
        let rx_socket_0 = syscall::bind_udp(IPV4_LOCALHOST, false, false, false)?;
        rx_socket_0.set_nonblocking(true)?;
        let port = rx_socket_0.local_addr()?.as_socket().unwrap().port();
        rx_socket_0.set_reuse_port(true)?;

        let rx_socket_1 = syscall::bind_udp(("127.0.0.1", port), false, true, false)?;
        rx_socket_1.set_nonblocking(true)?;

        // Attach ROUTER to both sockets
        ROUTER.attach(&rx_socket_0)?;
        ROUTER.attach(&rx_socket_1)?;

        // Convert to tokio sockets for async recv
        let rx_socket_0 = UdpSocket::from_std(rx_socket_0.into())?;
        let rx_socket_1 = UdpSocket::from_std(rx_socket_1.into())?;

        // Create sender socket
        let sender = UdpSocket::bind("127.0.0.1:0").await?;
        let target_addr: std::net::SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();

        // Test packet A: Initial packet with DCID length = 8
        // Should route to socket 0
        // Format: [header byte, version (4 bytes), dcid_len, ...]
        let packet_a = {
            let mut p = vec![0u8; 32];
            p[0] = 0xC0; // Initial packet (first 4 bits = 1100)
            p[1..5].copy_from_slice(&[0x00, 0x00, 0x00, 0x01]); // version
            p[5] = 0x08; // DCID length = 8
            p
        };

        // Test packet B: Handshake packet
        // Should route to socket 1
        let packet_b = {
            let mut p = vec![0u8; 32];
            p[0] = 0xE0; // Handshake packet first four bits are 1110
            p
        };

        // Test packet C: Initial packet but DCID length != 8
        // Should route to socket 1
        let packet_c = {
            let mut p = vec![0u8; 32];
            p[0] = 0xC0; // Initial packet (first 4 bits = 1100)
            p[1..5].copy_from_slice(&[0x00, 0x00, 0x00, 0x01]); // version
            p[5] = 0x10; // DCID length = 16
            p
        };

        // Send packets
        sender.send_to(&packet_a, target_addr).await?;
        sender.send_to(&packet_b, target_addr).await?;
        sender.send_to(&packet_c, target_addr).await?;

        // Receive and verify routing
        let mut buf_socket0 = [0u8; 1024];
        let mut buf_packet1_socket1 = [0u8; 1024];
        let mut buf_packet2_socket1 = [0u8; 1024];

        // Socket 0 should receive packet_a (Initial with DCID len = 8)
        let recv_result = tokio::time::timeout(
            Duration::from_millis(500),
            rx_socket_0.recv_from(&mut buf_socket0),
        )
        .await;
        assert!(
            recv_result.is_ok(),
            "Socket 0 should receive packet_a (Initial with DCID len=8)"
        );
        let (len, _) = recv_result.unwrap()?;
        assert_eq!(
            buf_socket0[0], 0xC0,
            "Socket 0 should receive Initial packet"
        );
        assert_eq!(
            buf_socket0[5], 0x08,
            "Socket 0 should receive packet with DCID len=8"
        );
        assert_eq!(len, 32);

        // Socket 1 should receive packet_b and packet_c
        let recv_result = tokio::time::timeout(
            Duration::from_millis(500),
            rx_socket_1.recv_from(&mut buf_packet1_socket1),
        )
        .await;
        assert!(
            recv_result.is_ok(),
            "Socket 1 should receive packet_b or packet_c"
        );
        let (len1, _) = recv_result.unwrap()?;
        assert_eq!(len1, 32);

        let recv_result = tokio::time::timeout(
            Duration::from_millis(500),
            rx_socket_1.recv_from(&mut buf_packet2_socket1),
        )
        .await;
        assert!(
            recv_result.is_ok(),
            "Socket 1 should receive packet_b or packet_c"
        );
        let (len2, _) = recv_result.unwrap()?;
        assert_eq!(len2, 32);

        // Verify that socket 1 received exactly packet_b and packet_c in either order
        let received_packets = [&buf_packet1_socket1[..32], &buf_packet2_socket1[..32]];

        // Check that one packet matches packet_b (Handshake: header = 0xE0)
        let has_packet_b = received_packets
            .iter()
            .any(|p| p[0] == 0xE0 && p[..] == packet_b[..]);
        assert!(
            has_packet_b,
            "Socket 1 should receive packet_b (Handshake packet with header 0xE0)"
        );

        // Check that one packet matches packet_c (Initial with DCID len = 16)
        let has_packet_c = received_packets
            .iter()
            .any(|p| p[0] == 0xC0 && p[5] == 0x10 && p[..] == packet_c[..]);
        assert!(
            has_packet_c,
            "Socket 1 should receive packet_c (Initial packet with DCID len=16)"
        );

        // Socket 0 should not have any more packets
        let recv_result = tokio::time::timeout(
            Duration::from_millis(100),
            rx_socket_0.recv_from(&mut buf_socket0),
        )
        .await;
        assert!(
            recv_result.is_err(),
            "Socket 0 should not receive any more packets"
        );

        Ok(())
    }
}

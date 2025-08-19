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
    Connection,
};
use std::{
    hash::BuildHasher,
    io,
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::Semaphore;

pub use crate::stream::DEFAULT_IDLE_TIMEOUT;
pub const DEFAULT_MAX_DATA: u64 = 1u64 << 25;
pub const DEFAULT_BASE_MTU: u16 = 1450;
#[cfg(target_os = "linux")]
pub const DEFAULT_MTU: u16 = 8940;
#[cfg(not(target_os = "linux"))]
pub const DEFAULT_MTU: u16 = DEFAULT_BASE_MTU;
const DEFAULT_INITIAL_RTT: Duration = Duration::from_millis(1);

const BUFFER_SIZE: usize = 16 * 1024;

pub type Error = Box<dyn std::error::Error + Send + Sync + 'static>;

pub type Result<T = (), E = Error> = core::result::Result<T, E>;

pub struct Server {
    server: s2n_quic::Server,
}

impl Server {
    pub fn bind<
        Provider: Prov + Clone + Send + Sync + 'static,
        Subscriber: Sub + Send + Sync + 'static,
        Event: s2n_quic::provider::event::Subscriber,
    >(
        addr: SocketAddr,
        map: secret::Map,
        tls_materials_provider: Provider,
        subscriber: Subscriber,
        builder: server::Builder<Event>,
    ) -> Result<Self, Error> {
        let io = s2n_quic::provider::io::default::Builder::default()
            .with_receive_address(addr)?
            .with_base_mtu(DEFAULT_BASE_MTU.min(builder.mtu))?
            .with_initial_mtu(builder.mtu)?
            .with_max_mtu(builder.mtu)?
            .with_internal_recv_buffer_size(BUFFER_SIZE)?
            .build()?;

        let server = s2n_quic::Server::builder().with_io(io)?;

        let connection_limits = s2n_quic::provider::limits::Limits::new()
            .with_max_idle_timeout(builder.max_idle_timeout)?
            .with_data_window(builder.data_window)?
            .with_bidirectional_local_data_window(builder.data_window)?
            .with_bidirectional_remote_data_window(builder.data_window)?
            .with_initial_round_trip_time(DEFAULT_INITIAL_RTT)?;
        let event = (ConfirmComplete, subscriber);

        let server = server
            .with_limits(connection_limits)?
            .with_dc(map.clone())?
            .with_event(event)?
            .with_tls(tls_materials_provider)?
            .start()?;

        Ok(Self { server })
    }

    #[allow(dead_code)]
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.server.local_addr()
    }
}

pub(super) async fn server<
    Provider: Prov + Clone + Send + Sync + 'static,
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
        tokio::spawn(async move {
            // The accepted connection must remain open until the client has finished inserting
            // the entry into its map. The client indicates this by sending a ConnectionClose
            // when it is done. This will cause the `connection.accept()` to return and allow
            // the server's connection to be dropped.
            //
            // A 10 second timeout is specified to avoid spawned tasks piling up when the
            // ConnectionClose from the client is lost.
            let success = tokio::time::timeout(
                Duration::from_secs(10),
                ConfirmComplete::wait_ready(&mut connection),
            )
            .await
            .is_ok_and(|inner| inner.is_ok());

            if success {
                // If the handshake completes successfully, the connection is left open for a
                // little longer to allow for MTU probing to complete. Depending on MTU configuration
                // this is likely to complete immediately, but a 10 second timeout is specified to
                // avoid spawned tasks piling up if the other end of the connection terminates ungracefully.
                let _ = tokio::time::timeout(
                    Duration::from_secs(10),
                    MtuConfirmComplete::wait_ready(&mut connection),
                )
                .await;
                // Leave the connection open for 1 more second to allow the peer
                // to finish MTU probing as well
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        });
    }
}

#[derive(Clone)]
pub struct Client {
    client: s2n_quic::Client,

    queue: Arc<HandshakeQueue>,
}

impl Client {
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.client.local_addr()
    }

    pub fn bind<
        Provider: Prov + Clone + Send + Sync + 'static,
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
        let event = (ConfirmComplete, subscriber);

        let client = client
            .with_limits(connection_limits)?
            .with_dc(map.clone())?
            .with_event(event)?
            .with_tls(tls_materials_provider)?
            .start()?;

        Ok(Self {
            client,
            queue: Arc::new(HandshakeQueue::new()),
        })
    }

    pub(super) async fn connect(
        &self,
        peer: SocketAddr,
        query_event_callback: fn(&mut Connection, Duration),
        server_name: String,
    ) -> Result<(), HandshakeFailed> {
        self.queue
            .clone()
            .handshake(&self.client, peer, query_event_callback, server_name)
            .await
    }
}

struct Entry {
    peer: SocketAddr,
    handshaker: tokio::sync::OnceCell<Result<(), HandshakeFailed>>,
}

#[derive(Default)]
struct HandshakeQueueInner {
    table: hashbrown::HashTable<Arc<Entry>>,
}
struct HandshakeQueue {
    inner: Mutex<HandshakeQueueInner>,
    limiter_start: Semaphore,
    limiter_inflight: Arc<Semaphore>,
    hasher: std::collections::hash_map::RandomState,
}

impl HandshakeQueue {
    fn new() -> Self {
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
            inner: Default::default(),
            hasher: Default::default(),
        }
    }

    /// Allocate an entry that will let us wait for the handshake to complete.
    /// This entry also stores the result of the handshake (success or failure).
    fn allocate_entry(&self, peer: SocketAddr) -> Arc<Entry> {
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
            hashbrown::hash_table::Entry::Occupied(o) => o.get().clone(),
            hashbrown::hash_table::Entry::Vacant(v) => {
                let entry = v
                    .insert(Arc::new(Entry {
                        peer,
                        handshaker: tokio::sync::OnceCell::new(),
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
        peer: SocketAddr,
        query_event_callback: fn(&mut Connection, Duration),
        server_name: String,
    ) -> Result<(), HandshakeFailed> {
        let entry = self.allocate_entry(peer);
        let entry2 = entry.clone();
        let entry3 = entry.clone();

        let handshake = async {
            // We've de-duplicated above already so the handshaker is unique per SocketAddr, so
            // this permit will only be used for the current handshake.
            let start = std::time::Instant::now();
            let permit_inflight = self.limiter_inflight.clone().acquire_owned().await;
            let permit_start = self.limiter_start.acquire().await;
            let limiter_duration = start.elapsed();

            let mut connection = client
                .connect(s2n_quic::client::Connect::new(peer).with_server_name(server_name))
                .await?;

            query_event_callback(&mut connection, limiter_duration);

            // we need to wait for confirmation that the dcQUIC handshake is complete
            // TODO: This will not be needed if https://github.com/aws/s2n-quic/issues/2273 is addressed
            ConfirmComplete::wait_ready(&mut connection).await?;

            // Don't wait for the connection to fully close, just wait until dc.complete to
            // drop the permit.
            drop(permit_start);

            // Spawn a task to leave the connection open for a little longer to allow for MTU
            // probing to complete. Depending on MTU configuration this is likely to complete
            // immediately, but a 10 second timeout is specified to avoid spawned tasks piling
            // up if the other end of the connection terminates ungracefully.
            let this = self.clone();
            tokio::spawn(async move {
                let _ = tokio::time::timeout(
                    Duration::from_secs(10),
                    MtuConfirmComplete::wait_ready(&mut connection),
                )
                .await;
                // Leave the connection open for 1 more second to allow the peer
                // to finish MTU probing as well
                tokio::time::sleep(Duration::from_secs(1)).await;

                this.remove_entry(&entry);

                drop(connection);
                drop(permit_inflight);
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
                            rng.random_range(1..120)
                        };
                        tokio::time::sleep(Duration::from_secs(duration)).await;

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

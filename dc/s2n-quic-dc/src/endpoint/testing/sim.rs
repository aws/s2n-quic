// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Helpers for building stream endpoint instances inside Bach simulations.
//!
//! The two main high-level entry points are [`Server`] and [`Client`]:
//!
//! ```ignore
//! // Server group
//! let server = Server::new();
//! let acceptor = server.register_acceptor_channel(VarInt::from_u8(1), 8).unwrap();
//! while let Ok(stream) = acceptor.recv().await { /* … */ }
//!
//! // Client group
//! let client = Client::new();
//! let stream = client.connect("server:4433", VarInt::from_u8(1)).await.unwrap();
//! ```
//!
//! Both types lazily create exactly one [`Endpoint`] per Bach group (simulated machine).
//! [`Server`] binds its recv/send sockets to the well-known port [`SERVER_PORT`] so
//! the client can resolve the peer address purely by group name (via `bach::net::lookup_host`).
//!
//! For lower-level access, [`setup_sim_endpoint`], [`connect`], and
//! [`insert_fake_path_pair`] are also available.

use crate::{
    acceptor,
    acceptor::channel as accept_channel,
    flow,
    path::secret::{map::TestPairIds, Map as PathSecretMap},
    socket::{pool::Pool, rate::Rate},
    stream::{
        endpoint::{msg, setup_endpoint, Budgets, Config, Endpoint, WorkerLayout},
        PendingValidation, Reader, Stream, Writer,
    },
};
use core::net::SocketAddr;
use s2n_quic_core::varint::VarInt;
use std::{
    cell::RefCell,
    collections::HashMap,
    io,
    sync::{atomic::Ordering, Arc, Weak},
};

// ── Thread-local endpoint registries ─────────────────────────────────────────
//
// Bach runs all tasks on a single OS thread, so `thread_local!` state is safe
// for inter-task communication within a simulation run.
//
// SIM_MAP_REGISTRY: stores each endpoint's PathSecretMap keyed by data_addr.
//   `connect()` uses this to auto-insert fake path secrets without the caller
//   having to thread maps through test code.
//
// SIM_ENDPOINT_BY_GROUP: stores one Weak<Endpoint> per Bach group (keyed by
//   group.id()).  A Weak reference is intentional: the strong Arc is held by
//   the Server/Client structs, which are dropped *inside* the Bach simulation
//   (Server when Bach cancels the non-primary task; Client when its task
//   completes).  At that point all Bach TLS is still live, so the
//   SubmissionSender::drop → AtomicWaker::wake() call that notifies the frame
//   dispatch receiver safely runs inside the runtime.
//
//   After the simulation ends, this TLS holds only dead Weak references, so
//   thread-exit TLS destruction is trivially cheap and cannot panic by trying
//   to schedule a Bach task in an already-torn-down runtime.

thread_local! {
    static SIM_MAP_REGISTRY: RefCell<HashMap<SocketAddr, PathSecretMap>> =
        RefCell::new(HashMap::new());

    static SIM_ADDR_REGISTRY: RefCell<HashMap<SocketAddr, Vec<SocketAddr>>> =
        RefCell::new(HashMap::new());

    static SIM_ENDPOINT_BY_GROUP: RefCell<HashMap<u64, Weak<Endpoint>>> =
        RefCell::new(HashMap::new());
}

fn register_endpoint_map(data_addrs: &[SocketAddr], map: PathSecretMap) {
    SIM_MAP_REGISTRY.with(|r| {
        let mut reg = r.borrow_mut();
        for &addr in data_addrs {
            reg.insert(addr, map.clone());
        }
    });
    SIM_ADDR_REGISTRY.with(|r| {
        let mut reg = r.borrow_mut();
        let addrs = data_addrs.to_vec();
        for &addr in data_addrs {
            reg.insert(addr, addrs.clone());
        }
    });
}

fn lookup_peer_data_addrs(any_addr: SocketAddr) -> Vec<SocketAddr> {
    SIM_ADDR_REGISTRY
        .with(|r| r.borrow().get(&any_addr).cloned())
        .unwrap_or_else(|| vec![any_addr])
}

/// Well-known server identifier port used by sim endpoint discovery.
///
/// # ⚠️ WARNING
///
/// In simulation, this is a stable *lookup identifier* for the server group, not
/// the destination port used for runtime data packets. `Client::connect` resolves
/// the group name and then rewrites traffic to the server's advertised data
/// addresses.
///
/// 4433 is the QUIC/IETF QUIC standard port and is conventionally used for DC
/// (datagram-capable) connections. Using a well-known identifier means clients
/// can resolve the peer by group name alone, without an out-of-band channel:
/// ```ignore
/// client.connect("server:4433", acceptor_id).await
/// ```
pub const SERVER_PORT: u16 = 4433;

/// Returns the shared [`Endpoint`] for the current Bach group, creating it lazily.
///
/// `bind_addr` is used only on the first call for a given group (or after a
/// previous endpoint for that group has been fully dropped); subsequent calls
/// upgrade the cached [`Weak`] reference and return the same [`Endpoint`].
fn get_or_create_group_endpoint(bind_addr: SocketAddr) -> Arc<Endpoint> {
    let group_id = bach::group::current().id();
    SIM_ENDPOINT_BY_GROUP.with(|r| {
        let mut map = r.borrow_mut();
        if let Some(weak) = map.get(&group_id) {
            if let Some(ep) = weak.upgrade() {
                return ep;
            }
        }
        let path_secret_map = crate::path::secret::map::testing::new(50_000);
        let acceptor_registry = acceptor::Registry::new();
        let config = SimEndpointConfig {
            bind_addr,
            ..SimEndpointConfig::default()
        };
        let ep = Arc::new(setup_sim_endpoint(
            config,
            path_secret_map,
            acceptor_registry,
        ));
        map.insert(group_id, Arc::downgrade(&ep));
        ep
    })
}

// ── SimEndpointConfig ─────────────────────────────────────────────────────────

/// Describes how to create a simulated endpoint.
///
/// All fields are public so callers can override individual values; the
/// [`Default`] implementation supplies sensible testing values.
pub struct SimEndpointConfig {
    /// Address to bind the send + recv sockets to.
    ///
    /// Use `0.0.0.0:0` (the default) to let Bach assign the group-local IP and
    /// pick an ephemeral port.  Avoid hard-coding `127.0.0.1` or `[::1]` here —
    /// Bach replaces those with the group-assigned IP anyway, and using an
    /// IPv4/IPv6-specific address forces a specific address family.
    pub bind_addr: SocketAddr,

    /// Number of send sockets.  Must be a power of two (≥ 1).
    pub num_send_sockets: usize,

    /// Number of submission shards for the frame channel.  Must be a power of two.
    pub submission_shards: usize,

    /// Overall send rate cap (Gbps).
    pub overall_send_rate: Rate,

    /// Per-socket send rate cap (Gbps).
    pub per_socket_send_rate: Rate,

    /// Per-poll budgets.
    pub budgets: Budgets,

    /// Maximum transfer unit for the send / recv buffer pools (bytes).
    pub mtu: u16,
}

impl Default for SimEndpointConfig {
    fn default() -> Self {
        Self {
            bind_addr: SocketAddr::new(std::net::Ipv4Addr::UNSPECIFIED.into(), 0),
            num_send_sockets: 1,
            submission_shards: 1,
            overall_send_rate: Rate::new(25.0),
            per_socket_send_rate: Rate::new(5.0),
            budgets: Budgets::default(),
            mtu: 1500,
        }
    }
}

// ── setup_sim_endpoint ────────────────────────────────────────────────────────

/// Builds a stream [`Endpoint`] wired to Bach simulated UDP sockets.
///
/// All pipeline tasks are pinned to worker 0 (Bach is single-threaded).
/// The returned `Endpoint` is ready for use inside a `testing::sim` closure.
///
/// The endpoint's data address is automatically registered in the thread-local
/// sim endpoint registry so that [`connect`] can find the peer's path-secret map
/// without the caller having to pass it explicitly.
pub fn setup_sim_endpoint(
    config: SimEndpointConfig,
    path_secret_map: PathSecretMap,
    acceptor_registry: acceptor::Registry<PendingValidation>,
) -> Endpoint {
    let SimEndpointConfig {
        bind_addr,
        num_send_sockets,
        submission_shards,
        overall_send_rate,
        per_socket_send_rate,
        budgets,
        mtu,
    } = config;

    assert!(
        num_send_sockets.is_power_of_two(),
        "num_send_sockets must be a power of two"
    );
    assert!(
        submission_shards.is_power_of_two(),
        "submission_shards must be a power of two"
    );

    // Bind send sockets using the synchronous constructor so this function can
    // be called before the Bach runtime drains async tasks.
    // Send sockets always use an ephemeral port — only the recv socket needs
    // the configured (potentially well-known) address.
    let send_bind_opts = {
        let mut o = bach::net::socket::Options::default();
        o.local_addr = SocketAddr::new(bind_addr.ip(), 0);
        o
    };
    let recv_bind_opts = {
        let mut o = bach::net::socket::Options::default();
        o.local_addr = bind_addr;
        o
    };

    let send_sockets: Vec<Arc<bach::net::UdpSocket>> = (0..num_send_sockets)
        .map(|_| {
            let sock =
                bach::net::UdpSocket::new(&send_bind_opts).expect("failed to bind send socket");
            Arc::new(sock)
        })
        .collect();

    // Bind a single recv socket.
    let recv_socket =
        bach::net::UdpSocket::new(&recv_bind_opts).expect("failed to bind recv socket");
    let recv_sockets = vec![recv_socket];

    let send_pool = Pool::new(mtu);
    let recv_pool = Pool::new(mtu);

    // Update path secret map so it knows how many sender slots to allocate.
    path_secret_map.set_socket_sender_count(num_send_sockets);

    // All workers on index 0 — Bach is single-threaded.
    let layout = WorkerLayout {
        frame_dispatch: 0,
        send: vec![0],
        recv_io: vec![0],
        recv_dispatch: vec![0],
        waker_drain: vec![0],
        background: 0,
    };

    let runtime = crate::runtime::bach::Handle::new(1);
    let gso = s2n_quic_platform::features::Gso::default();

    let endpoint_config = Config {
        layout,
        send_pool,
        recv_pool,
        path_secret_map: path_secret_map.clone(),
        gso,
        acceptor_registry,
        overall_send_rate,
        per_socket_send_rate,
        budgets,
        submission_shards,
    };

    let endpoint = setup_endpoint(runtime, endpoint_config, send_sockets, recv_sockets);

    endpoint.counters.spawn_reporter_with_label(
        core::time::Duration::from_secs(1),
        bach::group::current().name(),
    );

    // Register in the thread-local registry so `connect` can find it.
    register_endpoint_map(&endpoint.data_addrs, path_secret_map);

    endpoint
}

// ── connect ───────────────────────────────────────────────────────────────────

/// Ensures a fake path-secret pair exists between `local_endpoint` and the
/// peer at `peer_addr`, then returns the local path-secret entry for `peer_addr`.
///
/// Path secrets are inserted the first time; if they already exist (e.g. because
/// the test called this function twice) the existing entries are reused.
///
/// Both the local and peer endpoints must have been created via
/// [`setup_sim_endpoint`] in the same sim run so that both maps are registered.
///
/// # Panics
///
/// Panics if `peer_addr` has not been registered (i.e. no endpoint with that
/// data address was created via [`setup_sim_endpoint`] in this sim run).
pub fn connect(
    local_endpoint: &Endpoint,
    peer_addr: SocketAddr,
) -> Arc<crate::path::secret::map::Entry> {
    let local_addr = local_endpoint.data_addrs[0];
    let local_map = &local_endpoint.path_secret_map;

    // Fast path: already connected.
    if let Some(entry) = local_map.get_raw(peer_addr) {
        return entry;
    }

    // Look up the peer's map from the registry.
    let peer_map = SIM_MAP_REGISTRY
        .with(|r| r.borrow().get(&peer_addr).cloned())
        .unwrap_or_else(|| {
            panic!(
                "no sim endpoint registered at {peer_addr}; \
                 call setup_sim_endpoint before connect"
            )
        });

    insert_fake_path_pair(local_map, local_addr, &peer_map, peer_addr);

    let entry = local_map
        .get_raw(peer_addr)
        .expect("path-secret entry just inserted by insert_fake_path_pair");

    // Set the peer's full recv address list (simulates the post-handshake exchange).
    let peer_data_addrs = lookup_peer_data_addrs(peer_addr);
    entry.set_peer_data_addrs(&peer_data_addrs);

    // Also set our addrs on the peer's entry for us.
    if let Some(peer_entry) = peer_map.get_raw(local_addr) {
        peer_entry.set_peer_data_addrs(&local_endpoint.data_addrs);
    }

    entry
}

// ── insert_fake_path_pair ─────────────────────────────────────────────────────

/// Inserts a pair of matching fake path-secret entries so that two simulated
/// endpoints can exchange encrypted packets without a handshake.
///
/// `local_addr` is the address the peer (at `peer_map`) should send packets to.
/// `peer_addr` is the address the local endpoint (at `local_map`) should target.
///
/// Both maps receive reciprocal entries with the same shared secret. Returns the
/// common credential ID.
///
/// The socket sender count used for the new entries is read from each map; call
/// [`setup_sim_endpoint`] (which calls [`Map::set_socket_sender_count`]) before
/// this function so the entries are allocated with the correct number of sender
/// slots.
pub fn insert_fake_path_pair(
    local_map: &PathSecretMap,
    local_addr: SocketAddr,
    peer_map: &PathSecretMap,
    peer_addr: SocketAddr,
) -> TestPairIds {
    use s2n_quic_core::dc::testing::TEST_APPLICATION_PARAMS;

    let mut params = TEST_APPLICATION_PARAMS;
    params.remote_max_data = params.local_recv_max_data;

    local_map.test_insert_pair(
        local_addr,
        Some(params.clone()),
        peer_map,
        peer_addr,
        Some(params),
    )
}

// ── Server ─────────────────────────────────────────────────────────────────

/// High-level sim server that lazily creates one [`Endpoint`] per Bach group.
///
/// The underlying endpoint is bound to [`SERVER_PORT`] so clients can resolve
/// the peer address using only the group name:
/// ```ignore
/// let stream = client.connect("server:4433", acceptor_id).await.unwrap();
/// ```
///
/// Acceptors are registered via [`Server::register_acceptor_channel`], which
/// returns an [`mpmc::Receiver<PendingValidation>`] that yields accepted streams.
pub struct Server {
    endpoint: Arc<Endpoint>,
}

impl Server {
    /// Returns the sim server for the current Bach group, creating it if needed.
    ///
    /// Binds sockets to `0.0.0.0:`[`SERVER_PORT`].  Call this inside the group
    /// task (after `.group("name").spawn()`) so the socket is associated with the
    /// correct simulated machine.
    pub fn new() -> Self {
        let bind_addr = SocketAddr::new(std::net::Ipv4Addr::UNSPECIFIED.into(), SERVER_PORT);
        let endpoint = get_or_create_group_endpoint(bind_addr);
        Self { endpoint }
    }

    /// Returns the first bound data address (for use as a connection target).
    pub fn data_addr(&self) -> SocketAddr {
        self.endpoint.data_addrs[0]
    }

    /// Returns all recv data addresses advertised to peers.
    pub fn data_addrs(&self) -> &[SocketAddr] {
        &self.endpoint.data_addrs
    }

    /// Register a channel-based acceptor for incoming streams.
    ///
    /// Returns an [`accept_channel::Receiver<PendingValidation>`] that yields accepted streams.
    /// The acceptor is automatically unregistered when all receivers are dropped.
    pub fn register_acceptor_channel(
        &self,
        acceptor_id: VarInt,
        capacity: usize,
    ) -> io::Result<accept_channel::Receiver<PendingValidation>> {
        use crate::stream::server::ChannelAcceptor;

        let (tx, rx) = accept_channel::new(capacity.into());
        let acceptor = Arc::new(ChannelAcceptor::new(tx));
        let handle = self
            .endpoint
            .acceptor_registry
            .register(acceptor_id, acceptor.clone())
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::AddrInUse, "acceptor ID already registered")
            })?;
        acceptor.set_handle(handle);
        Ok(rx)
    }
}

impl Default for Server {
    fn default() -> Self {
        Self::new()
    }
}

// ── Client ─────────────────────────────────────────────────────────────────

/// High-level sim client that lazily creates one [`Endpoint`] per Bach group.
///
/// Call [`Client::connect`] to resolve the peer by group name, auto-insert
/// fake path-secrets, and get back a ready-to-use bidirectional [`Stream`]:
///
/// ```ignore
/// let mut client = Client::new();
/// let mut stream = client.connect("server:4433", VarInt::from_u8(1)).await?;
/// ```
pub struct Client {
    endpoint: Arc<Endpoint>,
    /// Cloned allocator so `connect` can call `alloc_or_grow(&mut self)` without
    /// holding a mutable reference to the Arc-wrapped endpoint.
    queue_allocator: msg::queue::Allocator,
}

impl Client {
    /// Returns the sim client for the current Bach group, creating it if needed.
    ///
    /// Binds sockets to an ephemeral port (`0.0.0.0:0`).  Call this inside the
    /// group task so the socket is associated with the correct simulated machine.
    pub fn new() -> Self {
        let bind_addr = SocketAddr::new(std::net::Ipv4Addr::UNSPECIFIED.into(), 0);
        let endpoint = get_or_create_group_endpoint(bind_addr);
        let queue_allocator = endpoint.queue_allocator.clone();
        Self {
            endpoint,
            queue_allocator,
        }
    }

    /// Returns the first bound data address (for use as a connection target).
    pub fn data_addr(&self) -> SocketAddr {
        self.endpoint.data_addrs[0]
    }

    /// Returns all recv data addresses advertised to peers.
    pub fn data_addrs(&self) -> &[SocketAddr] {
        &self.endpoint.data_addrs
    }

    /// Connect to a peer, returning a bidirectional [`Stream`].
    ///
    /// Resolves `peer` via [`bach::net::lookup_host`] (so `"server:4433"` works),
    /// auto-inserts fake path-secret entries into both endpoint maps if not already
    /// present, then allocates queues and constructs the `Stream`.
    ///
    /// The returned stream is ready to use: call [`Stream::write_all_from_fin`] /
    /// [`Stream::read_into`] (or the tokio `AsyncRead`/`AsyncWrite` impls) to
    /// exchange data with the server.
    pub async fn connect<A>(&mut self, peer: A, acceptor_id: VarInt) -> io::Result<Stream>
    where
        A: bach::net::ToSocketAddrs,
    {
        // Yield so the server group has had a chance to bind its socket.
        bach::task::yield_now().await;

        // Resolve hostname → SocketAddr (e.g. "server:4433" → <server-ip>:4433).
        let mut peer_addr = bach::net::lookup_host(peer)
            .await
            .map_err(|e| io::Error::new(io::ErrorKind::AddrNotAvailable, e))?
            .next()
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::AddrNotAvailable, "no address found for peer")
            })?;

        if peer_addr.port() == 0 {
            // If the peer didn't specify a port, use the default.
            peer_addr.set_port(SERVER_PORT);
        }

        // Auto-insert path secrets (idempotent: re-uses existing entry if present).
        let path_secret_entry = connect(&self.endpoint, peer_addr);

        // Allocate a fresh stream ID and flow queues.
        let stream_id = VarInt::new(self.endpoint.next_stream_id.fetch_add(1, Ordering::Relaxed))
            .expect("stream_id overflow");

        let handle = flow::Handle::client(stream_id, path_secret_entry.clone());
        let (queue_control, queue_stream) = self.queue_allocator.alloc_or_grow(handle, None);

        // Build Reader + Writer and wrap them in a Stream.
        let frame_tx = self.endpoint.frame_tx.clone();
        let writer = Writer::new_client(
            frame_tx.clone(),
            path_secret_entry.clone(),
            stream_id,
            acceptor_id,
            queue_control,
        );
        let reader = Reader::new_client(frame_tx, path_secret_entry, stream_id, queue_stream);

        Ok(Stream::new(reader, writer))
    }
}

impl Default for Client {
    fn default() -> Self {
        Self::new()
    }
}

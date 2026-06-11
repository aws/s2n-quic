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
//! For scenarios where one node should act as both server and client on the
//! same endpoint and path-secret map, use [`Peer`].
//!
//! For lower-level access, [`setup_sim_endpoint`], [`connect`], and
//! [`insert_fake_path_pair`] are also available.

use crate::{
    acceptor,
    acceptor::channel as accept_channel,
    path::secret::{
        map::{entry::QueueState, TestPairIds},
        Map as PathSecretMap,
    },
    socket::{pool::Pool, rate::Rate},
    stream::{
        endpoint::{setup_endpoint, Budgets, Config, Endpoint, WorkerLayout},
        Reader, Stream, Writer,
    },
};
use core::net::{IpAddr, SocketAddr};
use s2n_quic_core::{dc::ApplicationParams, varint::VarInt};
use std::{
    cell::RefCell,
    collections::HashMap,
    io,
    sync::{Arc, Weak},
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

    static SIM_PARAMS_REGISTRY: RefCell<HashMap<SocketAddr, ApplicationParams>> =
        RefCell::new(HashMap::new());
}

fn register_endpoint_map(data_addrs: &[SocketAddr], map: PathSecretMap, params: ApplicationParams) {
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
    SIM_PARAMS_REGISTRY.with(|r| {
        let mut reg = r.borrow_mut();
        for &addr in data_addrs {
            reg.insert(addr, params.clone());
        }
    });
}

fn lookup_sim_params(addr: SocketAddr) -> Option<ApplicationParams> {
    SIM_PARAMS_REGISTRY.with(|r| r.borrow().get(&addr).cloned())
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

/// Lazily resolves and caches a Bach group host address for monitor filters.
///
/// Resolution happens on first use via [`bach::net::try_lookup`] and then the
/// cached address is re-used for subsequent packet checks.
///
/// Packet checks intentionally compare only the IP (not the full socket
/// address), since endpoints can bind multiple ports on the same host IP.
#[derive(Clone, Debug)]
pub struct MonitorHostAddr {
    host: &'static str,
    addr: Option<SocketAddr>,
}

impl MonitorHostAddr {
    pub fn new(host: &'static str) -> Self {
        Self { host, addr: None }
    }

    fn addr(&mut self) -> SocketAddr {
        *self.addr.get_or_insert_with(|| {
            bach::net::try_lookup((self.host, SERVER_PORT)).unwrap_or_else(|err| {
                panic!(
                    "failed to resolve monitor host '{}' on port {}: {err}",
                    self.host, SERVER_PORT
                )
            })
        })
    }

    pub fn ip(&mut self) -> IpAddr {
        self.addr().ip()
    }

    pub fn is_packet_source(&mut self, packet: &bach::net::monitor::Packet) -> bool {
        packet.source().ip() == self.ip()
    }
}

/// Returns the shared [`Endpoint`] for the current Bach group, creating it lazily.
///
/// The config is used only on the first call for a given group (or after a
/// previous endpoint for that group has been fully dropped); subsequent calls
/// upgrade the cached [`Weak`] reference and return the same [`Endpoint`].
fn get_or_create_group_endpoint(config: SimEndpointConfig) -> Arc<Endpoint> {
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

    /// Number of recv sockets.  Distributed round-robin across recv_io workers.
    pub num_recv_sockets: usize,

    /// Number of recv_io workers.  Each worker handles one or more recv sockets.
    pub num_recv_io_workers: usize,

    /// Number of send workers.
    pub num_send_workers: usize,

    /// Number of recv dispatch workers.
    pub num_recv_dispatch_workers: usize,

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

    /// Local send window (local_send_max_data). When set, constrains how much data
    /// the writer can emit before receiving MAX_DATA from the peer.
    pub send_window: Option<VarInt>,

    /// Cooldown period for peers marked dead before new flows are allowed.
    pub dead_peer_cooldown: core::time::Duration,

    /// Recv credit pool config. Defaults to [`crate::credit::Config::default`] (a large,
    /// effectively non-contending pool). Override to a smaller capacity to exercise the recv-side
    /// fair-share distributor under contention from many concurrent streams (mirrors the dc-tester
    /// production sizing).
    pub recv_credit_pool_config: crate::credit::Config,

    /// Send credit pool config. Defaults to [`crate::credit::Config::default`].
    pub send_credit_pool_config: crate::credit::Config,
}

impl Default for SimEndpointConfig {
    fn default() -> Self {
        Self {
            bind_addr: SocketAddr::new(std::net::Ipv4Addr::UNSPECIFIED.into(), 0),
            num_send_sockets: 8,
            // TODO: support asymmetric send/recv socket counts — currently both sides
            // must be equal so that the shared-bidirectional-socket setup in
            // setup_sim_endpoint mirrors production Config::create(). Unequal counts
            // are structurally valid but the sim harness does not yet wire them up
            // correctly for symmetric-5-tuple routing tests.
            num_recv_sockets: 8,
            num_recv_io_workers: 2,
            num_send_workers: 4,
            num_recv_dispatch_workers: 4,
            submission_shards: 4,
            overall_send_rate: Rate::new(25.0),
            per_socket_send_rate: Rate::new(5.0),
            budgets: Budgets::default(),
            mtu: 1500,
            send_window: None,
            dead_peer_cooldown: crate::stream::endpoint::DEFAULT_DEAD_PEER_COOLDOWN,
            recv_credit_pool_config: crate::credit::Config::default(),
            send_credit_pool_config: crate::credit::Config::default(),
        }
    }
}

impl SimEndpointConfig {
    pub fn mtu(mut self, mtu: u16) -> Self {
        self.mtu = mtu;
        self
    }

    pub fn send_window(mut self, window: VarInt) -> Self {
        self.send_window = Some(window);
        self
    }

    pub fn overall_send_rate(mut self, rate: Rate) -> Self {
        self.overall_send_rate = rate;
        self
    }

    pub fn per_socket_send_rate(mut self, rate: Rate) -> Self {
        self.per_socket_send_rate = rate;
        self
    }

    pub fn recv_credit_pool_config(mut self, config: crate::credit::Config) -> Self {
        self.recv_credit_pool_config = config;
        self
    }

    pub fn send_credit_pool_config(mut self, config: crate::credit::Config) -> Self {
        self.send_credit_pool_config = config;
        self
    }

    pub fn server(self) -> Server {
        Server::with_config(self)
    }

    pub fn client(self) -> Client {
        Client::with_config(self)
    }

    pub fn peer(self) -> Peer {
        Peer::with_config(self)
    }
}

// ── setup_sim_endpoint ────────────────────────────────────────────────────────

/// Builds a stream [`Endpoint`] wired to Bach simulated UDP sockets.
///
/// Pipeline workers are spread across emulated bach worker IDs to exercise
/// multi-worker dispatch, fan-out, and affinity logic — even though bach is
/// single-threaded underneath.
///
/// The returned `Endpoint` is ready for use inside a `testing::sim` closure.
///
/// The endpoint's data address is automatically registered in the thread-local
/// sim endpoint registry so that [`connect`] can find the peer's path-secret map
/// without the caller having to pass it explicitly.
pub fn setup_sim_endpoint(
    config: SimEndpointConfig,
    path_secret_map: PathSecretMap,
    acceptor_registry: acceptor::Registry<Stream>,
) -> Endpoint {
    let SimEndpointConfig {
        bind_addr,
        num_send_sockets,
        num_recv_sockets,
        num_recv_io_workers,
        num_send_workers,
        num_recv_dispatch_workers,
        submission_shards,
        overall_send_rate,
        per_socket_send_rate,
        budgets,
        mtu,
        send_window,
        dead_peer_cooldown,
        recv_credit_pool_config,
        send_credit_pool_config,
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

    // Share the first min(send, recv) sockets between send and recv (bidirectional),
    // mirroring production Config::create() which uses try_clone(). This ensures
    // symmetric 5-tuple routing works: send_socket[i] and recv_socket[i] share the
    // same port for i < shared_count.
    let shared_count = num_send_sockets.min(num_recv_sockets);
    let mut send_sockets: Vec<Arc<bach::net::UdpSocket>> = Vec::with_capacity(num_send_sockets);
    let mut recv_sockets: Vec<Arc<bach::net::UdpSocket>> = Vec::with_capacity(num_recv_sockets);

    // Shared sockets: same underlying socket for both send and recv.
    for i in 0..shared_count {
        let opts = if i == 0 {
            &recv_bind_opts
        } else {
            &send_bind_opts
        };
        let sock = Arc::new(bach::net::UdpSocket::new(opts).expect("failed to bind shared socket"));
        send_sockets.push(sock.clone());
        recv_sockets.push(sock);
    }

    // Extra send-only sockets.
    for _ in shared_count..num_send_sockets {
        let sock = bach::net::UdpSocket::new(&send_bind_opts).expect("failed to bind send socket");
        send_sockets.push(Arc::new(sock));
    }

    // Extra recv-only sockets.
    for _ in shared_count..num_recv_sockets {
        let sock = bach::net::UdpSocket::new(&send_bind_opts).expect("failed to bind recv socket");
        recv_sockets.push(Arc::new(sock));
    }

    let send_pool = Pool::new(u16::MAX);
    let recv_pool = Pool::new(u16::MAX);

    // Update path secret map so it knows how many sender slots to allocate.
    path_secret_map.set_socket_sender_count(num_send_sockets);

    // Assign distinct worker IDs to each pipeline stage.
    let mut ids = 0usize..;
    let frame_dispatch = ids.next().unwrap();
    let send: Vec<usize> = (&mut ids).take(num_send_workers).collect();
    let recv_io: Vec<usize> = (&mut ids).take(num_recv_io_workers).collect();
    let recv_dispatch: Vec<usize> = (&mut ids).take(num_recv_dispatch_workers).collect();
    let waker_drain: Vec<usize> = (&mut ids).take(1).collect();
    let background = ids.next().unwrap();
    let worker_count = background + 1;

    let layout = WorkerLayout {
        frame_dispatch,
        send,
        recv_io,
        recv_dispatch,
        waker_drain,
        background,
    };

    let runtime = crate::runtime::bach::Handle::new(worker_count);
    let gso = s2n_quic_platform::features::Gso::default();

    let ups_socket =
        Arc::new(bach::net::UdpSocket::new(&send_bind_opts).expect("failed to bind UPS socket"));

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
        ups_rate: crate::socket::rate::Rate::new(0.001),
        ups_dedup_capacity: 1024,
        ups_dedup_window: core::time::Duration::from_secs(1),
        dead_peer_cooldown,
        initial_tx_descriptor_allocs: 0,
        initial_rx_descriptor_allocs: 0,
        send_credit_pool_config,
        recv_credit_pool_config,
    };

    let endpoint = setup_endpoint(
        runtime,
        endpoint_config,
        send_sockets,
        recv_sockets,
        ups_socket,
    );

    // Build ApplicationParams from the config for path-secret insertion.
    // Subtract IP + UDP headers to get the max datagram payload size.
    let ip_header_len: u16 = if endpoint.data_addrs[0].is_ipv4() {
        20
    } else {
        40
    };
    let udp_header_len: u16 = 8;
    let max_datagram_size = mtu - ip_header_len - udp_header_len;
    let mut params = s2n_quic_core::dc::testing::TEST_APPLICATION_PARAMS;
    params
        .max_datagram_size
        .store(max_datagram_size, core::sync::atomic::Ordering::Relaxed);
    params.remote_max_data = params.local_recv_max_data;
    if let Some(window) = send_window {
        params.local_send_max_data = window;
        params.local_recv_max_data = window;
        params.remote_max_data = window;
    }

    // Register in the thread-local registry so `connect` can find it.
    register_endpoint_map(&endpoint.data_addrs, path_secret_map, params);

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
        // Self-connect: get_raw resolves to the Server entry (the address index is
        // overwritten by the second insert in insert_fake_path_pair).  Always select
        // the Client entry so the Writer seals packets with the correct keys.
        if local_addr == peer_addr {
            let client_id = entry
                .id()
                .for_endpoint(s2n_quic_core::endpoint::Type::Client);
            return local_map
                .get_by_id(&client_id)
                .expect("self-connect Client entry must exist when get_raw succeeds");
        }
        // In bidirectional P2P scenarios, the address map may contain a Server entry
        // (created when the peer previously connected to us). Only use the fast path
        // if the entry has Client queue state.
        if matches!(
            entry.queue_state(),
            crate::path::secret::map::entry::QueueState::Client(_)
        ) {
            return entry;
        }
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

    let ids = insert_fake_path_pair(local_map, local_addr, &peer_map, peer_addr);

    // For self-connect (local_addr == peer_addr, same map), get_raw returns the last-inserted
    // entry which is the Server entry — wrong for the Writer side.  Look up the Client entry
    // (ids.local) explicitly so the Writer seals packets with the correct Client-type keys.
    let entry = if local_addr == peer_addr {
        local_map
            .get_by_id(&ids.local)
            .expect("client entry just inserted by insert_fake_path_pair")
    } else {
        local_map
            .get_raw(peer_addr)
            .expect("path-secret entry just inserted by insert_fake_path_pair")
    };

    // Set the peer's full recv address list (simulates the post-handshake exchange).
    let peer_data_addrs = lookup_peer_data_addrs(peer_addr);
    entry.set_peer_data_addrs(&peer_data_addrs);

    if local_addr == peer_addr {
        // Self-connect: also set peer_data_addrs on the Server entry so that the
        // acceptor-side Writer can route the echo packets back.
        if let Some(server_entry) = local_map.get_by_id(&ids.peer) {
            server_entry.set_peer_data_addrs(&peer_data_addrs);
        }
    } else {
        // Set our addrs on the peer's server-side entry (looked up by ID since
        // server entries are not in the address-keyed client map).
        if let Some(peer_entry) = peer_map.get_by_id(&ids.peer) {
            peer_entry.set_peer_data_addrs(&local_endpoint.data_addrs);
        }
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
    let local_params = lookup_sim_params(local_addr);
    let peer_params = lookup_sim_params(peer_addr);

    local_map.test_insert_pair(local_addr, local_params, peer_map, peer_addr, peer_params)
}

/// Returns the [`PathSecretMap`] registered for the given address, or `None`
/// if no sim endpoint is registered at that address.
///
/// This is useful for tests that need to pre-insert path-secret entries with
/// custom [`dc::ApplicationParams`] before a stream is established.
///
/// # Example
///
/// ```ignore
/// let server_map = lookup_sim_map(server_addr).expect("server not registered");
/// let client_map = lookup_sim_map(client.data_addr()).expect("client not registered");
/// // local_params → applied to server_map's entry for client_addr (short timeout)
/// // peer_params  → applied to client_map's entry for server_addr (long timeout)
/// client_map.test_insert_pair(client_addr, Some(short_params), &server_map, server_addr, Some(long_params));
/// ```
pub fn lookup_sim_map(addr: SocketAddr) -> Option<PathSecretMap> {
    SIM_MAP_REGISTRY.with(|r| r.borrow().get(&addr).cloned())
}

async fn connect_stream<A>(
    endpoint: &Arc<Endpoint>,
    peer: A,
    acceptor_id: VarInt,
) -> io::Result<Stream>
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
        peer_addr.set_port(SERVER_PORT);
    }

    // Auto-insert path secrets (idempotent: re-uses existing entry if present).
    let path_secret_entry = connect(endpoint, peer_addr);

    let now = endpoint.clock.now();
    if path_secret_entry.is_dead_during_cooldown(now, endpoint.dead_peer_cooldown) {
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "peer is in dead cooldown window",
        ));
    }

    let QueueState::Client(client_state) = path_secret_entry.queue_state() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "path secret entry has server queue state for client connect",
        ));
    };

    let alloc = client_state
        .alloc(&path_secret_entry, endpoint.dead_peer_cooldown)
        .await
        .ok_or_else(|| io::Error::new(io::ErrorKind::ConnectionReset, "peer queue slots closed"))?;

    let frame_tx = endpoint.frame_tx.clone();
    let writer = Writer::new_client(
        frame_tx.clone(),
        path_secret_entry.clone(),
        alloc.dest_queue_id,
        acceptor_id,
        alloc.control,
        endpoint.clock.clone(),
        endpoint.writer_metrics.clone(),
        endpoint.send_credit_pool.clone(),
        crate::credit::Priority::default(),
    );
    let reader = Reader::new_client(
        frame_tx,
        path_secret_entry,
        alloc.dest_queue_id,
        alloc.stream,
        endpoint.clock.clone(),
        endpoint.reader_metrics.clone(),
        endpoint.recv_credit_pool.clone(),
        crate::credit::Priority::default(),
    );

    Ok(Stream::new(reader, writer))
}

// ── Peer ───────────────────────────────────────────────────────────────────

/// High-level sim peer that combines server + client behavior on one endpoint.
///
/// [`Peer::new`] returns the per-group endpoint wrapper that can both register
/// acceptors and initiate outbound connects while sharing one underlying
/// endpoint and path-secret map.
///
/// When no endpoint exists yet for the current Bach group, it is created with
/// a bind hint of [`SERVER_PORT`]. If a [`Client`] or [`Server`] already
/// created the group endpoint first, [`Peer::new`] reuses that existing
/// endpoint and its original bind address.
pub struct Peer {
    endpoint: Arc<Endpoint>,
}

impl Peer {
    /// Returns the sim peer for the current Bach group, creating it if needed.
    pub fn new() -> Self {
        Self::with_config(SimEndpointConfig::default())
    }

    /// Returns the sim peer for the current Bach group with custom config.
    pub fn with_config(config: SimEndpointConfig) -> Self {
        let config = SimEndpointConfig {
            bind_addr: SocketAddr::new(std::net::Ipv4Addr::UNSPECIFIED.into(), SERVER_PORT),
            ..config
        };
        let endpoint = get_or_create_group_endpoint(config);
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
    pub fn register_acceptor_channel(
        &self,
        acceptor_id: VarInt,
        capacity: usize,
    ) -> io::Result<accept_channel::Receiver<Stream>> {
        self.endpoint
            .acceptor_registry
            .register(acceptor_id, capacity.into())
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::AddrInUse, "acceptor ID already registered")
            })
    }

    /// Connect to a peer, returning a bidirectional [`Stream`].
    pub async fn connect<A>(&mut self, peer: A, acceptor_id: VarInt) -> io::Result<Stream>
    where
        A: bach::net::ToSocketAddrs,
    {
        connect_stream(&self.endpoint, peer, acceptor_id).await
    }
}

impl Default for Peer {
    fn default() -> Self {
        Self::new()
    }
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
/// returns an [`mpmc::Receiver<Stream>`] that yields accepted streams.
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
        Self::with_config(SimEndpointConfig::default())
    }

    /// Returns the sim server for the current Bach group with custom config.
    pub fn with_config(config: SimEndpointConfig) -> Self {
        let config = SimEndpointConfig {
            bind_addr: SocketAddr::new(std::net::Ipv4Addr::UNSPECIFIED.into(), SERVER_PORT),
            ..config
        };
        let endpoint = get_or_create_group_endpoint(config);
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
    /// Returns an [`accept_channel::Receiver<Stream>`] that yields accepted streams.
    /// The acceptor is automatically cleaned up when all receivers are dropped.
    pub fn register_acceptor_channel(
        &self,
        acceptor_id: VarInt,
        capacity: usize,
    ) -> io::Result<accept_channel::Receiver<Stream>> {
        self.endpoint
            .acceptor_registry
            .register(acceptor_id, capacity.into())
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::AddrInUse, "acceptor ID already registered")
            })
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
}

impl Client {
    /// Returns the sim client for the current Bach group, creating it if needed.
    ///
    /// Binds sockets to an ephemeral port (`0.0.0.0:0`).  Call this inside the
    /// group task so the socket is associated with the correct simulated machine.
    pub fn new() -> Self {
        Self::with_config(SimEndpointConfig::default())
    }

    /// Returns the sim client for the current Bach group with custom config.
    pub fn with_config(config: SimEndpointConfig) -> Self {
        let endpoint = get_or_create_group_endpoint(config);
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
        connect_stream(&self.endpoint, peer, acceptor_id).await
    }
}

impl Default for Client {
    fn default() -> Self {
        Self::new()
    }
}

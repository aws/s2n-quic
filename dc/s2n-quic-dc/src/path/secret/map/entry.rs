// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0
use super::{
    size_of::SizeOf,
    status::{Dedup, IsRetired},
    Map,
};
use crate::{
    credentials::{self, Credentials},
    endpoint::id::LocalSenderId,
    packet::{secret_control as control, WireVersion},
    path::secret::{
        open, receiver,
        schedule::{self, Initiator},
        seal, sender,
    },
    tracing::*,
};
use s2n_codec::EncoderBuffer;
use s2n_quic_core::{dc, recovery::bandwidth::Bandwidth, time::Timestamp, varint::VarInt};
use std::{
    any::Any,
    net::SocketAddr,
    sync::{
        atomic::{AtomicI64, AtomicU16, AtomicU32, AtomicU64, AtomicU8, Ordering},
        Arc,
    },
    time::Duration,
};

#[cfg(test)]
mod tests;

pub type ApplicationData = Arc<dyn Any + Send + Sync>;

pub const MAX_PEER_DATA_ADDRS: usize = 128;

pub type PeerDataAddrs = tokio::sync::SetOnce<Arc<[s2n_quic_core::inet::SocketAddressV6]>>;

#[derive(Debug, thiserror::Error)]
#[error("{inner}")]
pub struct ApplicationDataError {
    pub msg: &'static str,
    #[source]
    pub inner: Box<dyn std::error::Error + Send + Sync>,
}

#[derive(Debug)]
pub struct Entry {
    creation_time: Timestamp,
    peer: SocketAddr,
    secret: schedule::Secret,
    retired: IsRetired,
    sender: sender::State,
    receiver: receiver::State,
    parameters: dc::ApplicationParams,
    // we store this as a u8 to allow the cleaner to separately "take" accessed for id and addr
    // maps while not having two writes and wasting an extra byte of space.
    accessed: AtomicU8,
    application_data: Option<ApplicationData>,
    last_activity: AtomicU64,
    dead_at: AtomicI64,
    /// The peer's data recv addresses, learned via the post-handshake exchange.
    peer_data_addrs: PeerDataAddrs,
    /// Per-socket-sender load scores encoded as u64 nanoseconds.
    ///
    /// A lower score means the sender is expected to drain its queue sooner and is
    /// therefore preferred for new work.  The value is not a raw clock timestamp but a
    /// composite score:
    ///
    ///   score = max(now, earliest_departure_time)
    ///         + congestion_penalty           (one smoothed RTT when cwnd-limited)
    ///         + queued_bytes / bandwidth     (estimated queue-drain time)
    ///
    /// This means the score units are still nanoseconds but the semantics are *load*, not
    /// *wall-clock time*.  Comparisons between two scores for the same peer at the same
    /// instant are always valid; absolute values have no external meaning.
    sender_load_scores: Box<[AtomicU64]>,
    /// Per-peer queue state — client or server depending on role.
    queue_state: QueueState,
}

/// Per-peer queue slot state, determined by role (derived from credential_id).
#[derive(Debug)]
pub enum QueueState {
    /// Client: allocates local slots and tracks peer's available slots.
    Client(Arc<crate::queue::ClientState>),
    /// Server: owns the page table that the client addresses into.
    Server(Arc<crate::queue::ServerState>),
}

impl SizeOf for Entry {
    fn size(&self) -> usize {
        let Entry {
            creation_time,
            peer,
            secret,
            retired,
            sender,
            receiver,
            parameters,
            accessed,
            application_data,
            last_activity,
            dead_at,
            peer_data_addrs,
            sender_load_scores,
            queue_state: _,
        } = self;
        creation_time.size()
            + peer.size()
            + secret.size()
            + retired.size()
            + sender.size()
            + receiver.size()
            + parameters.size()
            + accessed.size()
            + application_data.size()
            + last_activity.size()
            + dead_at.size()
            + std::mem::size_of::<PeerDataAddrs>()
            + peer_data_addrs.get().map_or(0, |a| {
                a.len() * std::mem::size_of::<s2n_quic_core::inet::SocketAddressV6>()
            })
            + std::mem::size_of::<Box<[AtomicU64]>>()
            + sender_load_scores.len() * std::mem::size_of::<AtomicU64>()
            + std::mem::size_of::<QueueState>()
    }
}

impl SizeOf for Option<ApplicationData> {
    fn size(&self) -> usize {
        std::mem::size_of::<ApplicationData>()
    }
}

impl SizeOf for ApplicationData {
    fn size(&self) -> usize {
        std::mem::size_of_val(self)
    }
}

impl SizeOf for AtomicU8 {}
impl SizeOf for AtomicU16 {}
impl SizeOf for AtomicU32 {}
impl SizeOf for AtomicI64 {}

impl Entry {
    #[inline]
    fn timestamp_to_millis(timestamp: crate::time::precision::Timestamp) -> i64 {
        (timestamp.nanos / 1_000_000).min(i64::MAX as u64) as i64
    }

    #[inline]
    fn duration_to_millis(duration: Duration) -> i64 {
        duration.as_millis().min(i64::MAX as u128) as i64
    }

    pub fn new(
        peer: SocketAddr,
        secret: schedule::Secret,
        sender: sender::State,
        receiver: receiver::State,
        parameters: dc::ApplicationParams,
        creation_time: Timestamp,
        application_data: Option<ApplicationData>,
    ) -> Self {
        Self::new_with_socket_senders(
            peer,
            secret,
            sender,
            receiver,
            parameters,
            creation_time,
            application_data,
            1,
        )
    }

    pub fn new_with_socket_senders(
        peer: SocketAddr,
        secret: schedule::Secret,
        sender: sender::State,
        receiver: receiver::State,
        parameters: dc::ApplicationParams,
        creation_time: Timestamp,
        application_data: Option<ApplicationData>,
        socket_sender_count: usize,
    ) -> Self {
        // clamp max datagram size to a well-known value
        parameters
            .max_datagram_size
            .fetch_min(crate::endpoint::MAX_DATAGRAM_SIZE as _, Ordering::Relaxed);

        // TODO: max_queues should be min(local_preference, remote_preference) once
        // the handshake negotiates it. For now we use the peer's advertised value as-is.
        let max_queues = parameters.max_queues;
        let queue_state = match secret.id().endpoint_type() {
            s2n_quic_core::endpoint::Type::Client => {
                QueueState::Client(Arc::new(crate::queue::ClientState::new(max_queues)))
            }
            s2n_quic_core::endpoint::Type::Server => {
                QueueState::Server(Arc::new(crate::queue::ServerState::new(max_queues)))
            }
        };

        Self {
            creation_time,
            peer,
            secret,
            retired: Default::default(),
            sender,
            receiver,
            parameters,
            accessed: AtomicU8::new(0),
            application_data,
            last_activity: AtomicU64::new(0),
            dead_at: AtomicI64::new(-1),
            peer_data_addrs: PeerDataAddrs::default(),
            sender_load_scores: Self::init_load_scores(socket_sender_count),
            queue_state,
        }
    }

    #[cfg(any(test, feature = "testing"))]
    pub fn builder(peer: SocketAddr) -> TestEntryBuilder<'static> {
        TestEntryBuilder::new(peer)
    }

    pub fn peer(&self) -> &SocketAddr {
        &self.peer
    }

    /// Returns the peer's data recv addresses.
    pub fn peer_data_addrs(&self) -> &PeerDataAddrs {
        &self.peer_data_addrs
    }

    /// Returns true if the peer's data addresses have been learned via the post-handshake exchange.
    #[inline]
    pub fn has_data_addrs(&self) -> bool {
        self.peer_data_addrs.get().is_some()
    }

    /// Set the peer's data addresses, learned from the post-handshake exchange.
    ///
    /// Wildcard IPs in the address list are replaced with the peer's handshake IP,
    /// since the peer bound to `[::]` but is reachable at the address we connected to.
    ///
    /// Returns `false` if validation fails (empty list, wildcard ports, or
    /// loopback addrs from a non-loopback peer).
    pub fn set_peer_data_addrs(&self, addrs: &[SocketAddr]) -> bool {
        use s2n_quic_core::inet::SocketAddress;

        if addrs.is_empty() {
            error!(peer = %self.peer, "peer data addrs list is empty");
            return false;
        }

        let peer_ip = self.peer.ip();
        let peer_is_loopback = peer_ip.is_loopback();
        let mut v6_addrs = Vec::with_capacity(addrs.len());

        for addr in addrs {
            if addr.port() == 0 {
                error!(%addr, peer = %self.peer, "peer data addr has wildcard port");
                return false;
            }

            let ip = match addr.ip() {
                ip if ip.is_unspecified() => peer_ip,
                ip => ip,
            };

            if !peer_is_loopback && ip.is_loopback() {
                error!(
                    %addr, peer = %self.peer,
                    "peer data addr is loopback but handshake addr is not"
                );
                return false;
            }

            let resolved = SocketAddr::new(ip, addr.port());
            v6_addrs.push(SocketAddress::from(resolved).to_ipv6_mapped());
        }

        ::tracing::debug!(peer = %self.peer, addrs = ?v6_addrs, "setting peer data addresses");

        let _ = self.peer_data_addrs.set(v6_addrs.into());
        true
    }

    fn sender_index(&self, sender_idx: LocalSenderId) -> Option<usize> {
        let len = self.sender_load_scores.len();
        if len == 0 {
            None
        } else {
            debug_assert!(sender_idx.as_usize() < len);
            Some(sender_idx.as_usize() % len)
        }
    }

    /// Encode a `Timestamp` as a u64 load-score value (nanoseconds since epoch).
    fn score_as_u64(ts: Timestamp) -> u64 {
        // SAFETY: `Timestamp` values in this crate are monotonic and treated as non-negative.
        let nanos = unsafe { ts.as_duration().as_nanos() };
        nanos.min(u64::MAX as u128) as u64
    }

    fn init_load_scores(socket_sender_count: usize) -> Box<[AtomicU64]> {
        (0..socket_sender_count)
            .map(|_| AtomicU64::new(0))
            .collect::<Vec<_>>()
            .into_boxed_slice()
    }

    /// Compute how long it would take to drain `queued_bytes` at the given `bandwidth`.
    fn queue_drain_delay(queued_bytes: usize, bandwidth: Bandwidth) -> Duration {
        if queued_bytes == 0 {
            return Duration::ZERO;
        }

        let queued_bytes = u64::try_from(queued_bytes).unwrap_or(u64::MAX);
        <u64 as core::ops::Div<Bandwidth>>::div(queued_bytes, bandwidth)
    }

    #[inline]
    pub fn socket_sender_count(&self) -> usize {
        self.sender_load_scores.len()
    }

    /// Return the current load score for the given sender slot.
    ///
    /// A lower value means the sender is estimated to be less loaded.  Returns `0`
    /// (the lowest possible score) when the entry has no sender slots.
    #[inline]
    pub fn sender_load_score(&self, sender_idx: LocalSenderId) -> u64 {
        let Some(sender_idx) = self.sender_index(sender_idx) else {
            return 0;
        };
        self.sender_load_scores[sender_idx].load(Ordering::Acquire)
    }

    /// Update the load score for the given sender slot.
    ///
    /// The caller supplies `base` — a pre-computed starting point that already accounts
    /// for the CCA's earliest-departure time and any congestion penalty:
    ///
    ///   base = max(now, earliest_departure_time) + congestion_penalty
    ///
    /// This method adds the estimated time to drain `queued_bytes` at `bandwidth` and
    /// stores the result atomically so the pick-two load balancer can read it lock-free.
    pub fn update_sender_load_score(
        &self,
        sender_idx: LocalSenderId,
        base: Timestamp,
        queued_bytes: usize,
        bandwidth: Bandwidth,
    ) -> Timestamp {
        let Some(sender_idx) = self.sender_index(sender_idx) else {
            return base;
        };
        let delay = Self::queue_drain_delay(queued_bytes, bandwidth);
        let score = base + delay;
        let score_value = Self::score_as_u64(score);
        self.sender_load_scores[sender_idx].store(score_value, Ordering::Release);
        score
    }

    pub fn idle_timeout(&self) -> Duration {
        self.parameters
            .max_idle_timeout
            .map_or(Duration::from_secs(60), |v| {
                Duration::from_millis(v.get() as u64)
            })
    }

    pub fn id(&self) -> &credentials::Id {
        self.secret.id()
    }

    pub fn secret(&self) -> &schedule::Secret {
        &self.secret
    }

    pub fn queue_state(&self) -> &QueueState {
        &self.queue_state
    }

    pub fn set_accessed_id(&self) {
        self.accessed.fetch_or(0b10, Ordering::Relaxed);
    }

    pub fn set_accessed_addr(&self) {
        self.accessed.fetch_or(0b01, Ordering::Relaxed);
    }

    pub fn take_accessed_id(&self) -> bool {
        self.accessed.fetch_and(!0b10, Ordering::Relaxed) & 0b10 != 0
    }

    pub fn take_accessed_addr(&self) -> bool {
        self.accessed.fetch_and(!0b01, Ordering::Relaxed) & 0b01 != 0
    }

    pub fn retire(&self, at_epoch: u64) {
        self.retired.retire(at_epoch);
    }

    pub fn retired_at(&self) -> Option<u64> {
        self.retired.retired_at()
    }

    pub fn touch_activity(&self, now: crate::time::precision::Timestamp) {
        self.last_activity.store(now.nanos, Ordering::Release);
    }

    pub fn last_activity(&self) -> crate::time::precision::Timestamp {
        let nanos = self.last_activity.load(Ordering::Acquire);
        crate::time::precision::Timestamp { nanos }
    }

    /// Marks the entry as dead at `now` unless it was already marked dead within `cooldown`.
    ///
    /// Returns `true` when the mark was updated and downstream dead-peer fanout work should run.
    #[inline]
    pub fn mark_dead_if_cooldown_elapsed(
        &self,
        now: crate::time::precision::Timestamp,
        cooldown: Duration,
    ) -> bool {
        let now_ms = Self::timestamp_to_millis(now);
        let cooldown_ms = Self::duration_to_millis(cooldown);
        self.dead_at
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |previous| {
                if previous >= 0 && now_ms.saturating_sub(previous) < cooldown_ms {
                    None
                } else {
                    Some(now_ms)
                }
            })
            .is_ok()
    }

    #[inline]
    pub fn is_dead_during_cooldown(
        &self,
        now: crate::time::precision::Timestamp,
        cooldown: Duration,
    ) -> bool {
        let now_ms = Self::timestamp_to_millis(now);
        let cooldown_ms = Self::duration_to_millis(cooldown);
        let dead_at = self.dead_at.load(Ordering::Acquire);
        dead_at >= 0 && now_ms.saturating_sub(dead_at) < cooldown_ms
    }

    pub fn is_idle_expired(&self, now: crate::time::precision::Timestamp) -> bool {
        let last = self.last_activity();
        let elapsed = now.nanos_since(last);
        let timeout = self.idle_timeout();
        elapsed >= timeout.as_nanos() as u64
    }

    pub fn uni_sealer(&self) -> (seal::Once, Credentials) {
        let key_id = self.sender.next_key_id();
        let credentials = Credentials {
            id: self.secret.peer_id(),
            key_id,
        };
        let sealer = self.secret.application_sealer(key_id);
        let sealer = seal::Once::new(sealer);

        (sealer, credentials)
    }

    pub fn reusable_sealer(&self) -> (crate::crypto::awslc::seal::Application, Credentials) {
        let key_id = self.sender.next_key_id();
        let credentials = Credentials {
            id: self.secret.peer_id(),
            key_id,
        };
        let sealer = self.secret.application_sealer(key_id);

        (sealer, credentials)
    }

    pub fn uni_opener(
        self: Arc<Self>,
        map: Map,
        credentials: &Credentials,
        queue_id: Option<VarInt>,
    ) -> open::Once {
        let key_id = credentials.key_id;
        let opener = self.secret.application_opener(key_id);
        let dedup = Dedup::new(self, key_id, queue_id, map);
        open::Once::new(opener, dedup)
    }

    pub fn bidi_local(&self) -> Bidirectional {
        let key_id = self.sender.next_key_id();
        let initiator = Initiator::Local;

        let application = ApplicationPair::new(
            &self.secret,
            key_id,
            initiator,
            // we don't need to dedup locally-initiated openers
            Dedup::disabled(),
        );

        let control = Some(ControlPair::new(&self.secret, key_id, initiator));

        Bidirectional {
            credentials: Credentials {
                id: self.secret.peer_id(),
                key_id,
            },
            application,
            control,
        }
    }

    pub fn bidi_remote(
        self: &Arc<Self>,
        map: Map,
        credentials: &Credentials,
        queue_id: Option<VarInt>,
    ) -> Bidirectional {
        let key_id = credentials.key_id;
        let initiator = Initiator::Remote;

        let application = ApplicationPair::new(
            &self.secret,
            key_id,
            initiator,
            // Remote application keys need to be de-duplicated
            Dedup::new(self.clone(), key_id, queue_id, map),
        );

        let control = Some(ControlPair::new(&self.secret, key_id, initiator));

        Bidirectional {
            credentials: *credentials,
            application,
            control,
        }
    }

    pub fn parameters(&self) -> dc::ApplicationParams {
        self.parameters.clone()
    }

    pub fn max_datagram_size(&self) -> u16 {
        self.parameters.max_datagram_size.load(Ordering::Relaxed)
    }

    pub fn update_max_datagram_size(&self, mtu: u16) {
        self.parameters
            .max_datagram_size
            .store(mtu, Ordering::Relaxed);
    }

    pub fn creation_time(&self) -> Timestamp {
        self.creation_time
    }

    pub fn receiver(&self) -> &receiver::State {
        &self.receiver
    }

    pub fn sender(&self) -> &sender::State {
        &self.sender
    }

    pub fn control_opener(&self) -> crate::crypto::awslc::open::control::Secret {
        self.sender.control_secret(&self.secret)
    }

    pub fn control_sealer(&self) -> crate::crypto::awslc::seal::control::Secret {
        self.secret.control_sealer()
    }

    pub fn application_data(&self) -> &Option<ApplicationData> {
        &self.application_data
    }

    #[cfg(test)]
    pub fn reset_sender_counter(&self) {
        self.sender.reset_counter();
    }
}

#[cfg(any(test, feature = "testing"))]
pub struct TestEntryBuilder<'a> {
    local: SocketAddr,
    peer: SocketAddr,
    endpoint_type: s2n_quic_core::endpoint::Type,
    socket_sender_count: usize,
    generation: u64,
    params: Option<dc::ApplicationParams>,
    signer: Option<&'a super::stateless_reset::Signer>,
}

#[cfg(any(test, feature = "testing"))]
impl<'a> TestEntryBuilder<'a> {
    fn new(peer: SocketAddr) -> Self {
        Self {
            local: peer,
            peer,
            endpoint_type: s2n_quic_core::endpoint::Type::Client,
            socket_sender_count: 0,
            generation: 0,
            params: None,
            signer: None,
        }
    }

    pub fn local(mut self, addr: SocketAddr) -> Self {
        self.local = addr;
        self
    }

    pub fn endpoint_type(mut self, endpoint_type: s2n_quic_core::endpoint::Type) -> Self {
        self.endpoint_type = endpoint_type;
        self
    }

    pub fn socket_sender_count(mut self, count: usize) -> Self {
        self.socket_sender_count = count;
        self
    }

    pub fn generation(mut self, generation: u64) -> Self {
        self.generation = generation;
        self
    }

    pub fn params(mut self, params: dc::ApplicationParams) -> Self {
        self.params = Some(params);
        self
    }

    fn secret_bytes(&self) -> [u8; 32] {
        use s2n_quic_core::endpoint::Type;

        let (client_addr, server_addr) = match self.endpoint_type {
            Type::Client => (self.local, self.peer),
            Type::Server => (self.peer, self.local),
        };
        Self::deterministic_secret(client_addr, server_addr, self.generation)
    }

    pub fn signer(mut self, signer: &'a super::stateless_reset::Signer) -> Self {
        self.signer = Some(signer);
        self
    }

    pub fn build(&self) -> Arc<Entry> {
        let secret_bytes = self.secret_bytes();
        let default_signer;
        let signer = match self.signer {
            Some(s) => s,
            None => {
                default_signer = super::stateless_reset::Signer::new(&secret_bytes);
                &default_signer
            }
        };

        let params = self
            .params
            .clone()
            .unwrap_or(dc::testing::TEST_APPLICATION_PARAMS);

        let secret = schedule::Secret::new(
            schedule::Ciphersuite::AES_GCM_128_SHA256,
            dc::SUPPORTED_VERSIONS[0],
            self.endpoint_type,
            &secret_bytes,
        );

        let stateless_reset = signer.sign(secret.id());

        Arc::new(Entry::new_with_socket_senders(
            self.peer,
            secret,
            sender::State::new(stateless_reset),
            receiver::State::new(),
            params,
            crate::time::DefaultClock::default().now().into(),
            None,
            self.socket_sender_count,
        ))
    }

    fn deterministic_secret(
        client_addr: SocketAddr,
        server_addr: SocketAddr,
        generation: u64,
    ) -> [u8; 32] {
        fn mix(state: &mut u64, bytes: &[u8]) {
            for &byte in bytes {
                *state ^= byte as u64;
                *state = state.wrapping_mul(0x1000_0000_01B3);
            }
        }

        fn mix_addr(state: &mut u64, addr: SocketAddr) {
            match addr {
                SocketAddr::V4(addr) => {
                    mix(state, &[4]);
                    mix(state, &addr.ip().octets());
                    mix(state, &addr.port().to_be_bytes());
                }
                SocketAddr::V6(addr) => {
                    mix(state, &[6]);
                    mix(state, &addr.ip().octets());
                    mix(state, &addr.port().to_be_bytes());
                    mix(state, &addr.flowinfo().to_be_bytes());
                    mix(state, &addr.scope_id().to_be_bytes());
                }
            }
        }

        fn splitmix64(state: &mut u64) -> u64 {
            *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = *state;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^ (z >> 31)
        }

        let mut state: u64 = 0xCBF2_9CE4_8422_2325; // FNV offset basis

        mix_addr(&mut state, client_addr);
        mix_addr(&mut state, server_addr);
        mix(&mut state, &generation.to_be_bytes());

        let mut output = [0u8; 32];
        for chunk in output.chunks_exact_mut(8) {
            chunk.copy_from_slice(&splitmix64(&mut state).to_le_bytes());
        }
        output
    }
}

impl receiver::Error {
    pub(super) fn to_packet<'buffer>(
        self,
        entry: &Entry,
        credentials: &Credentials,
        queue_id: Option<VarInt>,
        buffer: &'buffer mut [u8; control::MAX_PACKET_SIZE],
    ) -> &'buffer [u8] {
        debug_assert_eq!(entry.secret.id(), &credentials.id);
        let encoder = EncoderBuffer::new(&mut buffer[..]);
        let length = match self {
            receiver::Error::AlreadyExists => control::ReplayDetected {
                wire_version: WireVersion::ZERO,
                credential_id: credentials.id,
                rejected_key_id: credentials.key_id,
                sender_id: queue_id,
            }
            .encode(encoder, &entry.control_sealer()),
            receiver::Error::Unknown => control::StaleKey {
                wire_version: WireVersion::ZERO,
                credential_id: credentials.id,
                min_key_id: entry.receiver.minimum_unseen_key_id(),
                sender_id: queue_id,
            }
            .encode(encoder, &entry.control_sealer()),
        };
        &buffer[..length]
    }
}

pub struct Bidirectional {
    pub credentials: Credentials,
    pub application: ApplicationPair,
    pub control: Option<ControlPair>,
}

pub struct ApplicationPair {
    pub sealer: seal::Application,
    pub opener: open::Application,
}

impl ApplicationPair {
    pub fn new(
        secret: &schedule::Secret,
        key_id: VarInt,
        initiator: Initiator,
        dedup: Dedup,
    ) -> Self {
        let (sealer, sealer_ku, opener, opener_ku) = secret.application_pair(key_id, initiator);

        let sealer = seal::Application::new(sealer, sealer_ku);

        let opener = open::Application::new(opener, opener_ku, dedup);

        Self { sealer, opener }
    }
}

pub struct ControlPair {
    pub sealer: seal::control::Stream,
    pub opener: open::control::Stream,
}

impl ControlPair {
    fn new(secret: &schedule::Secret, key_id: VarInt, initiator: Initiator) -> Self {
        let (sealer, opener) = secret.control_pair(key_id, initiator);

        Self { sealer, opener }
    }
}

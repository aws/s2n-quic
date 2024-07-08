// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{
    receiver,
    schedule::{self, Initiator},
    sender, stateless_reset, Opener, Sealer,
};
use crate::{
    credentials::{Credentials, Id},
    crypto,
    packet::{secret_control as control, Packet},
};
use rand::Rng as _;
use s2n_codec::EncoderBuffer;
use s2n_quic_core::{
    dc::{self, ApplicationParams, DatagramInfo},
    ensure,
    event::api::EndpointType,
};
use std::{
    fmt,
    net::{Ipv4Addr, SocketAddr},
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use zeroize::Zeroizing;

const TLS_EXPORTER_LABEL: &str = "EXPERIMENTAL EXPORTER s2n-quic-dc";
const TLS_EXPORTER_CONTEXT: &str = "";
const TLS_EXPORTER_LENGTH: usize = schedule::EXPORT_SECRET_LEN;

// FIXME: Most of this comment is not true today, we're expecting to implement the details
// contained here. This is presented as a roadmap.
/// This map caches path secrets derived from handshakes.
///
/// The cache is configurable on two axes:
///
/// * Maximum size (in megabytes)
/// * Maximum per-peer/secret derivation per-second rate (in derived secrets, e.g., accepted/opened streams)
///
/// Each entry in the cache will take around 550 bytes plus 15 bits per derived secret at the
/// maximum rate (corresponding to no false positives in replay prevention for 15 seconds).
#[derive(Clone)]
pub struct Map {
    pub(super) state: Arc<State>,
}

// # Managing memory consumption
//
// For regular rotation with live peers, we retain at most two secrets: one derived from the most
// recent locally initiated handshake and the most recent remote initiated handshake (from our
// perspective). We guarantee that at most one handshake is ongoing for a given peer pair at a
// time, so both sides will have at least one mutually trusted entry after the handshake. If a peer
// is only acting as a client or only as a server, then one of the peer maps will always be empty.
//
// Previous entries can safely be removed after a grace period (EVICTION_TIME). EVICTION_TIME
// is only needed because a stream/datagram might be opening/sent concurrently with the new
// handshake (e.g., during regular rotation), and we don't want that to fail spuriously.
//
// We also need to manage secrets for no longer existing peers. These are peers where typically the
// underlying host has gone away and/or the address for it has changed. At 95% occupancy for the
// maximum size allowed, we will remove least recently used secrets (1% of these per minute). Usage
// is defined by access to the entry in the map. Unfortunately we lack any good way to authenticate
// a peer as *not* having credentials, especially after the peer is gone. It's possible that in the
// future information could also come from the TLS provider.
pub(super) struct State {
    // This is in number of entries.
    max_capacity: usize,

    // peers is the most recent entry originating from a locally *or* remote initiated handshake.
    //
    // Handshakes use s2n-quic and the SocketAddr is the address of the handshake socket. Since
    // s2n-quic only has Client or Server endpoints, a given SocketAddr can only be used for
    // exactly one of a locally initiated handshake or a remote initiated handshake. As a result we
    // can use a single map to store both kinds and treat them identically.
    //
    // In the future it's likely we'll want to build bidirectional support in which case splitting
    // this into two maps (per the discussion in "Managing memory consumption" above) will be
    // needed.
    pub(super) peers: flurry::HashMap<SocketAddr, Arc<Entry>>,

    // This is used for deduplicating outgoing handshakes. We manage this here as it's a
    // property required for correctness (see comment on the struct).
    //
    // FIXME: make use of this.
    #[allow(unused)]
    pub(super) ongoing_handshakes: flurry::HashMap<SocketAddr, ()>,

    // Stores the set of SocketAddr for which we received a UnknownPathSecret packet.
    // When handshake_with is called we will allow a new handshake if this contains a socket, this
    // is a temporary solution until we implement proper background handshaking.
    pub(super) requested_handshakes: flurry::HashSet<SocketAddr>,

    // All known entries.
    pub(super) ids: flurry::HashMap<Id, Arc<Entry>>,

    pub(super) signer: stateless_reset::Signer,

    // This socket is used *only* for sending secret control packets.
    // FIXME: This will get replaced with sending on a handshake socket associated with the map.
    pub(super) control_socket: std::net::UdpSocket,

    pub(super) receiver_shared: Arc<receiver::Shared>,

    handled_control_packets: AtomicUsize,

    cleaner: Cleaner,
}

struct Cleaner {
    should_stop: AtomicBool,
    thread: Mutex<Option<std::thread::JoinHandle<()>>>,
    epoch: AtomicU64,
}

impl Drop for Cleaner {
    fn drop(&mut self) {
        self.stop();
    }
}

impl Cleaner {
    fn new() -> Cleaner {
        Cleaner {
            should_stop: AtomicBool::new(false),
            thread: Mutex::new(None),
            epoch: AtomicU64::new(1),
        }
    }

    fn stop(&self) {
        self.should_stop.store(true, Ordering::Relaxed);
        if let Some(thread) =
            std::mem::take(&mut *self.thread.lock().unwrap_or_else(|e| e.into_inner()))
        {
            thread.thread().unpark();

            // If this isn't getting dropped on the cleaner thread,
            // then wait for the background thread to finish exiting.
            if std::thread::current().id() != thread.thread().id() {
                // We expect this to terminate very quickly.
                thread.join().unwrap();
            }
        }
    }

    fn spawn_thread(&self, state: Arc<State>) {
        let state = Arc::downgrade(&state);
        let handle = std::thread::spawn(move || loop {
            let Some(state) = state.upgrade() else {
                break;
            };
            if state.cleaner.should_stop.load(Ordering::Relaxed) {
                break;
            }
            state.cleaner.clean(&state, EVICTION_CYCLES);
            let pause = rand::thread_rng().gen_range(5..60);
            drop(state);
            std::thread::park_timeout(Duration::from_secs(pause));
        });
        *self.thread.lock().unwrap() = Some(handle);
    }

    /// Clean up dead items.
    // In local benchmarking iterating a 500,000 element flurry::HashMap takes about
    // 60-70ms. With contention, etc. it might be longer, but this is not an overly long
    // time given that we expect to run this in a background thread once a minute.
    //
    // This is exposed as a method primarily for tests to directly invoke.
    fn clean(&self, state: &State, eviction_cycles: u64) {
        let current_epoch = self.epoch.fetch_add(1, Ordering::Relaxed);

        // FIXME: Rather than just tracking one minimum, we might want to try to do some counting
        // as we iterate to have a higher likelihood of identifying 1% of peers falling into the
        // epoch we pick. Exactly how to do that without collecting a ~full distribution by epoch
        // is not clear though and we'd prefer to avoid allocating extra memory here.
        //
        // As-is we're just hoping that once-per-minute oldest-epoch identification and removal is
        // enough that we keep the capacity below 100%. We could have a mode that starts just
        // randomly evicting entries if we hit 100% but even this feels like an annoying modality
        // to deal with.
        let mut minimum = u64::MAX;
        {
            let guard = state.ids.guard();
            for (id, entry) in state.ids.iter(&guard) {
                let retired_at = entry.retired.0.load(Ordering::Relaxed);
                if retired_at == 0 {
                    // Find the minimum non-retired epoch currently in the set.
                    minimum = std::cmp::min(entry.used_at.load(Ordering::Relaxed), minimum);

                    // Not retired.
                    continue;
                }
                // Avoid panics on overflow (which should never happen...)
                if current_epoch.saturating_sub(retired_at) >= eviction_cycles {
                    state.ids.remove(id, &guard);
                }
            }
        }

        if state.ids.len() <= (state.max_capacity * 95 / 100) {
            return;
        }

        let mut to_remove = std::cmp::max(state.ids.len() / 100, 1);
        let guard = state.ids.guard();
        for (id, entry) in state.ids.iter(&guard) {
            if to_remove > 0 {
                // Only remove with the minimum epoch. This hopefully means that we will remove
                // fairly stale entries.
                if entry.used_at.load(Ordering::Relaxed) == minimum {
                    state.ids.remove(id, &guard);
                    to_remove -= 1;
                }
            } else {
                break;
            }
        }
    }

    fn epoch(&self) -> u64 {
        self.epoch.load(Ordering::Relaxed)
    }
}

const EVICTION_CYCLES: u64 = if cfg!(test) { 0 } else { 10 };

impl Map {
    pub fn new(signer: stateless_reset::Signer) -> Self {
        // FIXME: Avoid unwrap and the whole socket.
        //
        // We only ever send on this socket - but we really should be sending on the same
        // socket as used by an associated s2n-quic handshake runtime, and receiving control packets
        // from that socket as well. Not exactly clear on how to achieve that yet though (both
        // ownership wise since the map doesn't have direct access to handshakes and in terms
        // of implementation).
        let control_socket = std::net::UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).unwrap();
        control_socket.set_nonblocking(true).unwrap();
        let state = State {
            // This is around 500MB with current entry size.
            max_capacity: 500_000,
            peers: Default::default(),
            ongoing_handshakes: Default::default(),
            requested_handshakes: Default::default(),
            ids: Default::default(),
            cleaner: Cleaner::new(),
            signer,

            receiver_shared: receiver::Shared::new(),

            handled_control_packets: AtomicUsize::new(0),
            control_socket,
        };

        let state = Arc::new(state);

        state.cleaner.spawn_thread(state.clone());

        Self { state }
    }

    pub fn drop_state(&self) {
        self.state.peers.pin().clear();
        self.state.ids.pin().clear();
    }

    pub fn contains(&self, peer: SocketAddr) -> bool {
        self.state.peers.pin().contains_key(&peer)
            && !self.state.requested_handshakes.pin().contains(&peer)
    }

    pub fn sealer(&self, peer: SocketAddr) -> Option<(Sealer, ApplicationParams)> {
        let peers_guard = self.state.peers.guard();
        let state = self.state.peers.get(&peer, &peers_guard)?;
        state.mark_live(self.state.cleaner.epoch());

        let sealer = state.uni_sealer();
        Some((sealer, state.parameters))
    }

    pub fn opener(&self, credentials: &Credentials, control_out: &mut Vec<u8>) -> Option<Opener> {
        let state = self.pre_authentication(credentials, control_out)?;
        let opener = state.uni_opener(self.clone(), credentials);
        Some(opener)
    }

    pub fn pair_for_peer(&self, peer: SocketAddr) -> Option<(Sealer, Opener, ApplicationParams)> {
        let peers_guard = self.state.peers.guard();
        let state = self.state.peers.get(&peer, &peers_guard)?;
        state.mark_live(self.state.cleaner.epoch());

        let (sealer, opener) = state.bidi_local();

        Some((sealer, opener, state.parameters))
    }

    pub fn pair_for_credentials(
        &self,
        credentials: &Credentials,
        control_out: &mut Vec<u8>,
    ) -> Option<(Sealer, Opener, ApplicationParams)> {
        let state = self.pre_authentication(credentials, control_out)?;

        let params = state.parameters;
        let (sealer, opener) = state.bidi_remote(self.clone(), credentials);

        Some((sealer, opener, params))
    }

    /// This can be called from anywhere to ask the map to handle a packet.
    ///
    /// For secret control packets, this will process those.
    /// For other packets, the map may collect metrics but will otherwise drop the packets.
    pub fn handle_unexpected_packet(&self, packet: &Packet) {
        match packet {
            Packet::Stream(_) => {
                // no action for now. FIXME: Add metrics.
            }
            Packet::Datagram(_) => {
                // no action for now. FIXME: Add metrics.
            }
            Packet::Control(_) => {
                // no action for now. FIXME: Add metrics.
            }
            Packet::StaleKey(packet) => self.handle_control_packet(&(*packet).into()),
            Packet::ReplayDetected(packet) => self.handle_control_packet(&(*packet).into()),
            Packet::UnknownPathSecret(packet) => self.handle_control_packet(&(*packet).into()),
        }
    }

    pub fn handle_unknown_secret_packet(&self, packet: &control::unknown_path_secret::Packet) {
        let ids_guard = self.state.ids.guard();
        let Some(state) = self.state.ids.get(packet.credential_id(), &ids_guard) else {
            return;
        };
        // Do not mark as live, this is lightly authenticated.

        // ensure the packet is authentic
        if packet.authenticate(&state.sender.stateless_reset).is_none() {
            return;
        }

        self.state
            .handled_control_packets
            .fetch_add(1, Ordering::Relaxed);

        // FIXME: More actively schedule a new handshake.
        // See comment on requested_handshakes for details.
        self.state.requested_handshakes.pin().insert(state.peer);
    }

    pub fn handle_control_packet(&self, packet: &control::Packet) {
        if let control::Packet::UnknownPathSecret(ref packet) = &packet {
            return self.handle_unknown_secret_packet(packet);
        }

        let ids_guard = self.state.ids.guard();
        let Some(state) = self.state.ids.get(packet.credential_id(), &ids_guard) else {
            // If we get a control packet we don't have a registered path secret for, ignore the
            // packet.
            return;
        };

        let key = state.sender.control_secret(&state.secret);

        match packet {
            control::Packet::StaleKey(packet) => {
                let Some(packet) = packet.authenticate(key) else {
                    return;
                };
                state.mark_live(self.state.cleaner.epoch());
                state.sender.update_for_stale_key(packet.min_key_id);
                self.state
                    .handled_control_packets
                    .fetch_add(1, Ordering::Relaxed);
            }
            control::Packet::ReplayDetected(packet) => {
                let Some(_packet) = packet.authenticate(key) else {
                    return;
                };
                self.state
                    .handled_control_packets
                    .fetch_add(1, Ordering::Relaxed);

                // If we see replay then we're going to assume that we should re-handshake in the
                // background with this peer. Currently we can't handshake in the background (only
                // in the foreground on next handshake_with).
                //
                // Note that there's no good way for us to prevent an attacker causing us to hit
                // this code: they can always trivially replay a packet we send. At most we could
                // de-duplicate *receiving* so there's one handshake per sent packet at most, but
                // that's not particularly useful: we expect to send a lot of new packets that
                // could be harvested.
                //
                // Handshaking will be rate limited per destination peer (and at least
                // de-duplicated).
                self.state.requested_handshakes.pin().insert(state.peer);
            }
            control::Packet::UnknownPathSecret(_) => unreachable!(),
        }
    }

    fn pre_authentication(
        &self,
        identity: &Credentials,
        control_out: &mut Vec<u8>,
    ) -> Option<Arc<Entry>> {
        let ids_guard = self.state.ids.guard();
        let Some(state) = self.state.ids.get(&identity.id, &ids_guard) else {
            let packet = control::UnknownPathSecret {
                credential_id: identity.id,
            };
            control_out.resize(control::UnknownPathSecret::PACKET_SIZE, 0);
            let stateless_reset = self.state.signer.sign(&identity.id);
            let encoder = EncoderBuffer::new(control_out);
            packet.encode(encoder, &stateless_reset);
            return None;
        };
        state.mark_live(self.state.cleaner.epoch());

        match state.receiver.pre_authentication(identity) {
            Ok(()) => {}
            Err(e) => {
                self.send_control(state, identity, e);
                control_out.resize(control::UnknownPathSecret::PACKET_SIZE, 0);

                return None;
            }
        }

        Some(state.clone())
    }

    pub(super) fn insert(&self, entry: Arc<Entry>) {
        // On insert clear our interest in a handshake.
        self.state.requested_handshakes.pin().remove(&entry.peer);
        entry.mark_live(self.state.cleaner.epoch());
        let id = *entry.secret.id();
        let peer = entry.peer;
        let ids_guard = self.state.ids.guard();
        if self
            .state
            .ids
            .insert(id, entry.clone(), &ids_guard)
            .is_some()
        {
            // FIXME: Make insertion fallible and fail handshakes instead?
            panic!("inserting a path secret ID twice");
        }

        let peers_guard = self.state.peers.guard();
        if let Some(prev) = self.state.peers.insert(peer, entry, &peers_guard) {
            // This shouldn't happen due to the panic above, but just in case something went wrong
            // with the secret map we double check here.
            // FIXME: Make insertion fallible and fail handshakes instead?
            assert_ne!(*prev.secret.id(), id, "duplicate path secret id");

            prev.retire(self.state.cleaner.epoch());
        }
    }

    pub(super) fn signer(&self) -> &stateless_reset::Signer {
        &self.state.signer
    }

    #[doc(hidden)]
    #[cfg(any(test, feature = "testing"))]
    pub fn for_test_with_peers(
        peers: Vec<(schedule::Ciphersuite, dc::Version, SocketAddr)>,
    ) -> (Self, Vec<Id>) {
        let provider = Self::new(Default::default());
        let mut secret = [0; 32];
        aws_lc_rs::rand::fill(&mut secret).unwrap();
        let mut stateless_reset = [0; 16];
        aws_lc_rs::rand::fill(&mut stateless_reset).unwrap();

        let receiver_shared = receiver::Shared::new();

        let mut ids = Vec::with_capacity(peers.len());
        for (idx, (ciphersuite, version, peer)) in peers.into_iter().enumerate() {
            secret[..8].copy_from_slice(&(idx as u64).to_be_bytes()[..]);
            stateless_reset[..8].copy_from_slice(&(idx as u64).to_be_bytes()[..]);
            let secret = schedule::Secret::new(
                ciphersuite,
                version,
                s2n_quic_core::endpoint::Type::Client,
                &secret,
            );
            ids.push(*secret.id());
            let sender = sender::State::new(stateless_reset);
            let entry = Entry::new(
                peer,
                secret,
                sender,
                receiver_shared.clone().new_receiver(),
                dc::testing::TEST_APPLICATION_PARAMS,
            );
            let entry = Arc::new(entry);
            provider.insert(entry);
        }

        (provider, ids)
    }

    #[doc(hidden)]
    #[cfg(any(test, feature = "testing"))]
    pub fn test_insert(&self, peer: SocketAddr) {
        let mut secret = [0; 32];
        aws_lc_rs::rand::fill(&mut secret).unwrap();
        let secret = schedule::Secret::new(
            schedule::Ciphersuite::AES_GCM_128_SHA256,
            dc::SUPPORTED_VERSIONS[0],
            s2n_quic_core::endpoint::Type::Client,
            &secret,
        );
        let sender = sender::State::new([0; 16]);
        let receiver = self.state.receiver_shared.clone().new_receiver();
        let entry = Entry::new(
            peer,
            secret,
            sender,
            receiver,
            dc::testing::TEST_APPLICATION_PARAMS,
        );
        self.insert(Arc::new(entry));
    }

    fn send_control(&self, entry: &Entry, credentials: &Credentials, error: receiver::Error) {
        let mut buffer = [0; control::MAX_PACKET_SIZE];
        let buffer = error.to_packet(entry, credentials, &mut buffer);
        let dst = entry.peer;
        self.send_control_packet(dst, buffer);
    }

    pub(crate) fn send_control_packet(&self, dst: SocketAddr, buffer: &[u8]) {
        match self.state.control_socket.send_to(buffer, dst) {
            Ok(_) => {
                // all done
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                // ignore would block -- we're not going to queue up control packet messages.
            }
            Err(e) => {
                tracing::warn!("Failed to send control packet to {:?}: {:?}", dst, e);
            }
        }
    }

    #[doc(hidden)]
    #[cfg(any(test, feature = "testing"))]
    pub fn handled_control_packets(&self) -> usize {
        self.state.handled_control_packets.load(Ordering::Relaxed)
    }
}

impl receiver::Error {
    pub(super) fn to_packet<'buffer>(
        self,
        entry: &Entry,
        credentials: &Credentials,
        buffer: &'buffer mut [u8; control::MAX_PACKET_SIZE],
    ) -> &'buffer [u8] {
        debug_assert_eq!(entry.secret.id(), &credentials.id);
        let encoder = EncoderBuffer::new(&mut buffer[..]);
        let length = match self {
            receiver::Error::AlreadyExists => control::ReplayDetected {
                credential_id: credentials.id,
                rejected_key_id: credentials.key_id,
            }
            .encode(encoder, &entry.secret.control_sealer()),
            receiver::Error::Unknown => control::StaleKey {
                credential_id: credentials.id,
                min_key_id: entry.receiver.minimum_unseen_key_id(),
            }
            .encode(encoder, &entry.secret.control_sealer()),
        };
        &buffer[..length]
    }
}

#[derive(Debug)]
pub(super) struct Entry {
    peer: SocketAddr,
    secret: schedule::Secret,
    retired: IsRetired,
    // Last time the entry was pulled out of the State map.
    // This is not necessarily the last time the entry was used but it's close enough for our
    // purposes: if the entry is not being pulled out of the State map, it's hopefully not going to
    // start getting pulled out shortly. This is used for the LRU mechanism, see the Cleaner impl
    // for details.
    used_at: AtomicU64,
    sender: sender::State,
    receiver: receiver::State,
    parameters: ApplicationParams,
}

// Retired is 0 if not yet retired. Otherwise it stores the background cleaner epoch at which it
// retired; that epoch increments roughly once per minute.
#[derive(Default)]
struct IsRetired(AtomicU64);

impl fmt::Debug for IsRetired {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("IsRetired").field(&self.retired()).finish()
    }
}

impl IsRetired {
    fn retired(&self) -> bool {
        self.0.load(Ordering::Relaxed) != 0
    }
}

impl Entry {
    pub fn new(
        peer: SocketAddr,
        secret: schedule::Secret,
        sender: sender::State,
        receiver: receiver::State,
        parameters: ApplicationParams,
    ) -> Self {
        Self {
            peer,
            secret,
            retired: Default::default(),
            used_at: AtomicU64::new(0),
            sender,
            receiver,
            parameters,
        }
    }

    fn retire(&self, at_epoch: u64) {
        self.retired.0.store(at_epoch, Ordering::Relaxed);
    }

    fn mark_live(&self, at_epoch: u64) {
        self.used_at.store(at_epoch, Ordering::Relaxed);
    }

    fn uni_sealer(&self) -> Sealer {
        let key_id = self.sender.next_key_id();
        let sealer = self.secret.application_sealer(key_id);

        Sealer { sealer }
    }

    fn uni_opener(self: Arc<Self>, map: Map, credentials: &Credentials) -> Opener {
        let opener = self.secret.application_opener(credentials.key_id);

        let dedup = Dedup::new(self, map);

        Opener { opener, dedup }
    }

    fn bidi_local(&self) -> (Sealer, Opener) {
        let key_id = self.sender.next_key_id();
        let (sealer, opener) = self.secret.application_pair(key_id, Initiator::Local);
        let sealer = Sealer { sealer };

        // we don't need to dedup locally-initiated openers
        let dedup = Dedup::disabled();

        let opener = Opener { opener, dedup };

        (sealer, opener)
    }

    fn bidi_remote(self: Arc<Self>, map: Map, credentials: &Credentials) -> (Sealer, Opener) {
        let (sealer, opener) = self
            .secret
            .application_pair(credentials.key_id, Initiator::Remote);
        let sealer = Sealer { sealer };

        let dedup = Dedup::new(self, map);

        let opener = Opener { opener, dedup };

        (sealer, opener)
    }
}

pub struct Dedup {
    cell: once_cell::sync::OnceCell<crypto::decrypt::Result>,
    init: core::cell::Cell<Option<(Arc<Entry>, Map)>>,
}

/// SAFETY: `init` cell is synchronized by `OnceCell`
unsafe impl Sync for Dedup {}

impl Dedup {
    #[inline]
    fn new(entry: Arc<Entry>, map: Map) -> Self {
        // TODO potentially record a timestamp of when this was created to try and detect long
        // delays of processing the first packet.
        Self {
            cell: Default::default(),
            init: core::cell::Cell::new(Some((entry, map))),
        }
    }

    #[inline]
    fn disabled() -> Self {
        Self {
            cell: once_cell::sync::OnceCell::with_value(Ok(())),
            init: core::cell::Cell::new(None),
        }
    }

    #[inline]
    pub(crate) fn disable(&self) {
        // TODO
    }

    #[inline]
    pub fn check(&self, c: &impl crypto::decrypt::Key) -> crypto::decrypt::Result {
        *self.cell.get_or_init(|| {
            match self.init.take() {
                Some((entry, map)) => {
                    let creds = c.credentials();
                    match entry.receiver.post_authentication(creds) {
                        Ok(()) => Ok(()),
                        Err(receiver::Error::AlreadyExists) => {
                            map.send_control(&entry, creds, receiver::Error::AlreadyExists);
                            Err(crypto::decrypt::Error::ReplayDefinitelyDetected)
                        }
                        Err(receiver::Error::Unknown) => {
                            map.send_control(&entry, creds, receiver::Error::Unknown);
                            Err(crypto::decrypt::Error::ReplayPotentiallyDetected {
                                gap: Some(
                                    (*entry.receiver.minimum_unseen_key_id())
                                        // This should never be negative, but saturate anyway to avoid
                                        // wildly large numbers.
                                        .saturating_sub(*creds.key_id),
                                ),
                            })
                        }
                    }
                }
                None => {
                    // Dedup has been poisoned! TODO log this
                    Err(crypto::decrypt::Error::ReplayPotentiallyDetected { gap: None })
                }
            }
        })
    }
}

impl fmt::Debug for Dedup {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Dedup").field("cell", &self.cell).finish()
    }
}

pub struct HandshakingPath {
    peer: SocketAddr,
    dc_version: dc::Version,
    parameters: ApplicationParams,
    endpoint_type: s2n_quic_core::endpoint::Type,
    secret: Option<schedule::Secret>,
    map: Map,
}

impl HandshakingPath {
    fn new(connection_info: &dc::ConnectionInfo, map: Map) -> Self {
        let endpoint_type = match connection_info.endpoint_type {
            EndpointType::Server { .. } => s2n_quic_core::endpoint::Type::Server,
            EndpointType::Client { .. } => s2n_quic_core::endpoint::Type::Client,
        };

        Self {
            peer: connection_info.remote_address.clone().into(),
            dc_version: connection_info.dc_version,
            parameters: connection_info.application_params,
            endpoint_type,
            secret: None,
            map,
        }
    }
}

impl dc::Endpoint for Map {
    type Path = HandshakingPath;

    fn new_path(&mut self, connection_info: &dc::ConnectionInfo) -> Option<Self::Path> {
        Some(HandshakingPath::new(connection_info, self.clone()))
    }

    fn on_possible_secret_control_packet(
        &mut self,
        // TODO: Maybe we should confirm that the sender IP at least matches the IP for the
        //       corresponding control secret?
        _datagram_info: &DatagramInfo,
        payload: &mut [u8],
    ) -> bool {
        let payload = s2n_codec::DecoderBufferMut::new(payload);
        // TODO: Is 16 always right?
        return match control::Packet::decode(payload, 16) {
            Ok((packet, tail)) => {
                // Probably a bug somewhere? There shouldn't be anything trailing in the buffer
                // after we decode a secret control packet.
                ensure!(tail.is_empty(), false);

                // If we successfully decoded a control packet, pass it into our map to handle.
                self.handle_control_packet(&packet);

                true
            }
            Err(_) => false,
        };
    }
}

impl dc::Path for HandshakingPath {
    fn on_path_secrets_ready(
        &mut self,
        session: &impl s2n_quic_core::crypto::tls::TlsSession,
    ) -> Result<Vec<s2n_quic_core::stateless_reset::Token>, s2n_quic_core::transport::Error> {
        let mut material = Zeroizing::new([0; TLS_EXPORTER_LENGTH]);
        session
            .tls_exporter(
                TLS_EXPORTER_LABEL.as_bytes(),
                TLS_EXPORTER_CONTEXT.as_bytes(),
                &mut *material,
            )
            .unwrap();

        let cipher_suite = match session.cipher_suite() {
            s2n_quic_core::crypto::tls::CipherSuite::TLS_AES_128_GCM_SHA256 => {
                schedule::Ciphersuite::AES_GCM_128_SHA256
            }
            s2n_quic_core::crypto::tls::CipherSuite::TLS_AES_256_GCM_SHA384 => {
                schedule::Ciphersuite::AES_GCM_256_SHA384
            }
            _ => return Err(s2n_quic_core::transport::Error::INTERNAL_ERROR),
        };

        let secret =
            schedule::Secret::new(cipher_suite, self.dc_version, self.endpoint_type, &material);

        let stateless_reset = self.map.signer().sign(secret.id());
        self.secret = Some(secret);

        Ok(vec![stateless_reset.into()])
    }

    fn on_peer_stateless_reset_tokens<'a>(
        &mut self,
        stateless_reset_tokens: impl Iterator<Item = &'a s2n_quic_core::stateless_reset::Token>,
    ) {
        // TODO: support multiple stateless reset tokens
        let sender = sender::State::new(
            stateless_reset_tokens
                .into_iter()
                .next()
                .unwrap()
                .into_inner(),
        );

        let receiver = self.map.state.receiver_shared.clone().new_receiver();

        let entry = Entry::new(
            self.peer,
            self.secret
                .take()
                .expect("peer tokens are only received after secrets are ready"),
            sender,
            receiver,
            self.parameters,
        );
        let entry = Arc::new(entry);
        self.map.insert(entry);
    }
}

#[cfg(test)]
mod test;

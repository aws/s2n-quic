// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{
    cleaner::Cleaner, stateless_reset, ApplicationData, ApplicationDataError, Entry, Store,
};
use crate::{
    credentials::{Credentials, Id},
    crypto,
    event::{self, EndpointPublisher as _, IntoEvent as _},
    packet::{secret_control as control, Packet},
    path::secret::receiver,
};
use s2n_quic_core::{
    inet::SocketAddress,
    time::{self, Timestamp},
    varint::VarInt,
};
use std::{
    collections::VecDeque,
    hash::BuildHasher,
    net::{Ipv4Addr, SocketAddr},
    sync::{Arc, Mutex, RwLock, Weak},
    time::Duration,
};

#[cfg(test)]
mod tests;

#[derive(Default)]
#[repr(align(128))]
pub(crate) struct PeerMap(
    parking_lot::RwLock<hashbrown::HashTable<Arc<Entry>>>,
    std::collections::hash_map::RandomState,
);

#[derive(Default)]
#[repr(align(128))]
pub(crate) struct IdMap(parking_lot::RwLock<hashbrown::HashTable<Arc<Entry>>>);

impl PeerMap {
    fn reserve(&self, additional: usize) {
        self.0.write().reserve(additional, |e| self.hash(e));
    }

    fn hash(&self, entry: &Entry) -> u64 {
        self.hash_key(entry.peer())
    }

    fn hash_key(&self, entry: &SocketAddr) -> u64 {
        self.1.hash_one(entry)
    }

    pub(crate) fn insert(&self, entry: Arc<Entry>) -> Option<Arc<Entry>> {
        let hash = self.hash(&entry);
        let mut map = self.0.write();
        match map.entry(hash, |other| other.peer() == entry.peer(), |e| self.hash(e)) {
            hashbrown::hash_table::Entry::Occupied(mut o) => {
                Some(std::mem::replace(o.get_mut(), entry))
            }
            hashbrown::hash_table::Entry::Vacant(v) => {
                v.insert(entry);
                None
            }
        }
    }

    pub(crate) fn contains_key(&self, ip: &SocketAddr) -> bool {
        let hash = self.hash_key(ip);
        let map = self.0.read();
        map.find(hash, |o| o.peer() == ip).is_some()
    }

    pub(crate) fn get(&self, peer: SocketAddr) -> Option<Arc<Entry>> {
        let hash = self.hash_key(&peer);
        let map = self.0.read();
        map.find(hash, |o| *o.peer() == peer).cloned()
    }

    pub(crate) fn clear(&self) {
        let mut map = self.0.write();
        map.clear();
    }

    pub(super) fn len(&self) -> usize {
        let map = self.0.read();
        map.len()
    }

    fn remove_exact(&self, entry: &Arc<Entry>) -> Option<Arc<Entry>> {
        let hash = self.hash(entry);
        let mut map = self.0.write();
        // Note that we are passing `eq` by **ID** not by address: this ensures that we find the
        // specific entry. The hash is still of the SocketAddr so we will look at the right entries
        // while doing this.
        match map.find_entry(hash, |other| other.id() == entry.id()) {
            Ok(o) => Some(o.remove().0),
            Err(_) => None,
        }
    }
}

impl IdMap {
    fn reserve(&self, additional: usize) {
        self.0.write().reserve(additional, |e| self.hash(e));
    }

    fn hash(&self, entry: &Entry) -> u64 {
        self.hash_key(entry.id())
    }

    fn hash_key(&self, entry: &Id) -> u64 {
        entry.to_hash()
    }

    pub(crate) fn insert(&self, entry: Arc<Entry>) -> Option<Arc<Entry>> {
        let hash = self.hash(&entry);
        let mut map = self.0.write();
        match map.entry(hash, |other| other.id() == entry.id(), |e| self.hash(e)) {
            hashbrown::hash_table::Entry::Occupied(mut o) => {
                Some(std::mem::replace(o.get_mut(), entry))
            }
            hashbrown::hash_table::Entry::Vacant(v) => {
                v.insert(entry);
                None
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn contains_key(&self, id: &Id) -> bool {
        let hash = self.hash_key(id);
        let map = self.0.read();
        map.find(hash, |o| o.id() == id).is_some()
    }

    pub(crate) fn get(&self, id: Id) -> Option<Arc<Entry>> {
        let hash = self.hash_key(&id);
        let map = self.0.read();
        map.find(hash, |o| *o.id() == id).cloned()
    }

    pub(crate) fn clear(&self) {
        let mut map = self.0.write();
        map.clear();
    }

    pub(super) fn len(&self) -> usize {
        let map = self.0.read();
        map.len()
    }

    pub(super) fn remove(&self, id: Id) -> Option<Arc<Entry>> {
        let hash = self.hash_key(&id);
        let mut map = self.0.write();
        match map.find_entry(hash, |other| *other.id() == id) {
            Ok(o) => Some(o.remove().0),
            Err(_) => None,
        }
    }
}

pub(super) struct State<C, S>
where
    C: 'static + time::Clock + Sync + Send,
    S: event::Subscriber,
{
    // This is in number of entries.
    max_capacity: usize,

    rehandshake_period: Duration,

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
    pub(super) peers: PeerMap,

    // All known entries.
    pub(super) ids: IdMap,

    // We evict entries based on FIFO order. When an entry is created, it gets added to the queue.
    // Entries can die in one of two ways: exiting the queue, and replacement due to re-handshaking
    // (if the peer address is the same).
    pub(super) eviction_queue: Mutex<VecDeque<Weak<Entry>>>,

    pub(super) signer: stateless_reset::Signer,

    // This socket is used *only* for sending secret control packets.
    // FIXME: This will get replaced with sending on a handshake socket associated with the map.
    pub(super) control_socket: Arc<std::net::UdpSocket>,

    #[allow(clippy::type_complexity)]
    pub(super) request_handshake: RwLock<Option<Box<dyn Fn(SocketAddr) + Send + Sync>>>,

    cleaner: Cleaner,

    // Avoids allocating/deallocating on each cleaner run.
    // We use a PeerMap to save memory -- an Arc is 8 bytes, SocketAddr is 32 bytes.
    pub(super) cleaner_peer_seen: PeerMap,

    // Lock is acquired only in Cleaner.
    pub(super) rehandshake: Mutex<super::rehandshake::RehandshakeState>,

    init_time: Timestamp,

    pub(super) clock: C,

    subscriber: S,

    #[allow(clippy::type_complexity)]
    mk_application_data: RwLock<
        Option<
            Box<
                dyn Fn(
                        &dyn s2n_quic_core::crypto::tls::TlsSession,
                    ) -> Result<Option<ApplicationData>, ApplicationDataError>
                    + Send
                    + Sync,
            >,
        >,
    >,
}

// Share control sockets -- we only send on these so it doesn't really matter if there's only one
// per process.
static CONTROL_SOCKET: Mutex<Weak<std::net::UdpSocket>> = Mutex::new(Weak::new());

impl<C, S> State<C, S>
where
    C: 'static + time::Clock + Sync + Send,
    S: event::Subscriber,
{
    pub fn new(
        signer: stateless_reset::Signer,
        capacity: usize,
        clock: C,
        subscriber: S,
    ) -> Arc<Self> {
        // FIXME: Avoid unwrap and the whole socket.
        //
        // We only ever send on this socket - but we really should be sending on the same
        // socket as used by an associated s2n-quic handshake runtime, and receiving control packets
        // from that socket as well. Not exactly clear on how to achieve that yet though (both
        // ownership wise since the map doesn't have direct access to handshakes and in terms
        // of implementation).
        let control_socket = {
            let mut guard = CONTROL_SOCKET.lock().unwrap();
            if let Some(socket) = guard.upgrade() {
                socket
            } else {
                let control_socket = std::net::UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).unwrap();
                control_socket.set_nonblocking(true).unwrap();
                let control_socket = Arc::new(control_socket);
                *guard = Arc::downgrade(&control_socket);
                control_socket
            }
        };

        let init_time = clock.get_time();

        // FIXME: Allow configuring the rehandshake_period.
        let rehandshake_period = Duration::from_secs(3600 * 24);

        let mut state = Self {
            // This is around 500MB with current entry size.
            max_capacity: capacity,
            rehandshake_period,
            peers: Default::default(),
            ids: Default::default(),
            eviction_queue: Default::default(),
            cleaner_peer_seen: Default::default(),
            cleaner: Cleaner::new(),
            rehandshake: Mutex::new(super::rehandshake::RehandshakeState::new(
                rehandshake_period,
            )),
            signer,
            control_socket,
            init_time,
            clock,
            subscriber,
            request_handshake: RwLock::new(None),
            mk_application_data: RwLock::new(None),
        };

        // Growing to double our maximum inserted entries should ensure that we never grow again, see:
        // https://github.com/rust-lang/hashbrown/blob/3bcb84537de01372cab2c1cd3bbfd8577a67ce05/src/raw/mod.rs#L2614
        //
        // In practice we don't pin a particular version of hashbrown but there's definitely at
        // most a constant factor of growth left (vs continuous upwards resizing) with any
        // reasonable implementation.
        state.peers.reserve(2 * state.max_capacity);
        state.ids.reserve(2 * state.max_capacity);
        state.cleaner_peer_seen.reserve(2 * state.max_capacity);
        state
            .rehandshake
            .get_mut()
            .unwrap()
            .reserve(state.max_capacity);

        let state = Arc::new(state);

        state.cleaner.spawn_thread(state.clone());

        state
            .subscriber()
            .on_path_secret_map_initialized(event::builder::PathSecretMapInitialized { capacity });

        state
    }

    // Sometimes called with queue lock held -- must not acquire it.
    pub(super) fn evict(&self, evicted: &Arc<Entry>) -> (bool, bool) {
        let mut id_removed = false;
        let mut peer_removed = false;

        // A concurrent cleaner can drop the entry from the `ids` map so we need to
        // re-check whether we actually evicted something.
        if self.ids.remove(*evicted.id()).is_some() {
            id_removed = true;
            self.subscriber().on_path_secret_map_id_entry_evicted(
                event::builder::PathSecretMapIdEntryEvicted {
                    peer_address: SocketAddress::from(*evicted.peer()).into_event(),
                    credential_id: evicted.id().into_event(),
                    age: evicted.age(),
                },
            );
        }

        // A concurrent cleaner can drop the entry from the `peers` map too so we need
        // to re-check whether we actually evicted something.
        //
        // We drop from the peers map only if this is exactly the entry in that map to
        // avoid evicting a newer path secret (in case of rehandshaking with the same
        // peer).
        if self.peers.remove_exact(evicted).is_some() {
            peer_removed = true;
            self.subscriber().on_path_secret_map_address_entry_evicted(
                event::builder::PathSecretMapAddressEntryEvicted {
                    peer_address: SocketAddress::from(*evicted.peer()).into_event(),
                    credential_id: evicted.id().into_event(),
                    age: evicted.age(),
                },
            );
        }

        (id_removed, peer_removed)
    }

    pub fn request_handshake(&self, peer: SocketAddr) {
        self.subscriber()
            .on_path_secret_map_background_handshake_requested(
                event::builder::PathSecretMapBackgroundHandshakeRequested {
                    peer_address: SocketAddress::from(peer).into_event(),
                },
            );

        // Normally we'd expect callers to use the Subscriber to register interest in this, but the
        // Map is typically created *before* the s2n_quic::Client with the dc provider registered.
        //
        // Users of the state tracker typically register the callback when creating a new s2n-quic
        // client to handshake into this map.
        if let Some(callback) = self
            .request_handshake
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .as_deref()
        {
            (callback)(peer);
        }
    }

    fn register_request_handshake(&self, cb: Box<dyn Fn(SocketAddr) + Send + Sync>) {
        // FIXME: Maybe panic if already initialized?
        *self
            .request_handshake
            .write()
            .unwrap_or_else(|e| e.into_inner()) = Some(cb);
    }

    pub fn cleaner(&self) -> &Cleaner {
        &self.cleaner
    }

    // for tests
    #[allow(unused)]
    fn set_max_capacity(&mut self, new: usize) {
        self.max_capacity = new;
        self.peers = Default::default();
        self.ids = Default::default();
    }

    pub(super) fn subscriber(&self) -> event::EndpointPublisherSubscriber<'_, S> {
        use event::IntoEvent as _;

        let timestamp = self.clock.get_time().into_event();

        event::EndpointPublisherSubscriber::new(
            event::builder::EndpointMeta { timestamp },
            None,
            &self.subscriber,
        )
    }
}

impl<C, S> Store for State<C, S>
where
    C: time::Clock + Sync + Send,
    S: event::Subscriber,
{
    fn secrets_len(&self) -> usize {
        self.ids.len()
    }

    fn peers_len(&self) -> usize {
        self.peers.len()
    }

    fn secrets_capacity(&self) -> usize {
        self.max_capacity
    }

    fn drop_state(&self) {
        self.ids.clear();
        self.peers.clear();
    }

    fn contains(&self, peer: &SocketAddr) -> bool {
        self.peers.contains_key(peer)
    }

    fn on_new_path_secrets(&self, entry: Arc<Entry>) {
        let id = *entry.id();
        let peer = entry.peer();

        // This is the only place that inserts into the ID list.
        let same = self.ids.insert(entry.clone());
        if same.is_some() {
            // FIXME: Make insertion fallible and fail handshakes instead?
            panic!("inserting a path secret ID twice");
        }

        {
            let mut queue = self
                .eviction_queue
                .lock()
                .unwrap_or_else(|e| e.into_inner());

            queue.push_back(Arc::downgrade(&entry));

            // We went beyond queue limit, need to prune some entries.
            if queue.len() > self.max_capacity {
                // FIXME: Consider a more interesting algorithm, e.g., scanning the first N entries
                // if the popped entry is still live to see if we can avoid dropping a live entry.
                // May not be worth it in practice.
                let element = queue.pop_front().unwrap();
                // Drop the queue lock prior to dropping element in case we wind up deallocating
                // This reduces lock contention and avoids interleaving locks (requiring careful
                // lock ordering).
                drop(queue);

                if let Some(evicted) = element.upgrade() {
                    self.evict(&evicted);
                }
            }
        }

        self.subscriber().on_path_secret_map_entry_inserted(
            event::builder::PathSecretMapEntryInserted {
                peer_address: SocketAddress::from(*peer).into_event(),
                credential_id: id.into_event(),
            },
        );
    }

    fn on_handshake_complete(&self, entry: Arc<Entry>) {
        let id = *entry.id();
        let peer = *entry.peer();

        if let Some(prev) = self.peers.insert(entry.clone()) {
            // This shouldn't happen due to the panic in on_new_path_secrets, but just
            // in case something went wrong with the secret map we double check here.
            // FIXME: Make insertion fallible and fail handshakes instead?
            let prev_id = *prev.id();
            assert_ne!(prev_id, id, "duplicate path secret id");

            prev.retire(self.cleaner.epoch());

            self.subscriber().on_path_secret_map_entry_replaced(
                event::builder::PathSecretMapEntryReplaced {
                    peer_address: SocketAddress::from(peer).into_event(),
                    new_credential_id: id.into_event(),
                    previous_credential_id: prev_id.into_event(),
                },
            );
        }

        // Note we evict only based on "new entry" and that happens strictly in
        // on_new_path_secrets, on_handshake_complete should never get an entry that's not already
        // in the eviction queue. *Checking* that is unfortunately expensive since it's O(n) on the
        // queue, so we don't do that.

        self.subscriber()
            .on_path_secret_map_entry_ready(event::builder::PathSecretMapEntryReady {
                peer_address: SocketAddress::from(peer).into_event(),
                credential_id: id.into_event(),
            });
    }

    fn register_request_handshake(&self, cb: Box<dyn Fn(SocketAddr) + Send + Sync>) {
        self.register_request_handshake(cb);
    }

    #[allow(clippy::type_complexity)]
    fn register_make_application_data(
        &self,
        cb: Box<
            dyn Fn(
                    &dyn s2n_quic_core::crypto::tls::TlsSession,
                ) -> Result<Option<ApplicationData>, ApplicationDataError>
                + Send
                + Sync,
        >,
    ) {
        // FIXME: Maybe panic if already initialized?
        *self
            .mk_application_data
            .write()
            .unwrap_or_else(|e| e.into_inner()) = Some(cb);
    }

    fn get_by_addr_untracked(&self, peer: &SocketAddr) -> Option<Arc<Entry>> {
        self.peers.get(*peer)
    }

    fn get_by_addr_tracked(&self, peer: &SocketAddr) -> Option<Arc<Entry>> {
        let result = self.peers.get(*peer);

        self.subscriber().on_path_secret_map_address_cache_accessed(
            event::builder::PathSecretMapAddressCacheAccessed {
                peer_address: SocketAddress::from(*peer).into_event(),
                hit: result.is_some(),
            },
        );

        if let Some(entry) = &result {
            entry.set_accessed_addr();
            self.subscriber()
                .on_path_secret_map_address_cache_accessed_hit(
                    event::builder::PathSecretMapAddressCacheAccessedHit {
                        peer_address: SocketAddress::from(*peer).into_event(),
                        age: entry.age(),
                    },
                );
        }

        result
    }

    fn get_by_id_untracked(&self, id: &Id) -> Option<Arc<Entry>> {
        self.ids.get(*id)
    }

    fn get_by_id_tracked(&self, id: &Id) -> Option<Arc<Entry>> {
        let result = self.ids.get(*id);

        self.subscriber().on_path_secret_map_id_cache_accessed(
            event::builder::PathSecretMapIdCacheAccessed {
                credential_id: id.into_event(),
                hit: result.is_some(),
            },
        );

        if let Some(entry) = &result {
            entry.set_accessed_id();
            self.subscriber().on_path_secret_map_id_cache_accessed_hit(
                event::builder::PathSecretMapIdCacheAccessedHit {
                    credential_id: id.into_event(),
                    age: entry.age(),
                },
            );
        }

        result
    }

    fn handle_unexpected_packet(&self, packet: &Packet, peer: &SocketAddr) {
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
            Packet::StaleKey(packet) => {
                let _ = self.handle_stale_key_packet(packet, peer);
            }
            Packet::ReplayDetected(packet) => {
                let _ = self.handle_replay_detected_packet(packet, peer);
            }
            Packet::UnknownPathSecret(packet) => {
                let _ = self.handle_unknown_path_secret_packet(packet, peer);
            }
        }
    }

    fn handle_unknown_path_secret_packet<'a>(
        &self,
        packet: &'a control::unknown_path_secret::Packet,
        peer: &SocketAddr,
    ) -> Option<&'a control::UnknownPathSecret> {
        let peer_address = SocketAddress::from(*peer);
        let peer_address = peer_address.into_event();

        self.subscriber().on_unknown_path_secret_packet_received(
            event::builder::UnknownPathSecretPacketReceived {
                credential_id: packet.credential_id().into_event(),
                peer_address,
            },
        );

        // don't track access patterns here since it's not initiated by the local application
        let Some(entry) = self.get_by_id_untracked(packet.credential_id()) else {
            self.subscriber().on_unknown_path_secret_packet_dropped(
                event::builder::UnknownPathSecretPacketDropped {
                    credential_id: packet.credential_id().into_event(),
                    peer_address,
                },
            );

            return None;
        };

        // Do not mark as live, this is lightly authenticated.

        // ensure the packet is authentic
        let Some(packet) = packet.authenticate(&entry.sender().stateless_reset) else {
            self.subscriber().on_unknown_path_secret_packet_rejected(
                event::builder::UnknownPathSecretPacketRejected {
                    credential_id: packet.credential_id().into_event(),
                    peer_address,
                },
            );

            return None;
        };

        self.subscriber().on_unknown_path_secret_packet_accepted(
            event::builder::UnknownPathSecretPacketAccepted {
                credential_id: packet.credential_id.into_event(),
                peer_address,
            },
        );

        // FIXME: More actively schedule a new handshake.
        // See comment on requested_handshakes for details.
        self.request_handshake(*entry.peer());

        Some(packet)
    }

    fn handle_stale_key_packet<'a>(
        &self,
        packet: &'a control::stale_key::Packet,
        peer: &SocketAddr,
    ) -> Option<&'a control::StaleKey> {
        let peer_address = SocketAddress::from(*peer);
        let peer_address = peer_address.into_event();

        self.subscriber()
            .on_stale_key_packet_received(event::builder::StaleKeyPacketReceived {
                credential_id: packet.credential_id().into_event(),
                peer_address,
            });

        let Some(entry) = self.ids.get(*packet.credential_id()) else {
            self.subscriber()
                .on_stale_key_packet_dropped(event::builder::StaleKeyPacketDropped {
                    credential_id: packet.credential_id().into_event(),
                    peer_address,
                });
            return None;
        };

        let key = entry.control_opener();

        let Some(packet) = packet.authenticate(&key) else {
            self.subscriber().on_stale_key_packet_rejected(
                event::builder::StaleKeyPacketRejected {
                    credential_id: packet.credential_id().into_event(),
                    peer_address,
                },
            );

            return None;
        };

        self.subscriber()
            .on_stale_key_packet_accepted(event::builder::StaleKeyPacketAccepted {
                credential_id: packet.credential_id.into_event(),
                peer_address,
            });

        entry.sender().update_for_stale_key(packet.min_key_id);

        Some(packet)
    }

    fn handle_replay_detected_packet<'a>(
        &self,
        packet: &'a control::replay_detected::Packet,
        peer: &SocketAddr,
    ) -> Option<&'a control::ReplayDetected> {
        let peer_address = SocketAddress::from(*peer);
        let peer_address = peer_address.into_event();

        self.subscriber().on_replay_detected_packet_received(
            event::builder::ReplayDetectedPacketReceived {
                credential_id: packet.credential_id().into_event(),
                peer_address,
            },
        );

        let Some(entry) = self.ids.get(*packet.credential_id()) else {
            self.subscriber().on_replay_detected_packet_dropped(
                event::builder::ReplayDetectedPacketDropped {
                    credential_id: packet.credential_id().into_event(),
                    peer_address,
                },
            );
            return None;
        };

        let key = entry.control_opener();

        let Some(packet) = packet.authenticate(&key) else {
            self.subscriber().on_replay_detected_packet_rejected(
                event::builder::ReplayDetectedPacketRejected {
                    credential_id: packet.credential_id().into_event(),
                    peer_address,
                },
            );
            return None;
        };

        self.subscriber().on_replay_detected_packet_accepted(
            event::builder::ReplayDetectedPacketAccepted {
                credential_id: packet.credential_id.into_event(),
                key_id: packet.rejected_key_id.into_event(),
                peer_address,
            },
        );

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
        self.request_handshake(*entry.peer());

        Some(packet)
    }

    fn signer(&self) -> &stateless_reset::Signer {
        &self.signer
    }

    fn send_control_packet(&self, dst: &SocketAddr, buffer: &mut [u8]) {
        match self.control_socket.send_to(buffer, dst) {
            Ok(_) => {
                // all done
                match control::Packet::decode(s2n_codec::DecoderBufferMut::new(buffer))
                    .map(|(t, _)| t)
                {
                    Ok(control::Packet::UnknownPathSecret(packet)) => {
                        self.subscriber().on_unknown_path_secret_packet_sent(
                            event::builder::UnknownPathSecretPacketSent {
                                peer_address: SocketAddress::from(*dst).into_event(),
                                credential_id: packet.credential_id().into_event(),
                            },
                        );
                    }
                    Ok(control::Packet::StaleKey(packet)) => {
                        self.subscriber().on_stale_key_packet_sent(
                            event::builder::StaleKeyPacketSent {
                                peer_address: SocketAddress::from(*dst).into_event(),
                                credential_id: packet.credential_id().into_event(),
                            },
                        );
                    }
                    Ok(control::Packet::ReplayDetected(packet)) => {
                        self.subscriber().on_replay_detected_packet_sent(
                            event::builder::ReplayDetectedPacketSent {
                                peer_address: SocketAddress::from(*dst).into_event(),
                                credential_id: packet.credential_id().into_event(),
                            },
                        );
                    }
                    Err(err) => debug_assert!(false, "decoder error {err:?}"),
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                // ignore would block -- we're not going to queue up control packet messages.
            }
            Err(e) => {
                tracing::warn!("Failed to send control packet to {:?}: {:?}", dst, e);
            }
        }
    }

    fn rehandshake_period(&self) -> Duration {
        self.rehandshake_period
    }

    fn check_dedup(
        &self,
        entry: &Entry,
        key_id: s2n_quic_core::varint::VarInt,
        queue_id: Option<VarInt>,
    ) -> crypto::open::Result {
        let creds = &Credentials {
            id: *entry.id(),
            key_id,
        };
        let starting = *entry.receiver().minimum_unseen_key_id();
        match entry.receiver().post_authentication(creds) {
            Ok(()) => {
                let gap = (*entry.receiver().minimum_unseen_key_id())
                    // This should never be negative, but saturate anyway to avoid
                    // wildly large numbers.
                    .saturating_sub(*creds.key_id);

                self.subscriber()
                    .on_key_accepted(event::builder::KeyAccepted {
                        credential_id: creds.id.into_event(),
                        key_id: key_id.into_event(),
                        gap,
                        forward_shift: (*creds.key_id).saturating_sub(starting),
                    });
                Ok(())
            }
            Err(receiver::Error::AlreadyExists) => {
                self.send_control_error(entry, creds, queue_id, receiver::Error::AlreadyExists);

                self.subscriber().on_replay_definitely_detected(
                    event::builder::ReplayDefinitelyDetected {
                        credential_id: creds.id.into_event(),
                        key_id: key_id.into_event(),
                    },
                );

                Err(crypto::open::Error::ReplayDefinitelyDetected)
            }
            Err(receiver::Error::Unknown) => {
                self.send_control_error(entry, creds, queue_id, receiver::Error::Unknown);

                let gap = (*entry.receiver().minimum_unseen_key_id())
                    // This should never be negative, but saturate anyway to avoid
                    // wildly large numbers.
                    .saturating_sub(*creds.key_id);

                self.subscriber().on_replay_potentially_detected(
                    event::builder::ReplayPotentiallyDetected {
                        credential_id: creds.id.into_event(),
                        key_id: key_id.into_event(),
                        gap,
                    },
                );

                Err(crypto::open::Error::ReplayPotentiallyDetected { gap: Some(gap) })
            }
        }
    }

    #[cfg(test)]
    fn test_stop_cleaner(&self) {
        self.cleaner.stop();
    }

    fn application_data(
        &self,
        session: &dyn s2n_quic_core::crypto::tls::TlsSession,
    ) -> Result<Option<ApplicationData>, ApplicationDataError> {
        if let Some(ctxt) = &*self
            .mk_application_data
            .read()
            .unwrap_or_else(|e| e.into_inner())
        {
            (ctxt)(session)
        } else {
            Ok(None)
        }
    }
}

impl<C, S> Drop for State<C, S>
where
    C: 'static + time::Clock + Sync + Send,
    S: event::Subscriber,
{
    fn drop(&mut self) {
        if std::thread::panicking() {
            return;
        }

        let lifetime = self
            .clock
            .get_time()
            .saturating_duration_since(self.init_time);

        self.subscriber().on_path_secret_map_uninitialized(
            event::builder::PathSecretMapUninitialized {
                capacity: self.secrets_capacity(),
                entries: self.secrets_len(),
                lifetime,
            },
        );
    }
}

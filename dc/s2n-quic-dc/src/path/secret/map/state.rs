// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{cleaner::Cleaner, stateless_reset, Entry, Store};
use crate::{
    credentials::Id,
    fixed_map::{self, ReadGuard},
    packet::{secret_control as control, Packet},
    path::secret::receiver,
};
use std::{
    hash::{BuildHasherDefault, Hasher},
    net::{Ipv4Addr, SocketAddr},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

#[cfg(test)]
mod tests;

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
    pub(super) peers: fixed_map::Map<SocketAddr, Arc<Entry>>,

    // Stores the set of SocketAddr for which we received a UnknownPathSecret packet.
    // When handshake_with is called we will allow a new handshake if this contains a socket, this
    // is a temporary solution until we implement proper background handshaking.
    pub(super) requested_handshakes: flurry::HashSet<SocketAddr>,

    // All known entries.
    pub(super) ids: fixed_map::Map<Id, Arc<Entry>, BuildHasherDefault<NoopIdHasher>>,

    pub(super) signer: stateless_reset::Signer,

    // This socket is used *only* for sending secret control packets.
    // FIXME: This will get replaced with sending on a handshake socket associated with the map.
    pub(super) control_socket: std::net::UdpSocket,

    pub(super) receiver_shared: Arc<receiver::Shared>,

    handled_control_packets: AtomicUsize,

    cleaner: Cleaner,
}

impl State {
    pub fn new(signer: stateless_reset::Signer, capacity: usize) -> Arc<Self> {
        // FIXME: Avoid unwrap and the whole socket.
        //
        // We only ever send on this socket - but we really should be sending on the same
        // socket as used by an associated s2n-quic handshake runtime, and receiving control packets
        // from that socket as well. Not exactly clear on how to achieve that yet though (both
        // ownership wise since the map doesn't have direct access to handshakes and in terms
        // of implementation).
        let control_socket = std::net::UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).unwrap();
        control_socket.set_nonblocking(true).unwrap();

        let state = Self {
            // This is around 500MB with current entry size.
            max_capacity: capacity,
            // FIXME: Allow configuring the rehandshake_period.
            rehandshake_period: Duration::from_secs(3600 * 24),
            peers: fixed_map::Map::with_capacity(capacity, Default::default()),
            ids: fixed_map::Map::with_capacity(capacity, Default::default()),
            requested_handshakes: Default::default(),
            cleaner: Cleaner::new(),
            signer,

            receiver_shared: receiver::Shared::new(),

            handled_control_packets: AtomicUsize::new(0),
            control_socket,
        };

        let state = Arc::new(state);

        state.cleaner.spawn_thread(state.clone());

        state
    }

    pub fn request_handshake(&self, peer: SocketAddr) {
        // The length is reset as part of cleanup to 5000.
        let handshakes = self.requested_handshakes.pin();
        if handshakes.len() <= 6000 {
            handshakes.insert(peer);
        }
    }

    fn handle_unknown_secret_packet(&self, packet: &control::unknown_path_secret::Packet) {
        let Some(entry) = self.get_by_id(packet.credential_id()) else {
            return;
        };
        // Do not mark as live, this is lightly authenticated.

        // ensure the packet is authentic
        if packet
            .authenticate(&entry.sender().stateless_reset)
            .is_none()
        {
            return;
        }

        self.handled_control_packets.fetch_add(1, Ordering::Relaxed);

        // FIXME: More actively schedule a new handshake.
        // See comment on requested_handshakes for details.
        self.request_handshake(*entry.peer());
    }

    pub fn cleaner(&self) -> &Cleaner {
        &self.cleaner
    }

    // for tests
    #[allow(unused)]
    fn set_max_capacity(&mut self, new: usize) {
        self.max_capacity = new;
        self.peers = fixed_map::Map::with_capacity(new, Default::default());
        self.ids = fixed_map::Map::with_capacity(new, Default::default());
    }
}

impl Store for State {
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

    fn contains(&self, peer: SocketAddr) -> bool {
        self.peers.contains_key(&peer) && !self.requested_handshakes.pin().contains(&peer)
    }

    fn on_new_path_secrets(&self, entry: Arc<Entry>) {
        // On insert clear our interest in a handshake.
        self.requested_handshakes.pin().remove(entry.peer());
        let id = *entry.id();
        if self.ids.insert(id, entry.clone()).is_some() {
            // FIXME: Make insertion fallible and fail handshakes instead?
            panic!("inserting a path secret ID twice");
        }
    }

    fn on_handshake_complete(&self, entry: Arc<Entry>) {
        let id = *entry.id();
        let peer = *entry.peer();
        if let Some(prev) = self.peers.insert(peer, entry) {
            // This shouldn't happen due to the panic in on_new_path_secrets, but just
            // in case something went wrong with the secret map we double check here.
            // FIXME: Make insertion fallible and fail handshakes instead?
            assert_ne!(*prev.id(), id, "duplicate path secret id");

            prev.retire(self.cleaner.epoch());
        }
    }

    fn get_by_addr(&self, peer: &SocketAddr) -> Option<ReadGuard<Arc<Entry>>> {
        self.peers.get_by_key(peer)
    }

    fn get_by_id(&self, id: &Id) -> Option<ReadGuard<Arc<Entry>>> {
        self.ids.get_by_key(id)
    }

    fn handle_control_packet(&self, packet: &control::Packet) {
        if let control::Packet::UnknownPathSecret(ref packet) = &packet {
            return self.handle_unknown_secret_packet(packet);
        }

        let Some(entry) = self.ids.get_by_key(packet.credential_id()) else {
            // If we get a control packet we don't have a registered path secret for, ignore the
            // packet.
            return;
        };

        let key = entry.control_secret();

        match packet {
            control::Packet::StaleKey(packet) => {
                let Some(packet) = packet.authenticate(&key) else {
                    return;
                };
                entry.sender().update_for_stale_key(packet.min_key_id);
                self.handled_control_packets.fetch_add(1, Ordering::Relaxed);
            }
            control::Packet::ReplayDetected(packet) => {
                let Some(_packet) = packet.authenticate(&key) else {
                    return;
                };
                self.handled_control_packets.fetch_add(1, Ordering::Relaxed);

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
            }
            control::Packet::UnknownPathSecret(_) => unreachable!(),
        }
    }

    fn handle_unexpected_packet(&self, packet: &Packet) {
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

    fn signer(&self) -> &stateless_reset::Signer {
        &self.signer
    }

    fn receiver(&self) -> &Arc<receiver::Shared> {
        &self.receiver_shared
    }

    fn send_control_packet(&self, dst: &SocketAddr, buffer: &[u8]) {
        match self.control_socket.send_to(buffer, dst) {
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

    fn rehandshake_period(&self) -> Duration {
        self.rehandshake_period
    }
}

#[derive(Default)]
pub(super) struct NoopIdHasher(Option<u64>);

impl Hasher for NoopIdHasher {
    fn finish(&self) -> u64 {
        self.0.unwrap()
    }

    fn write(&mut self, _bytes: &[u8]) {
        unimplemented!()
    }

    fn write_u64(&mut self, x: u64) {
        debug_assert!(self.0.is_none());
        self.0 = Some(x);
    }
}

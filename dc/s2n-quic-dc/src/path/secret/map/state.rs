// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{cleaner::Cleaner, stateless_reset, Entry, Store};
use crate::{
    credentials::{Credentials, Id},
    crypto,
    event::{self, EndpointPublisher as _, IntoEvent as _},
    fixed_map::{self, ReadGuard},
    packet::{secret_control as control, Packet},
    path::secret::{receiver, HandshakeKind},
};
use s2n_quic_core::{
    inet::SocketAddress,
    time::{self, Timestamp},
};
use std::{
    hash::{BuildHasherDefault, Hasher},
    net::{Ipv4Addr, SocketAddr},
    sync::Arc,
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

    cleaner: Cleaner,

    init_time: Timestamp,

    clock: C,

    subscriber: S,
}

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
        let control_socket = std::net::UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).unwrap();
        control_socket.set_nonblocking(true).unwrap();

        let init_time = clock.get_time();

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
            control_socket,
            init_time,
            clock,
            subscriber,
        };

        let state = Arc::new(state);

        state.cleaner.spawn_thread(state.clone());

        state
            .subscriber()
            .on_path_secret_map_initialized(event::builder::PathSecretMapInitialized { capacity });

        state
    }

    pub fn request_handshake(&self, peer: SocketAddr) {
        // The length is reset as part of cleanup to 5000.
        let handshakes = self.requested_handshakes.pin();
        if handshakes.len() <= 6000 {
            handshakes.insert(peer);
            self.subscriber()
                .on_path_secret_map_background_handshake_requested(
                    event::builder::PathSecretMapBackgroundHandshakeRequested {
                        peer_address: SocketAddress::from(peer).into_event(),
                    },
                );
        }
    }

    fn handle_unknown_secret(
        &self,
        packet: &control::unknown_path_secret::Packet,
        peer: &SocketAddress,
    ) {
        let peer_address = peer.into_event();

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

            return;
        };

        // Do not mark as live, this is lightly authenticated.

        // ensure the packet is authentic
        if packet
            .authenticate(&entry.sender().stateless_reset)
            .is_none()
        {
            self.subscriber().on_unknown_path_secret_packet_rejected(
                event::builder::UnknownPathSecretPacketRejected {
                    credential_id: packet.credential_id().into_event(),
                    peer_address,
                },
            );

            return;
        }

        self.subscriber().on_unknown_path_secret_packet_accepted(
            event::builder::UnknownPathSecretPacketAccepted {
                credential_id: packet.credential_id().into_event(),
                peer_address,
            },
        );

        // FIXME: More actively schedule a new handshake.
        // See comment on requested_handshakes for details.
        self.request_handshake(*entry.peer());
    }

    fn handle_stale_key(&self, packet: &control::stale_key::Packet, peer: &SocketAddress) {
        let peer_address = peer.into_event();

        self.subscriber()
            .on_stale_key_packet_received(event::builder::StaleKeyPacketReceived {
                credential_id: packet.credential_id().into_event(),
                peer_address,
            });

        let Some(entry) = self.ids.get_by_key(packet.credential_id()) else {
            self.subscriber()
                .on_stale_key_packet_dropped(event::builder::StaleKeyPacketDropped {
                    credential_id: packet.credential_id().into_event(),
                    peer_address,
                });
            return;
        };

        let key = entry.control_opener();

        let Some(packet) = packet.authenticate(&key) else {
            self.subscriber().on_stale_key_packet_rejected(
                event::builder::StaleKeyPacketRejected {
                    credential_id: packet.credential_id().into_event(),
                    peer_address,
                },
            );

            return;
        };

        self.subscriber()
            .on_stale_key_packet_accepted(event::builder::StaleKeyPacketAccepted {
                credential_id: packet.credential_id.into_event(),
                peer_address,
            });

        entry.sender().update_for_stale_key(packet.min_key_id);
    }

    fn handle_replay_detected(
        &self,
        packet: &control::replay_detected::Packet,
        peer: &SocketAddress,
    ) {
        let peer_address = peer.into_event();

        self.subscriber().on_replay_detected_packet_received(
            event::builder::ReplayDetectedPacketReceived {
                credential_id: packet.credential_id().into_event(),
                peer_address,
            },
        );

        let Some(entry) = self.ids.get_by_key(packet.credential_id()) else {
            self.subscriber().on_replay_detected_packet_dropped(
                event::builder::ReplayDetectedPacketDropped {
                    credential_id: packet.credential_id().into_event(),
                    peer_address,
                },
            );
            return;
        };

        let key = entry.control_opener();

        let Some(packet) = packet.authenticate(&key) else {
            self.subscriber().on_replay_detected_packet_rejected(
                event::builder::ReplayDetectedPacketRejected {
                    credential_id: packet.credential_id().into_event(),
                    peer_address,
                },
            );
            return;
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

    fn subscriber(&self) -> event::EndpointPublisherSubscriber<S> {
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

    fn contains(&self, peer: SocketAddr) -> bool {
        self.peers.contains_key(&peer) && !self.requested_handshakes.pin().contains(&peer)
    }

    fn on_new_path_secrets(&self, entry: Arc<Entry>) {
        let id = *entry.id();
        let peer = entry.peer();

        // On insert clear our interest in a handshake.
        self.requested_handshakes.pin().remove(peer);

        if self.ids.insert(id, entry.clone()).is_some() {
            // FIXME: Make insertion fallible and fail handshakes instead?
            panic!("inserting a path secret ID twice");
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

        if let Some(prev) = self.peers.insert(peer, entry) {
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

        self.subscriber()
            .on_path_secret_map_entry_ready(event::builder::PathSecretMapEntryReady {
                peer_address: SocketAddress::from(peer).into_event(),
                credential_id: id.into_event(),
            });
    }

    fn get_by_addr_untracked(&self, peer: &SocketAddr) -> Option<ReadGuard<Arc<Entry>>> {
        self.peers.get_by_key(peer).filter(|_| {
            // ensure this entry isn't requested to rehandshake
            !self.requested_handshakes.pin().contains(peer)
        })
    }

    fn get_by_addr_tracked(
        &self,
        peer: &SocketAddr,
        handshake: HandshakeKind,
    ) -> Option<ReadGuard<Arc<Entry>>> {
        let result = self.peers.get_by_key(peer)?;

        // If this is trying to use a cached handshake but we've got a request to do a handshake, then
        // force the application to do a new handshake. This is consistent with the `contains` method.
        if matches!(handshake, HandshakeKind::Cached)
            && self.requested_handshakes.pin().contains(peer)
        {
            return None;
        }

        self.subscriber().on_path_secret_map_address_cache_accessed(
            event::builder::PathSecretMapAddressCacheAccessed {
                peer_address: SocketAddress::from(*peer).into_event(),
                hit: matches!(handshake, HandshakeKind::Cached),
            },
        );

        Some(result)
    }

    fn get_by_id_untracked(&self, id: &Id) -> Option<ReadGuard<Arc<Entry>>> {
        self.ids.get_by_key(id)
    }

    fn get_by_id_tracked(&self, id: &Id) -> Option<ReadGuard<Arc<Entry>>> {
        let result = self.ids.get_by_key(id);

        self.subscriber().on_path_secret_map_id_cache_accessed(
            event::builder::PathSecretMapIdCacheAccessed {
                credential_id: id.into_event(),
                hit: result.is_some(),
            },
        );

        result
    }

    fn handle_control_packet(&self, packet: &control::Packet, peer: &SocketAddr) {
        match packet {
            control::Packet::StaleKey(packet) => self.handle_stale_key(packet, &(*peer).into()),
            control::Packet::ReplayDetected(packet) => {
                self.handle_replay_detected(packet, &(*peer).into())
            }
            control::Packet::UnknownPathSecret(packet) => {
                self.handle_unknown_secret(packet, &(*peer).into())
            }
        }
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
            Packet::StaleKey(packet) => self.handle_stale_key(packet, &(*peer).into()),
            Packet::ReplayDetected(packet) => self.handle_replay_detected(packet, &(*peer).into()),
            Packet::UnknownPathSecret(packet) => {
                self.handle_unknown_secret(packet, &(*peer).into())
            }
        }
    }

    fn signer(&self) -> &stateless_reset::Signer {
        &self.signer
    }

    fn receiver(&self) -> &Arc<receiver::Shared> {
        &self.receiver_shared
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
                self.send_control_error(entry, creds, receiver::Error::AlreadyExists);

                self.subscriber().on_replay_definitely_detected(
                    event::builder::ReplayDefinitelyDetected {
                        credential_id: creds.id.into_event(),
                        key_id: key_id.into_event(),
                    },
                );

                Err(crypto::open::Error::ReplayDefinitelyDetected)
            }
            Err(receiver::Error::Unknown) => {
                self.send_control_error(entry, creds, receiver::Error::Unknown);

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

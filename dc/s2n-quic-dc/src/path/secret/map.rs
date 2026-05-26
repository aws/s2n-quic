// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    credentials::{Credentials, Id},
    crypto::{self, awslc},
    event,
    packet::{secret_control as control, Packet},
    path::secret::{
        open,
        schedule::{Ciphersuite, ExportSecret},
        seal, stateless_reset,
    },
    psk::io::HandshakeReason,
};
use core::fmt;
use s2n_quic_core::{dc, time, varint::VarInt};
use std::{net::SocketAddr, sync::Arc};
use tokio::task::JoinHandle;

mod cleaner;
mod entry;
mod handshake;
mod peer;
mod rehandshake;
mod size_of;
mod state;
mod status;
pub(crate) mod store;

#[cfg(any(test, feature = "testing"))]
pub mod testing;

#[cfg(test)]
mod event_tests;

#[cfg(any(test, feature = "testing"))]
#[derive(Clone, Copy, Debug)]
pub struct TestPairIds {
    pub local: crate::credentials::Id,
    pub peer: crate::credentials::Id,
}

pub use entry::Entry;
use store::Store;

#[cfg(any(test, feature = "testing"))]
pub use entry::TestEntryBuilder;
pub use entry::{
    ApplicationData, ApplicationDataError, ApplicationPair, Bidirectional, ControlPair,
    PeerDataAddrs, MAX_PEER_DATA_ADDRS,
};
pub use handshake::HandshakingPath;
pub use peer::Peer;

pub(crate) use size_of::SizeOf;
pub(crate) use status::Dedup;

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
    store: Arc<dyn Store>,
}

impl PartialEq for Map {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.store, &other.store)
    }
}

impl Eq for Map {}

impl fmt::Debug for Map {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Map")
            .field("secrets_len", &self.secrets_len())
            .field("peers_len", &self.peers_len())
            .field("secrets_capacity", &self.secrets_capacity())
            .finish_non_exhaustive()
    }
}

impl Map {
    pub fn new<C, S>(
        signer: stateless_reset::Signer,
        capacity: usize,
        should_evict_on_unknown_path_secret: bool,
        clock: C,
        subscriber: S,
    ) -> Self
    where
        C: 'static + time::Clock + Send + Sync,
        S: event::Subscriber,
    {
        let store = state::State::builder()
            .with_signer(signer)
            .with_capacity(capacity)
            .with_evict_on_unknown_path_secret(should_evict_on_unknown_path_secret)
            .with_clock(clock)
            .with_subscriber(subscriber)
            .build()
            .unwrap();

        Self { store }
    }

    /// The number of trusted secrets.
    pub fn secrets_len(&self) -> usize {
        self.store.secrets_len()
    }

    /// The number of trusted peers.
    ///
    /// This should be smaller than `secrets_len` (modulo momentary churn).
    pub fn peers_len(&self) -> usize {
        self.store.peers_len()
    }

    pub fn secrets_capacity(&self) -> usize {
        self.store.secrets_capacity()
    }

    pub fn socket_sender_count(&self) -> usize {
        self.store.socket_sender_count()
    }

    pub fn set_socket_sender_count(&self, count: usize) {
        self.store.set_socket_sender_count(count);
    }

    pub fn drop_state(&self) {
        self.store.drop_state();
    }

    pub fn contains(&self, peer: &SocketAddr) -> bool {
        self.store.contains(peer)
    }

    pub fn register_request_handshake(
        &self,
        cb: Box<dyn Fn(SocketAddr, HandshakeReason) -> Option<JoinHandle<()>> + Send + Sync>,
    ) {
        self.store.register_request_handshake(cb);
    }

    /// Gets the [`Peer`] entry for the given address
    ///
    /// NOTE: This function is used to track cache hit ratios so it
    ///       should only be used for connection attempts.
    pub fn get_tracked(&self, peer: SocketAddr) -> Option<Peer> {
        let entry = self.store.get_by_addr_tracked(&peer)?;
        Some(Peer::new(&entry, self))
    }

    /// Gets the [`Peer`] entry for the given address
    ///
    /// NOTE: This function is used to track cache hit ratios so it
    ///       should only be used for connection attempts.
    pub fn get_untracked(&self, peer: SocketAddr) -> Option<Peer> {
        let entry = self.store.get_by_addr_untracked(&peer)?;
        Some(Peer::new(&entry, self))
    }

    /// Get the raw entry for a peer address.
    ///
    /// This bypasses the Peer wrapper and returns the underlying Arc<Entry> directly.
    /// Useful for low-level datagram transmission where you need the entry for creating
    /// PartialDatagram packets.
    pub fn get_raw(&self, peer: SocketAddr) -> Option<Arc<Entry>> {
        self.store.get_by_addr_untracked(&peer)
    }

    /// Retrieve a sealer by path secret ID.
    ///
    /// Generally callers should prefer to use one of the `pair` APIs; this is primarily useful for
    /// "response" datagrams which want to be bound to the exact same shared secret.
    ///
    /// Note that unlike by-IP lookup this should typically not be done significantly after the
    /// original secret was used for decryption.
    pub fn seal_once_id(&self, id: Id) -> Option<(seal::Once, Credentials, dc::ApplicationParams)> {
        let entry = self.store.get_by_id_tracked(&id)?;
        let (sealer, credentials) = entry.uni_sealer();
        Some((sealer, credentials, entry.parameters()))
    }

    /// Get a reusable opener for the given credentials.
    ///
    /// This performs authentication and returns a reusable opener with the entry if valid.
    /// If authentication fails or the credentials are unknown, `control_out` will
    /// be populated with an appropriate error control packet to send back.
    ///
    /// Returns the raw AEAD opener (from awslc) that can be cached and reused.
    /// For datagrams, this is sufficient as we don't need key updates or the
    /// additional wrapper logic.
    pub fn opener_for_credentials(
        &self,
        credentials: &Credentials,
        queue_id: Option<VarInt>,
        control: store::ControlResponse<'_>,
    ) -> Option<(awslc::open::Application, Arc<Entry>)> {
        let entry = self
            .store
            .pre_authentication(credentials, queue_id, control)?;
        let key_id = credentials.key_id;
        let opener = entry.secret().application_opener(key_id);
        Some((opener, entry))
    }

    pub fn open_once(
        &self,
        credentials: &Credentials,
        queue_id: Option<VarInt>,
        control: store::ControlResponse<'_>,
    ) -> Option<open::Once> {
        let entry = self
            .store
            .pre_authentication(credentials, queue_id, control)?;
        let opener = entry.uni_opener(self.clone(), credentials, queue_id);
        Some(opener)
    }

    pub fn pair_for_credentials(
        &self,
        credentials: &Credentials,
        queue_id: Option<VarInt>,
        control: store::ControlResponse<'_>,
    ) -> Option<(entry::Bidirectional, dc::ApplicationParams)> {
        let entry = self
            .store
            .pre_authentication(credentials, queue_id, control)?;

        let params = entry.parameters();
        let keys = entry.bidi_remote(self.clone(), credentials, queue_id);

        Some((keys, params))
    }

    pub fn secret_for_credentials(
        &self,
        credentials: &Credentials,
        queue_id: Option<VarInt>,
        control: store::ControlResponse<'_>,
    ) -> Option<(
        ExportSecret,
        Ciphersuite,
        entry::Bidirectional,
        dc::ApplicationParams,
    )> {
        let entry = self
            .store
            .pre_authentication(credentials, queue_id, control)?;
        let params = entry.parameters();
        let keys = entry.bidi_remote(self.clone(), credentials, queue_id);
        let secret = entry.secret();

        Some((*secret.export_secret(), *secret.ciphersuite(), keys, params))
    }

    /// This can be called from anywhere to ask the map to handle a packet.
    ///
    /// For secret control packets, this will process those.
    /// For other packets, the map may collect metrics but will otherwise drop the packets.
    pub fn handle_unexpected_packet(&self, packet: &Packet, peer: &SocketAddr) {
        self.store.handle_unexpected_packet(packet, peer);
    }

    /// Emits a DcConnectionTimeout event via the subscriber
    pub fn on_dc_connection_timeout(&self, peer_address: &SocketAddr) {
        self.store.on_dc_connection_timeout(peer_address);
    }

    pub fn handle_control_packet(&self, packet: &control::Packet, peer: &SocketAddr) {
        match packet {
            control::Packet::StaleKey(packet) => {
                let _ = self.handle_stale_key_packet(packet, peer);
            }
            control::Packet::ReplayDetected(packet) => {
                let _ = self.handle_replay_detected_packet(packet, peer);
            }
            control::Packet::UnknownPathSecret(packet) => {
                let _ = self.handle_unknown_path_secret_packet(packet, peer);
            }
        }
    }

    pub fn sign_queue_reset_packet(
        &self,
        packet: &control::QueueReset,
        out: &mut [u8],
    ) -> Option<usize> {
        self.store.sign_queue_reset_packet(packet, out)
    }

    pub fn handle_queue_reset_packet<'a>(
        &self,
        packet: &'a control::queue_reset::Packet,
        peer: &SocketAddr,
    ) -> Option<&'a control::QueueReset> {
        self.store.handle_queue_reset_packet(packet, peer)
    }

    pub fn handle_stale_key_packet<'a>(
        &self,
        packet: &'a control::stale_key::Packet,
        peer: &SocketAddr,
    ) -> Option<&'a control::StaleKey> {
        self.store.handle_stale_key_packet(packet, peer)
    }

    pub fn handle_replay_detected_packet<'a>(
        &self,
        packet: &'a control::replay_detected::Packet,
        peer: &SocketAddr,
    ) -> Option<&'a control::ReplayDetected> {
        self.store.handle_replay_detected_packet(packet, peer)
    }

    pub fn handle_unknown_path_secret_packet<'a>(
        &self,
        packet: &'a control::unknown_path_secret::Packet,
        peer: &SocketAddr,
    ) -> Option<&'a control::UnknownPathSecret> {
        self.store.handle_unknown_path_secret_packet(packet, peer)
    }

    #[doc(hidden)]
    #[cfg(any(test, feature = "testing"))]
    pub fn for_test_with_peers(
        peers: Vec<(
            crate::path::secret::schedule::Ciphersuite,
            dc::Version,
            SocketAddr,
        )>,
    ) -> (Self, Vec<Id>) {
        use crate::path::secret::{receiver, schedule, sender};

        let provider = Self::new(
            stateless_reset::Signer::random(),
            peers.len() * 3,
            false,
            time::NoopClock,
            event::testing::Subscriber::no_snapshot(),
        );
        let mut secret = [0; 32];
        aws_lc_rs::rand::fill(&mut secret).unwrap();
        let mut stateless_reset = [0; control::TAG_LEN];
        aws_lc_rs::rand::fill(&mut stateless_reset).unwrap();

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
                receiver::State::new(),
                dc::testing::TEST_APPLICATION_PARAMS,
                crate::time::now(),
                None,
            );
            let entry = Arc::new(entry);
            provider.store.test_insert(entry);
        }

        (provider, ids)
    }

    #[doc(hidden)]
    #[cfg(test)]
    pub fn test_stop_cleaner(&self) {
        self.store.test_stop_cleaner();
    }

    #[doc(hidden)]
    #[cfg(test)]
    pub fn reset_all_senders(&self) {
        self.store.reset_all_senders();
    }

    /// Look up a path secret entry by credential ID.
    ///
    /// This is available for testing only; production code should use
    /// [`opener_for_credentials`](Self::opener_for_credentials) which also
    /// performs pre-authentication.
    #[doc(hidden)]
    #[cfg(any(test, feature = "testing"))]
    pub fn get_by_id(&self, id: &Id) -> Option<Arc<Entry>> {
        self.store.get_by_id_untracked(id)
    }

    #[doc(hidden)]
    #[cfg(any(test, feature = "testing"))]
    pub fn test_insert(&self, peer: SocketAddr, endpoint_type: s2n_quic_core::endpoint::Type) {
        let entry = Entry::builder(peer)
            .endpoint_type(endpoint_type)
            .socket_sender_count(self.store.socket_sender_count())
            .build();
        self.store.test_insert(entry);
    }

    #[cfg(any(test, feature = "testing"))]
    pub(crate) fn test_insert_pair(
        &self,
        local_addr: SocketAddr,
        local_params: Option<dc::ApplicationParams>,
        peer: &Self,
        peer_addr: SocketAddr,
        peer_params: Option<dc::ApplicationParams>,
    ) -> TestPairIds {
        use s2n_quic_core::endpoint::Type;

        let mut local_builder = Entry::builder(peer_addr)
            .local(local_addr)
            .endpoint_type(Type::Client)
            .socket_sender_count(self.store.socket_sender_count())
            .signer(peer.store.signer());
        if let Some(params) = local_params {
            local_builder = local_builder.params(params);
        }

        let mut peer_builder = Entry::builder(local_addr)
            .local(peer_addr)
            .endpoint_type(Type::Server)
            .socket_sender_count(peer.store.socket_sender_count())
            .signer(self.store.signer());
        if let Some(params) = peer_params {
            peer_builder = peer_builder.params(params);
        }

        // Bump generation until we find one that doesn't collide with existing entries.
        let mut generation = 0u64;
        let (local_entry, peer_entry) = loop {
            local_builder = local_builder.generation(generation);
            peer_builder = peer_builder.generation(generation);

            let local_entry = local_builder.build();
            let peer_entry = peer_builder.build();

            debug_assert_eq!(
                local_entry.secret().peer_id(),
                *peer_entry.id(),
                "local's peer_id must match peer's id"
            );
            debug_assert_eq!(
                peer_entry.secret().peer_id(),
                *local_entry.id(),
                "peer's peer_id must match local's id"
            );

            let local_exists = self.store.get_by_id_untracked(local_entry.id()).is_some();
            let peer_exists = peer.store.get_by_id_untracked(peer_entry.id()).is_some();

            if !local_exists && !peer_exists {
                break (local_entry, peer_entry);
            }

            generation = generation.wrapping_add(1);
        };

        let local_id = *local_entry.id();
        self.store.test_insert(local_entry);

        let peer_id = *peer_entry.id();
        peer.store.test_insert(peer_entry);

        TestPairIds {
            local: local_id,
            peer: peer_id,
        }
    }

    /// Called after successful decryption to record the key_id as seen and detect replays.
    ///
    /// This is equivalent to the per-stream `check_dedup` but used in the datagram recv path
    /// where decryption happens without going through a stream opener.
    pub fn check_dedup(
        &self,
        entry: &Arc<Entry>,
        credentials: &Credentials,
        queue_id: Option<VarInt>,
        control_out: &mut Vec<u8>,
    ) -> crypto::open::Result {
        self.store
            .check_dedup(entry, credentials.key_id, queue_id, control_out)
    }

    #[allow(clippy::type_complexity)]
    pub fn register_make_application_data(
        &self,
        cb: Box<
            dyn Fn(
                    &dyn s2n_quic_core::crypto::tls::TlsSession,
                ) -> Result<Option<ApplicationData>, ApplicationDataError>
                + Send
                + Sync,
        >,
    ) {
        self.store.register_make_application_data(cb);
    }
}

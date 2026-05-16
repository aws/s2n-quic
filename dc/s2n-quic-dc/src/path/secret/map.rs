// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    credentials::{Credentials, Id},
    crypto::awslc,
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
mod store;

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
        control_out: &mut Vec<u8>,
    ) -> Option<(awslc::open::Application, Arc<Entry>)> {
        let entry = self
            .store
            .pre_authentication(credentials, queue_id, control_out)?;
        let key_id = credentials.key_id;
        let opener = entry.secret().application_opener(key_id);
        Some((opener, entry))
    }

    pub fn open_once(
        &self,
        credentials: &Credentials,
        queue_id: Option<VarInt>,
        control_out: &mut Vec<u8>,
    ) -> Option<open::Once> {
        let entry = self
            .store
            .pre_authentication(credentials, queue_id, control_out)?;
        let opener = entry.uni_opener(self.clone(), credentials, queue_id);
        Some(opener)
    }

    pub fn pair_for_credentials(
        &self,
        credentials: &Credentials,
        queue_id: Option<VarInt>,
        control_out: &mut Vec<u8>,
    ) -> Option<(entry::Bidirectional, dc::ApplicationParams)> {
        let entry = self
            .store
            .pre_authentication(credentials, queue_id, control_out)?;

        let params = entry.parameters();
        let keys = entry.bidi_remote(self.clone(), credentials, queue_id);

        Some((keys, params))
    }

    pub fn secret_for_credentials(
        &self,
        credentials: &Credentials,
        queue_id: Option<VarInt>,
        control_out: &mut Vec<u8>,
    ) -> Option<(
        ExportSecret,
        Ciphersuite,
        entry::Bidirectional,
        dc::ApplicationParams,
    )> {
        let entry = self
            .store
            .pre_authentication(credentials, queue_id, control_out)?;
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

    pub fn sign_flow_reset_packet(
        &self,
        packet: &control::FlowReset,
        out: &mut [u8],
    ) -> Option<usize> {
        self.store.sign_flow_reset_packet(packet, out)
    }

    pub fn handle_flow_reset_packet<'a>(
        &self,
        packet: &'a control::flow_reset::Packet,
        peer: &SocketAddr,
    ) -> Option<&'a control::FlowReset> {
        self.store.handle_flow_reset_packet(packet, peer)
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
                dc::testing::TEST_REHANDSHAKE_PERIOD,
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

    #[doc(hidden)]
    #[cfg(any(test, feature = "testing"))]
    pub fn test_insert(&self, peer: SocketAddr) {
        let receiver = super::receiver::State::new();
        let entry = Entry::fake(peer, Some(receiver));
        self.store.test_insert(entry);
    }

    /// Insert a deterministic test entry for cross-process testing.
    ///
    /// Uses a fixed secret so that client and server processes can
    /// communicate with matching credentials.
    #[doc(hidden)]
    #[cfg(any(test, feature = "testing"))]
    pub fn test_insert_deterministic(
        &self,
        peer: SocketAddr,
        endpoint_type: s2n_quic_core::endpoint::Type,
    ) {
        use crate::path::secret::{schedule, sender};
        use s2n_quic_core::dc;

        // Use a fixed deterministic secret for demo purposes
        let secret = [42u8; 32];

        let secret = schedule::Secret::new(
            schedule::Ciphersuite::AES_GCM_128_SHA256,
            dc::SUPPORTED_VERSIONS[0],
            endpoint_type,
            &secret,
        );

        let id = *secret.id();
        let srt = self.store.signer().sign(&id);
        let sender = sender::State::new(srt);

        let entry = Entry::new(
            peer,
            secret,
            sender,
            super::receiver::State::new(),
            dc::testing::TEST_APPLICATION_PARAMS,
            dc::testing::TEST_REHANDSHAKE_PERIOD,
            None,
        );

        self.store.test_insert(Arc::new(entry));
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
        use crate::path::secret::{schedule, sender};
        use s2n_quic_core::endpoint::Type;

        let ciphersuite = schedule::Ciphersuite::AES_GCM_128_SHA256;

        let mut secret = [0; 32];
        aws_lc_rs::rand::fill(&mut secret).unwrap();

        let insert = |map: &Self,
                      peer: &Self,
                      peer_addr,
                      params: Option<dc::ApplicationParams>,
                      endpoint| {
            let secret =
                schedule::Secret::new(ciphersuite, dc::SUPPORTED_VERSIONS[0], endpoint, &secret);
            let id = *secret.id();

            let srt = peer.store.signer().sign(&id);

            let sender = sender::State::new(srt);

            let params = params.unwrap_or(dc::testing::TEST_APPLICATION_PARAMS);

            // Use the map's configured socket sender count so that entries
            // created in tests have the correct sender-slot allocation.
            let socket_sender_count = map.store.socket_sender_count();
            let entry = Entry::new_with_socket_senders(
                peer_addr,
                secret,
                sender,
                super::receiver::State::new(),
                params,
                dc::testing::TEST_REHANDSHAKE_PERIOD,
                None,
                socket_sender_count,
            );
            let entry = Arc::new(entry);
            map.store.test_insert(entry);

            id
        };

        let local = insert(self, peer, peer_addr, peer_params, Type::Client);
        let peer = insert(peer, self, local_addr, local_params, Type::Server);

        TestPairIds { local, peer }
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

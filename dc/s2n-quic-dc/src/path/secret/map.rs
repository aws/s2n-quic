// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    credentials::{Credentials, Id},
    event,
    packet::{secret_control as control, Packet},
    path::secret::{open, seal, stateless_reset},
    stream::TransportFeatures,
};
use s2n_quic_core::{dc, time};
use std::{net::SocketAddr, sync::Arc};

mod cleaner;
mod entry;
mod handshake;
mod peer;
mod size_of;
mod state;
mod status;
mod store;

#[cfg(any(test, feature = "testing"))]
pub mod testing;

#[cfg(test)]
mod event_tests;

use entry::Entry;
use store::Store;

pub use entry::{ApplicationPair, Bidirectional, ControlPair};
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

impl Map {
    pub fn new<C, S>(
        signer: stateless_reset::Signer,
        capacity: usize,
        clock: C,
        subscriber: S,
    ) -> Self
    where
        C: 'static + time::Clock + Send + Sync,
        S: event::Subscriber,
    {
        let store = state::State::new(signer, capacity, clock, subscriber);
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

    pub fn drop_state(&self) {
        self.store.drop_state();
    }

    pub fn contains(&self, peer: &SocketAddr) -> bool {
        self.store.contains(peer)
    }

    pub fn register_request_handshake(&self, cb: Box<dyn Fn(SocketAddr) + Send + Sync>) {
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

    pub fn open_once(
        &self,
        credentials: &Credentials,
        control_out: &mut Vec<u8>,
    ) -> Option<open::Once> {
        let entry = self.store.pre_authentication(credentials, control_out)?;
        let opener = entry.uni_opener(self.clone(), credentials);
        Some(opener)
    }

    pub fn pair_for_credentials(
        &self,
        credentials: &Credentials,
        features: &TransportFeatures,
        control_out: &mut Vec<u8>,
    ) -> Option<(entry::Bidirectional, dc::ApplicationParams)> {
        let entry = self.store.pre_authentication(credentials, control_out)?;

        let params = entry.parameters();
        let keys = entry.bidi_remote(self.clone(), credentials, features);

        Some((keys, params))
    }

    /// This can be called from anywhere to ask the map to handle a packet.
    ///
    /// For secret control packets, this will process those.
    /// For other packets, the map may collect metrics but will otherwise drop the packets.
    pub fn handle_unexpected_packet(&self, packet: &Packet, peer: &SocketAddr) {
        self.store.handle_unexpected_packet(packet, peer);
    }

    pub fn handle_control_packet(&self, packet: &control::Packet, peer: &SocketAddr) {
        self.store.handle_control_packet(packet, peer)
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
    #[cfg(any(test, feature = "testing"))]
    pub fn test_insert(&self, peer: SocketAddr) {
        let receiver = super::receiver::State::new();
        let entry = Entry::fake(peer, Some(receiver));
        self.store.test_insert(entry);
    }

    #[cfg(any(test, feature = "testing"))]
    pub(crate) fn test_insert_pair(
        &self,
        local_addr: SocketAddr,
        peer: &Self,
        peer_addr: SocketAddr,
    ) -> crate::credentials::Id {
        use crate::path::secret::{schedule, sender};
        use s2n_quic_core::endpoint::Type;

        let ciphersuite = schedule::Ciphersuite::AES_GCM_128_SHA256;

        let mut secret = [0; 32];
        aws_lc_rs::rand::fill(&mut secret).unwrap();

        let insert = |map: &Self, peer: &Self, peer_addr, endpoint| {
            let secret =
                schedule::Secret::new(ciphersuite, dc::SUPPORTED_VERSIONS[0], endpoint, &secret);
            let id = *secret.id();

            let srt = peer.store.signer().sign(&id);

            let sender = sender::State::new(srt);

            let entry = Entry::new(
                peer_addr,
                secret,
                sender,
                super::receiver::State::new(),
                dc::testing::TEST_APPLICATION_PARAMS,
                dc::testing::TEST_REHANDSHAKE_PERIOD,
            );
            let entry = Arc::new(entry);
            map.store.test_insert(entry);

            id
        };

        let client_id = insert(self, peer, peer_addr, Type::Client);
        let server_id = insert(peer, self, local_addr, Type::Server);

        assert_eq!(client_id, server_id);

        client_id
    }
}

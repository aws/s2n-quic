// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    credentials::{Credentials, Id},
    packet::{secret_control as control, Packet},
    path::secret::{open, seal, stateless_reset},
    stream::TransportFeatures,
};
use s2n_quic_core::dc;
use std::{net::SocketAddr, sync::Arc};

mod cleaner;
mod entry;
mod handshake;
mod size_of;
mod state;
mod status;
mod store;

use entry::Entry;
use store::Store;

pub use entry::{ApplicationPair, Bidirectional, ControlPair};

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
    pub fn new(signer: stateless_reset::Signer, capacity: usize) -> Self {
        // TODO add the subscriber
        let state = state::State::new(signer, capacity);
        Self { store: state }
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

    pub fn contains(&self, peer: SocketAddr) -> bool {
        self.store.contains(peer)
    }

    pub fn seal_once(
        &self,
        peer: SocketAddr,
    ) -> Option<(seal::Once, Credentials, dc::ApplicationParams)> {
        let entry = self.store.get_by_addr(&peer)?;
        let (sealer, credentials) = entry.uni_sealer();
        Some((sealer, credentials, entry.parameters()))
    }

    /// Retrieve a sealer by path secret ID.
    ///
    /// Generally callers should prefer to use one of the `pair` APIs; this is primarily useful for
    /// "response" datagrams which want to be bound to the exact same shared secret.
    ///
    /// Note that unlike by-IP lookup this should typically not be done significantly after the
    /// original secret was used for decryption.
    pub fn seal_once_id(&self, id: Id) -> Option<(seal::Once, Credentials, dc::ApplicationParams)> {
        let entry = self.store.get_by_id(&id)?;
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

    pub fn pair_for_peer(
        &self,
        peer: SocketAddr,
        features: &TransportFeatures,
    ) -> Option<(entry::Bidirectional, dc::ApplicationParams)> {
        let entry = self.store.get_by_addr(&peer)?;
        let keys = entry.bidi_local(features);

        Some((keys, entry.parameters()))
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
    pub fn handle_unexpected_packet(&self, packet: &Packet) {
        self.store.handle_unexpected_packet(packet);
    }

    pub fn handle_control_packet(&self, packet: &control::Packet) {
        self.store.handle_control_packet(packet)
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

        let provider = Self::new(stateless_reset::Signer::random(), peers.len() * 3);
        let mut secret = [0; 32];
        aws_lc_rs::rand::fill(&mut secret).unwrap();
        let mut stateless_reset = [0; control::TAG_LEN];
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
                dc::testing::TEST_REHANDSHAKE_PERIOD,
            );
            let entry = Arc::new(entry);
            provider.store.test_insert(entry);
        }

        (provider, ids)
    }

    #[doc(hidden)]
    #[cfg(any(test, feature = "testing"))]
    pub fn test_insert(&self, peer: SocketAddr) {
        let receiver = self.store.receiver().clone().new_receiver();
        let entry = Entry::fake(peer, Some(receiver));
        self.store.test_insert(entry);
    }
}

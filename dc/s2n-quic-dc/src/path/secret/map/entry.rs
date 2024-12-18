// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{
    size_of::SizeOf,
    status::{Dedup, IsRetired},
    Map,
};
use crate::{
    credentials::{self, Credentials},
    packet::{secret_control as control, WireVersion},
    path::secret::{
        open, receiver,
        schedule::{self, Initiator},
        seal, sender,
    },
    stream::TransportFeatures,
};
use rand::Rng as _;
use s2n_codec::EncoderBuffer;
use s2n_quic_core::{dc, varint::VarInt};
use std::{
    net::SocketAddr,
    sync::{atomic::Ordering, Arc},
    time::{Duration, Instant},
};

#[cfg(test)]
mod tests;

#[derive(Debug)]
pub(super) struct Entry {
    creation_time: Instant,
    rehandshake_delta_secs: u32,
    peer: SocketAddr,
    secret: schedule::Secret,
    retired: IsRetired,
    sender: sender::State,
    receiver: receiver::State,
    parameters: dc::ApplicationParams,
}

impl SizeOf for Entry {
    fn size(&self) -> usize {
        let Entry {
            creation_time,
            rehandshake_delta_secs,
            peer,
            secret,
            retired,
            sender,
            receiver,
            parameters,
        } = self;
        creation_time.size()
            + rehandshake_delta_secs.size()
            + peer.size()
            + secret.size()
            + retired.size()
            + sender.size()
            + receiver.size()
            + parameters.size()
    }
}

impl Entry {
    pub fn new(
        peer: SocketAddr,
        secret: schedule::Secret,
        sender: sender::State,
        receiver: receiver::State,
        parameters: dc::ApplicationParams,
        rehandshake_time: Duration,
    ) -> Self {
        // clamp max datagram size to a well-known value
        parameters
            .max_datagram_size
            .fetch_min(crate::stream::MAX_DATAGRAM_SIZE as _, Ordering::Relaxed);

        assert!(rehandshake_time.as_secs() <= u32::MAX as u64);
        Self {
            creation_time: Instant::now(),
            // Schedule another handshake sometime in [5 minutes, rehandshake_time] from now.
            rehandshake_delta_secs: rand::thread_rng().gen_range(
                std::cmp::min(rehandshake_time.as_secs(), 360)..rehandshake_time.as_secs(),
            ) as u32,
            peer,
            secret,
            retired: Default::default(),
            sender,
            receiver,
            parameters,
        }
    }

    #[cfg(any(test, feature = "testing"))]
    pub fn fake(peer: SocketAddr, receiver: Option<receiver::State>) -> Arc<Entry> {
        let receiver = receiver.unwrap_or_else(receiver::State::without_shared);

        let mut secret = [0; 32];
        aws_lc_rs::rand::fill(&mut secret).unwrap();

        Arc::new(Entry::new(
            peer,
            schedule::Secret::new(
                schedule::Ciphersuite::AES_GCM_128_SHA256,
                dc::SUPPORTED_VERSIONS[0],
                s2n_quic_core::endpoint::Type::Client,
                &secret,
            ),
            sender::State::new([0; control::TAG_LEN]),
            receiver,
            dc::testing::TEST_APPLICATION_PARAMS,
            dc::testing::TEST_REHANDSHAKE_PERIOD,
        ))
    }

    pub fn peer(&self) -> &SocketAddr {
        &self.peer
    }

    pub fn id(&self) -> &credentials::Id {
        self.secret.id()
    }

    pub fn retire(&self, at_epoch: u64) {
        self.retired.retire(at_epoch);
    }

    pub fn retired_at(&self) -> Option<u64> {
        self.retired.retired_at()
    }

    pub fn uni_sealer(&self) -> (seal::Once, Credentials) {
        let key_id = self.sender.next_key_id();
        let credentials = Credentials {
            id: *self.secret.id(),
            key_id,
        };
        let sealer = self.secret.application_sealer(key_id);
        let sealer = seal::Once::new(sealer);

        (sealer, credentials)
    }

    pub fn uni_opener(self: Arc<Self>, map: Map, credentials: &Credentials) -> open::Once {
        let key_id = credentials.key_id;
        let opener = self.secret.application_opener(key_id);
        let dedup = Dedup::new(self, key_id, map);
        open::Once::new(opener, dedup)
    }

    pub fn bidi_local(&self, features: &TransportFeatures) -> Bidirectional {
        let key_id = self.sender.next_key_id();
        let initiator = Initiator::Local;

        let application = ApplicationPair::new(
            &self.secret,
            key_id,
            initiator,
            // we don't need to dedup locally-initiated openers
            Dedup::disabled(),
        );

        let control = if features.is_reliable() {
            None
        } else {
            Some(ControlPair::new(&self.secret, key_id, initiator))
        };

        Bidirectional {
            credentials: Credentials {
                id: *self.secret.id(),
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
        features: &TransportFeatures,
    ) -> Bidirectional {
        let key_id = credentials.key_id;
        let initiator = Initiator::Remote;

        let application = ApplicationPair::new(
            &self.secret,
            key_id,
            initiator,
            // Remote application keys need to be de-duplicated
            Dedup::new(self.clone(), key_id, map),
        );

        let control = if features.is_reliable() {
            None
        } else {
            Some(ControlPair::new(&self.secret, key_id, initiator))
        };

        Bidirectional {
            credentials: *credentials,
            application,
            control,
        }
    }

    pub fn parameters(&self) -> dc::ApplicationParams {
        self.parameters.clone()
    }

    pub fn update_max_datagram_size(&self, mtu: u16) {
        self.parameters
            .max_datagram_size
            .store(mtu, Ordering::Relaxed);
    }

    pub fn rehandshake_time(&self) -> Instant {
        self.creation_time + Duration::from_secs(u64::from(self.rehandshake_delta_secs))
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
                wire_version: WireVersion::ZERO,
                credential_id: credentials.id,
                rejected_key_id: credentials.key_id,
            }
            .encode(encoder, &entry.control_sealer()),
            receiver::Error::Unknown => control::StaleKey {
                wire_version: WireVersion::ZERO,
                credential_id: credentials.id,
                min_key_id: entry.receiver.minimum_unseen_key_id(),
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
    fn new(secret: &schedule::Secret, key_id: VarInt, initiator: Initiator, dedup: Dedup) -> Self {
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

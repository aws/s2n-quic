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
use s2n_codec::EncoderBuffer;
use s2n_quic_core::{dc, varint::VarInt};
use std::{
    any::Any,
    net::SocketAddr,
    sync::{
        atomic::{AtomicU32, AtomicU8, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

#[cfg(test)]
mod tests;

pub type ApplicationData = Arc<dyn Any + Send + Sync>;

#[derive(Debug)]
pub struct Entry {
    creation_time: Instant,
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
impl SizeOf for AtomicU32 {}

impl Entry {
    pub fn new(
        peer: SocketAddr,
        secret: schedule::Secret,
        sender: sender::State,
        receiver: receiver::State,
        parameters: dc::ApplicationParams,
        // FIXME: remove unused parameter
        _: Duration,
        application_data: Option<ApplicationData>,
    ) -> Self {
        // clamp max datagram size to a well-known value
        parameters
            .max_datagram_size
            .fetch_min(crate::stream::MAX_DATAGRAM_SIZE as _, Ordering::Relaxed);

        Self {
            creation_time: Instant::now(),
            peer,
            secret,
            retired: Default::default(),
            sender,
            receiver,
            parameters,
            accessed: AtomicU8::new(0),
            application_data,
        }
    }

    #[cfg(any(test, feature = "testing"))]
    pub fn fake(peer: SocketAddr, receiver: Option<receiver::State>) -> Arc<Entry> {
        let receiver = receiver.unwrap_or_default();

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
            None,
        ))
    }

    pub fn peer(&self) -> &SocketAddr {
        &self.peer
    }

    pub fn id(&self) -> &credentials::Id {
        self.secret.id()
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

    pub fn age(&self) -> Duration {
        self.creation_time.elapsed()
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

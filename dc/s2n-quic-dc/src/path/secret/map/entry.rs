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
use s2n_quic_core::{
    dc,
    recovery::bandwidth::Bandwidth,
    time::Timestamp,
    varint::VarInt,
};
use std::{
    any::Any,
    net::SocketAddr,
    sync::{
        atomic::{AtomicU16, AtomicU32, AtomicU64, AtomicU8, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

#[cfg(test)]
mod tests;

pub type ApplicationData = Arc<dyn Any + Send + Sync>;

#[derive(Debug, thiserror::Error)]
#[error("{inner}")]
pub struct ApplicationDataError {
    pub msg: &'static str,
    #[source]
    pub inner: Box<dyn std::error::Error + Send + Sync>,
}

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
    /// Stores the next allowed connection time as microseconds from the epoch of the
    /// `s2n_quic_core::time::Timestamp` clock. A value of `0` means no rate limiting is applied.
    /// Callers should atomically claim a slot by advancing this value forward.
    next_connection: AtomicU64,
    /// The peer's data port, exchanged after the handshake completes.
    /// 0 means not yet learned.
    peer_data_port: AtomicU16,
    /// Next scheduled transmission timestamp per socket sender, encoded as microseconds.
    next_transmission_by_sender: Box<[AtomicU64]>,
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
            next_connection,
            peer_data_port,
            next_transmission_by_sender,
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
            + next_connection.size()
            + peer_data_port.size()
            + std::mem::size_of::<Box<[AtomicU64]>>()
            + next_transmission_by_sender.len() * std::mem::size_of::<AtomicU64>()
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
impl SizeOf for AtomicU16 {}
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
        Self::new_with_socket_senders(
            peer,
            secret,
            sender,
            receiver,
            parameters,
            Duration::ZERO,
            application_data,
            0,
        )
    }

    pub fn new_with_socket_senders(
        peer: SocketAddr,
        secret: schedule::Secret,
        sender: sender::State,
        receiver: receiver::State,
        parameters: dc::ApplicationParams,
        // FIXME: remove unused parameter
        _: Duration,
        application_data: Option<ApplicationData>,
        socket_sender_count: usize,
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
            next_connection: AtomicU64::new(0),
            peer_data_port: AtomicU16::new(0),
            next_transmission_by_sender: Self::init_sender_schedule(socket_sender_count),
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

    /// Like [`fake`] but pre-allocates `socket_sender_count` sender slots so
    /// `update_sender_next_transmission_time` / `pick_sender_by_next_transmission`
    /// can be exercised in unit tests.
    #[cfg(any(test, feature = "testing"))]
    pub fn fake_with_socket_senders(
        peer: SocketAddr,
        receiver: Option<receiver::State>,
        socket_sender_count: usize,
    ) -> Arc<Entry> {
        let receiver = receiver.unwrap_or_default();

        let mut secret = [0; 32];
        aws_lc_rs::rand::fill(&mut secret).unwrap();

        Arc::new(Entry::new_with_socket_senders(
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
            socket_sender_count,
        ))
    }

    /// Create a deterministic entry for cross-process testing.
    ///
    /// Uses a fixed secret so client and server can communicate.
    #[cfg(any(test, feature = "testing"))]
    pub fn fake_deterministic(
        peer: SocketAddr,
        endpoint_type: s2n_quic_core::endpoint::Type,
    ) -> Arc<Entry> {
        let secret = [42; 32];

        Arc::new(Entry::new(
            peer,
            schedule::Secret::new(
                schedule::Ciphersuite::AES_GCM_128_SHA256,
                dc::SUPPORTED_VERSIONS[0],
                endpoint_type,
                &secret,
            ),
            sender::State::new([0; control::TAG_LEN]),
            receiver::State::new(),
            dc::testing::TEST_APPLICATION_PARAMS,
            dc::testing::TEST_REHANDSHAKE_PERIOD,
            None,
        ))
    }

    pub fn peer(&self) -> &SocketAddr {
        &self.peer
    }

    /// Returns the data endpoint address for this peer.
    ///
    /// The port is learned via a post-handshake exchange. Returns the peer's
    /// handshake address if the data port hasn't been set yet.
    pub fn data_addr(&self) -> SocketAddr {
        let mut addr = self.peer;
        let port = self.peer_data_port.load(Ordering::Relaxed);
        if port != 0 {
            addr.set_port(port);
        }
        addr
    }

    /// Returns true if the peer's data port has been learned via the post-handshake exchange.
    #[inline]
    pub fn has_data_port(&self) -> bool {
        self.peer_data_port.load(Ordering::Relaxed) != 0
    }

    /// Set the peer's data port, learned from the post-handshake port exchange.
    pub fn set_peer_data_port(&self, port: u16) {
        self.peer_data_port.store(port, Ordering::Relaxed);
    }

    fn sender_index(&self, sender_idx: usize) -> Option<usize> {
        let len = self.next_transmission_by_sender.len();
        if len == 0 {
            None
        } else {
            Some(sender_idx % len)
        }
    }

    fn sender_schedule_micros(ts: Timestamp) -> u64 {
        // SAFETY: `Timestamp` values in this crate are monotonic and treated as non-negative.
        let micros = unsafe { ts.as_duration().as_micros() };
        micros.min(u64::MAX as u128) as u64
    }

    fn init_sender_schedule(socket_sender_count: usize) -> Box<[AtomicU64]> {
        (0..socket_sender_count)
            .map(|_| AtomicU64::new(0))
            .collect::<Vec<_>>()
            .into_boxed_slice()
    }

    fn transmission_delay(queued_bytes: usize, bandwidth: Bandwidth) -> Duration {
        if queued_bytes == 0 {
            return Duration::ZERO;
        }

        let queued_bytes = u64::try_from(queued_bytes).unwrap_or(u64::MAX);
        <u64 as core::ops::Div<Bandwidth>>::div(queued_bytes, bandwidth)
    }

    #[inline]
    pub fn socket_sender_count(&self) -> usize {
        self.next_transmission_by_sender.len()
    }

    #[inline]
    pub fn sender_next_transmission_micros(&self, sender_idx: usize) -> u64 {
        let Some(sender_idx) = self.sender_index(sender_idx) else {
            return 0;
        };
        self.next_transmission_by_sender[sender_idx].load(Ordering::Acquire)
    }

    pub fn update_sender_next_transmission_time(
        &self,
        sender_idx: usize,
        now: Timestamp,
        queued_bytes: usize,
        bandwidth: Bandwidth,
    ) -> Timestamp {
        let Some(sender_idx) = self.sender_index(sender_idx) else {
            return now;
        };
        let delay = Self::transmission_delay(queued_bytes, bandwidth);
        let next = now + delay;
        let next_micros = Self::sender_schedule_micros(next);
        self.next_transmission_by_sender[sender_idx].store(next_micros, Ordering::Release);
        next
    }

    pub fn pick_sender_by_next_transmission(
        &self,
        random_fn: impl Fn(usize) -> usize,
    ) -> usize {
        let len = self.next_transmission_by_sender.len();
        if len == 0 {
            // No sender sockets are configured yet; callers treat 0 as the default route index.
            return 0;
        }

        if len == 1 {
            return 0;
        }

        let idx1 = random_fn(len) % len;
        let idx2 = if len == 2 {
            idx1 ^ 1
        } else {
            let mut idx2 = random_fn(len - 1) % (len - 1);
            if idx2 >= idx1 {
                idx2 += 1;
            }
            idx2
        };

        let time1 = self.next_transmission_by_sender[idx1].load(Ordering::Acquire);
        let time2 = self.next_transmission_by_sender[idx2].load(Ordering::Acquire);

        if time1 <= time2 {
            idx1
        } else {
            idx2
        }
    }

    pub fn idle_timeout(&self) -> Duration {
        self.parameters
            .max_idle_timeout
            .map_or(Duration::from_secs(60), |v| {
                Duration::from_millis(v.get() as u64)
            })
    }

    pub fn id(&self) -> &credentials::Id {
        self.secret.id()
    }

    pub fn secret(&self) -> &schedule::Secret {
        &self.secret
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

    /// Atomically claims the next connection slot for rate limiting.
    ///
    /// Given the current timestamp (`now`), this method returns the `Timestamp`
    /// at which the caller should start sending.
    ///
    /// The returned value may be in the past (if no rate limiting is needed) or in the
    /// future (if the caller should delay sending). This value can be used to initialize
    /// the transmission wheel's start time.
    pub fn next_connection_time(
        &self,
        now: s2n_quic_core::time::Timestamp,
    ) -> s2n_quic_core::time::Timestamp {
        let now_micros = unsafe { now.as_duration().as_micros() as u64 };

        let prev = self
            .next_connection
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                // The next allowed time is at least `now` and at least `current`
                let next = current.max(now_micros).saturating_add(1);
                Some(next)
            })
            // fetch_update with a closure that always returns Some never fails
            .unwrap();

        // The start time for this connection is max(prev, now), since prev is the
        // time the previous connection was scheduled to start (before we added delay)
        let start_micros = prev.max(now_micros);
        unsafe {
            s2n_quic_core::time::Timestamp::from_duration(Duration::from_micros(start_micros))
        }
    }

    /// Returns the raw next_connection value in microseconds for inspection/testing.
    pub fn next_connection_micros(&self) -> u64 {
        self.next_connection.load(Ordering::Acquire)
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

    pub fn reusable_sealer(&self) -> (crate::crypto::awslc::seal::Application, Credentials) {
        let key_id = self.sender.next_key_id();
        let credentials = Credentials {
            id: *self.secret.id(),
            key_id,
        };
        let sealer = self.secret.application_sealer(key_id);

        (sealer, credentials)
    }

    pub fn uni_opener(
        self: Arc<Self>,
        map: Map,
        credentials: &Credentials,
        queue_id: Option<VarInt>,
    ) -> open::Once {
        let key_id = credentials.key_id;
        let opener = self.secret.application_opener(key_id);
        let dedup = Dedup::new(self, key_id, queue_id, map);
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
        queue_id: Option<VarInt>,
        features: &TransportFeatures,
    ) -> Bidirectional {
        let key_id = credentials.key_id;
        let initiator = Initiator::Remote;

        let application = ApplicationPair::new(
            &self.secret,
            key_id,
            initiator,
            // Remote application keys need to be de-duplicated
            Dedup::new(self.clone(), key_id, queue_id, map),
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

    pub fn max_datagram_size(&self) -> u16 {
        self.parameters.max_datagram_size.load(Ordering::Relaxed)
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

    #[cfg(test)]
    pub fn reset_sender_counter(&self) {
        self.sender.reset_counter();
    }
}

impl receiver::Error {
    pub(super) fn to_packet<'buffer>(
        self,
        entry: &Entry,
        credentials: &Credentials,
        queue_id: Option<VarInt>,
        buffer: &'buffer mut [u8; control::MAX_PACKET_SIZE],
    ) -> &'buffer [u8] {
        debug_assert_eq!(entry.secret.id(), &credentials.id);
        let encoder = EncoderBuffer::new(&mut buffer[..]);
        let length = match self {
            receiver::Error::AlreadyExists => control::ReplayDetected {
                wire_version: WireVersion::ZERO,
                credential_id: credentials.id,
                rejected_key_id: credentials.key_id,
                queue_id,
            }
            .encode(encoder, &entry.control_sealer()),
            receiver::Error::Unknown => control::StaleKey {
                wire_version: WireVersion::ZERO,
                credential_id: credentials.id,
                min_key_id: entry.receiver.minimum_unseen_key_id(),
                queue_id,
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
    pub fn new(
        secret: &schedule::Secret,
        key_id: VarInt,
        initiator: Initiator,
        dedup: Dedup,
    ) -> Self {
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

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! The blob is opaque to the embedding application, which frames, checksums and files it without
//! inspecting its contents. Keeping the codec here, next to the `Entry` fields it serialises, is
//! what makes a change to any of those fields a conscious new version.
//!
//! The blob records values *as observed*; the security-critical transforms happen on load, so the
//! file records the truth and the advance can be tuned without a format change. Application data
//! is deliberately absent -- it is an opaque `Arc<dyn Any>` the map cannot encode, so the
//! application persists it and hands the resolved value back alongside the blob.

use crate::{
    credentials,
    path::secret::{receiver, schedule, sender},
};
use s2n_quic_core::dc;
use std::{
    fmt,
    net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6},
    ops::Deref,
    time::Instant,
};
use zeroize::{Zeroize, Zeroizing};

use super::{entry::ApplicationData, Entry};

/// The layout version this build writes. Bumped whenever the field region changes at all.
const VERSION: u8 = 1;

/// The oldest reader version this build's blobs may be read by, and the mechanism behind the
/// rollback guarantee: a team may deploy writer `N+1` and, if it goes wrong, roll back to reader
/// `N` without losing their persisted keys.
/// This is a value written into files by this binary.
const MIN_READER_VERSION: u8 = 1;

/// The highest layout this build can parse in full. A blob whose `min_reader_version` exceeds this
/// is rejected.
/// This is a property of this binary.
const THIS_READER_VERSION: u8 = VERSION;

/// `endpoint::Type` has no stable integer representation of its own, so the mapping is pinned here
/// rather than left to the enum's declaration order. Changing these values is a format change.
mod endpoint_byte {
    use s2n_quic_core::endpoint;

    pub(super) const CLIENT: u8 = 0;
    pub(super) const SERVER: u8 = 1;

    pub(super) fn encode(endpoint: endpoint::Type) -> u8 {
        match endpoint {
            endpoint::Type::Client => CLIENT,
            endpoint::Type::Server => SERVER,
        }
    }

    pub(super) fn decode(byte: u8) -> Option<endpoint::Type> {
        match byte {
            CLIENT => Some(endpoint::Type::Client),
            SERVER => Some(endpoint::Type::Server),
            _ => None,
        }
    }
}

/// The least distance a restored entry's key-id counters are moved past the values in the file.
const MIN_RESTORE_ADVANCE: u64 = 5 * 24 * 60 * 60 * 1_000_000;

#[derive(Clone, Copy, Debug, Default)]
pub struct RestoreParams {
    pub advance: u64,
}

#[derive(Clone)]
pub struct PersistedReport {
    pub id: credentials::Id,
    pub peer: SocketAddr,
    pub sender_current_id: u64,
    pub receiver_max_seen_key_id: Option<u64>,
    pub application_data: Option<ApplicationData>,
}

#[repr(transparent)]
#[derive(Clone)]
struct SecretBytes<const N: usize>([u8; N]);

impl<const N: usize> Drop for SecretBytes<N> {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl<const N: usize> fmt::Debug for SecretBytes<N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SecretBytes<{N}>(<redacted>)")
    }
}

impl<const N: usize> Deref for SecretBytes<N> {
    type Target = [u8; N];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<const N: usize> AsRef<[u8]> for SecretBytes<N> {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl<const N: usize> From<[u8; N]> for SecretBytes<N> {
    fn from(bytes: [u8; N]) -> Self {
        Self(bytes)
    }
}

#[derive(Clone, Debug)]
struct PersistedStateV1 {
    peer: SocketAddr,
    export_secret: SecretBytes<32>,
    endpoint: u8,
    ciphersuite: u8,
    stateless_reset: [u8; 16],
    created_at_unix_millis: u64,
    sender_current_id: u64,
    receiver_max_seen_key_id: Option<u64>,
    remote_max_data: u64,
    max_datagram_size: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RestoreDisposition {
    SkipEntry,
    StopFile,
    RejectFile,
}

#[derive(Debug, thiserror::Error)]
pub enum InsertPersistedError {
    #[error("persisted entry blob was truncated or malformed")]
    Malformed,

    #[error("persisted entry blob failed its checksum")]
    BadChecksum,

    #[error(
        "persisted entry needs reader version {min_reader_version}, this build is {this_reader}"
    )]
    RequiresNewerReader {
        min_reader_version: u8,
        this_reader: u8,
    },

    #[error("invalid peer address tag {0}")]
    InvalidPeerTag(u8),

    #[error("invalid endpoint byte {0}")]
    InvalidEndpoint(u8),

    #[error("invalid ciphersuite byte {0}")]
    InvalidCiphersuite(u8),

    #[error("persisted remote_max_data {0} exceeds the VarInt range")]
    InvalidRemoteMaxData(u64),

    #[error("receiver key id was the reserved sentinel value")]
    ReceiverSentinel,

    #[error("restored counter would overflow the usable id space")]
    CounterOverflow,

    #[error("an entry with this credential id already exists")]
    DuplicateCredentialId,
}

impl InsertPersistedError {
    /// The match below is exhaustive with no wildcard on purpose: a new variant will not compile
    /// until it is classified here, so the disposition can never be left to a default.
    pub fn disposition(&self) -> RestoreDisposition {
        use RestoreDisposition::*;
        match self {
            Self::CounterOverflow
            | Self::ReceiverSentinel
            | Self::InvalidEndpoint(_)
            | Self::InvalidCiphersuite(_)
            | Self::InvalidPeerTag(_)
            | Self::InvalidRemoteMaxData(_)
            | Self::DuplicateCredentialId => SkipEntry,

            Self::Malformed | Self::BadChecksum => StopFile,

            Self::RequiresNewerReader { .. } => RejectFile,
        }
    }
}

impl From<sender::RestoreError> for InsertPersistedError {
    fn from(error: sender::RestoreError) -> Self {
        match error {
            sender::RestoreError::CounterOverflow => Self::CounterOverflow,
        }
    }
}

impl From<receiver::RestoreError> for InsertPersistedError {
    fn from(error: receiver::RestoreError) -> Self {
        match error {
            receiver::RestoreError::Sentinel => Self::ReceiverSentinel,
            receiver::RestoreError::Overflow => Self::CounterOverflow,
        }
    }
}

impl Entry {
    pub fn to_persisted_bytes(&self) -> Zeroizing<Vec<u8>> {
        let secret = self.secret();

        let state = PersistedStateV1 {
            peer: *self.peer(),
            export_secret: (*secret.export_secret()).into(),
            endpoint: endpoint_byte::encode(secret.endpoint()),
            ciphersuite: u8::from(*secret.ciphersuite()),
            stateless_reset: self.sender().stateless_reset,
            created_at_unix_millis: creation_time_to_unix_millis(self.creation_time()),
            sender_current_id: self.sender().current_id(),
            receiver_max_seen_key_id: self.receiver().max_seen_key_id(),
            remote_max_data: *self.parameters().remote_max_data,
            max_datagram_size: self.parameters().max_datagram_size(),
        };

        Zeroizing::new(encode(&state))
    }

    pub fn restore(
        bytes: &[u8],
        application_data: Option<ApplicationData>,
        params: &RestoreParams,
    ) -> Result<Self, InsertPersistedError> {
        Self::restore_with_advance(
            bytes,
            application_data,
            params.advance.max(MIN_RESTORE_ADVANCE),
        )
    }

    pub fn inspect_persisted_bytes(
        bytes: &[u8],
        application_data: Option<ApplicationData>,
    ) -> Result<PersistedReport, InsertPersistedError> {
        let entry = Self::restore_with_advance(bytes, application_data, 0)?;

        Ok(PersistedReport {
            id: *entry.id(),
            peer: *entry.peer(),
            sender_current_id: entry.sender().current_id(),
            receiver_max_seen_key_id: entry.receiver().max_seen_key_id(),
            application_data: entry.application_data().clone(),
        })
    }

    fn restore_with_advance(
        bytes: &[u8],
        application_data: Option<ApplicationData>,
        advance: u64,
    ) -> Result<Self, InsertPersistedError> {
        let state = decode(bytes)?;

        let endpoint = endpoint_byte::decode(state.endpoint)
            .ok_or(InsertPersistedError::InvalidEndpoint(state.endpoint))?;
        let ciphersuite = schedule::Ciphersuite::try_from(state.ciphersuite)
            .map_err(|_| InsertPersistedError::InvalidCiphersuite(state.ciphersuite))?;

        let secret = schedule::Secret::new(
            ciphersuite,
            dc::SUPPORTED_VERSIONS[0],
            endpoint,
            &state.export_secret,
        );

        let sender =
            sender::State::restore(state.sender_current_id, state.stateless_reset, advance)?;

        let receiver = match state.receiver_max_seen_key_id {
            Some(max_seen) => receiver::State::restore(max_seen, advance)?,
            None => receiver::State::restore_unseen(advance)?,
        };

        let mut parameters = dc::ApplicationParams::new(
            state.max_datagram_size,
            &Default::default(),
            &Default::default(),
        );
        parameters.remote_max_data = s2n_quic_core::varint::VarInt::new(state.remote_max_data)
            .map_err(|_| InsertPersistedError::InvalidRemoteMaxData(state.remote_max_data))?;

        let creation_time = unix_millis_to_creation_time(state.created_at_unix_millis);

        Ok(Entry::from_restored_parts(
            state.peer,
            secret,
            sender,
            receiver,
            parameters,
            creation_time,
            application_data,
        ))
    }
}

// --- blob codec -------------------------------------------------------------------------------

/// Mirrors the existing address-only serializer so the two agree.
mod peer_tag {
    pub(super) const V4: u8 = 0;
    pub(super) const V6_MINIMAL: u8 = 1;
    pub(super) const V6_FULL: u8 = 2;
}

/// The `V1` layout. All integers little-endian.
///
/// ```text
/// version                u8    layout the writer used
/// min_reader_version     u8    oldest reader allowed to read this blob
/// field_region_len       u16   byte length of the field region that follows
/// --- field region (exactly field_region_len bytes) ---
/// peer                   tag u8 + address (see peer_tag)
/// export_secret          [u8; 32]
/// endpoint               u8
/// ciphersuite            u8
/// stateless_reset        [u8; 16]
/// created_at_unix_millis u64
/// sender_current_id      u64
/// receiver_present       u8    (0 = None, 1 = Some)
/// receiver_max_seen      u64   (present only when receiver_present == 1)
/// remote_max_data        u64
/// max_datagram_size      u16
/// --- end field region ---
/// crc32                  u32   (IEEE, over every preceding byte)
/// ```
///
/// A future version may only *append* fields to the region; see [`MIN_READER_VERSION`].
fn encode(state: &PersistedStateV1) -> Vec<u8> {
    const MAX_FIELD_REGION_LEN: usize = 112;
    let mut fields = Zeroizing::new(Vec::with_capacity(MAX_FIELD_REGION_LEN));
    match state.peer {
        SocketAddr::V4(addr) => {
            fields.push(peer_tag::V4);
            fields.extend_from_slice(&addr.ip().octets());
            fields.extend_from_slice(&addr.port().to_le_bytes());
        }
        SocketAddr::V6(addr) => {
            let minimal = addr.flowinfo() == 0 && addr.scope_id() == 0;
            fields.push(if minimal {
                peer_tag::V6_MINIMAL
            } else {
                peer_tag::V6_FULL
            });
            fields.extend_from_slice(&addr.ip().octets());
            fields.extend_from_slice(&addr.port().to_le_bytes());
            if !minimal {
                fields.extend_from_slice(&addr.flowinfo().to_le_bytes());
                fields.extend_from_slice(&addr.scope_id().to_le_bytes());
            }
        }
    }

    fields.extend_from_slice(state.export_secret.as_ref());
    fields.push(state.endpoint);
    fields.push(state.ciphersuite);
    fields.extend_from_slice(&state.stateless_reset);
    fields.extend_from_slice(&state.created_at_unix_millis.to_le_bytes());
    fields.extend_from_slice(&state.sender_current_id.to_le_bytes());
    match state.receiver_max_seen_key_id {
        Some(max) => {
            fields.push(1);
            fields.extend_from_slice(&max.to_le_bytes());
        }
        None => fields.push(0),
    }
    fields.extend_from_slice(&state.remote_max_data.to_le_bytes());
    fields.extend_from_slice(&state.max_datagram_size.to_le_bytes());

    debug_assert_eq!(
        fields.capacity(),
        MAX_FIELD_REGION_LEN,
        "encode reallocated: MAX_FIELD_REGION_LEN is below the largest shape"
    );
    debug_assert!(fields.len() <= u16::MAX as usize);
    let field_region_len = fields.len() as u16;

    let mut out = Vec::with_capacity(4 + fields.len() + 4);
    out.push(VERSION);
    out.push(MIN_READER_VERSION);
    out.extend_from_slice(&field_region_len.to_le_bytes());
    out.extend_from_slice(&fields);

    out.extend_from_slice(&crc32(&out).to_le_bytes());

    out
}

fn decode(bytes: &[u8]) -> Result<PersistedStateV1, InsertPersistedError> {
    let (payload, expected) = bytes
        .split_at_checked(
            bytes
                .len()
                .checked_sub(4)
                .ok_or(InsertPersistedError::Malformed)?,
        )
        .ok_or(InsertPersistedError::Malformed)?;
    let expected = u32::from_le_bytes(
        expected
            .try_into()
            .map_err(|_| InsertPersistedError::Malformed)?,
    );
    if crc32(payload) != expected {
        return Err(InsertPersistedError::BadChecksum);
    }

    let mut r = Cursor::new(payload);

    let _version = r.u8()?;
    let min_reader_version = r.u8()?;
    if min_reader_version > THIS_READER_VERSION {
        return Err(InsertPersistedError::RequiresNewerReader {
            min_reader_version,
            this_reader: THIS_READER_VERSION,
        });
    }

    let field_region_len = u16::from_le_bytes(r.array::<2>()?) as usize;
    let region = r.take(field_region_len)?;
    if !r.is_empty() {
        return Err(InsertPersistedError::Malformed);
    }
    let mut r = Cursor::new(region);

    let peer = match r.u8()? {
        peer_tag::V4 => {
            let ip = Ipv4Addr::from(r.array::<4>()?);
            let port = u16::from_le_bytes(r.array::<2>()?);
            SocketAddr::V4(SocketAddrV4::new(ip, port))
        }
        tag @ (peer_tag::V6_MINIMAL | peer_tag::V6_FULL) => {
            let ip = Ipv6Addr::from(r.array::<16>()?);
            let port = u16::from_le_bytes(r.array::<2>()?);
            let (flowinfo, scope_id) = if tag == peer_tag::V6_FULL {
                (
                    u32::from_le_bytes(r.array::<4>()?),
                    u32::from_le_bytes(r.array::<4>()?),
                )
            } else {
                (0, 0)
            };
            SocketAddr::V6(SocketAddrV6::new(ip, port, flowinfo, scope_id))
        }
        other => return Err(InsertPersistedError::InvalidPeerTag(other)),
    };

    let export_secret = r.array::<32>()?.into();
    let endpoint = r.u8()?;
    let ciphersuite = r.u8()?;
    let stateless_reset = r.array::<16>()?;
    let created_at_unix_millis = u64::from_le_bytes(r.array::<8>()?);
    let sender_current_id = u64::from_le_bytes(r.array::<8>()?);
    let receiver_max_seen_key_id = match r.u8()? {
        0 => None,
        1 => Some(u64::from_le_bytes(r.array::<8>()?)),
        _ => return Err(InsertPersistedError::Malformed),
    };
    let remote_max_data = u64::from_le_bytes(r.array::<8>()?);
    let max_datagram_size = u16::from_le_bytes(r.array::<2>()?);

    Ok(PersistedStateV1 {
        peer,
        export_secret,
        endpoint,
        ciphersuite,
        stateless_reset,
        created_at_unix_millis,
        sender_current_id,
        receiver_max_seen_key_id,
        remote_max_data,
        max_datagram_size,
    })
}

struct Cursor<'a> {
    bytes: &'a [u8],
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], InsertPersistedError> {
        self.bytes
            .split_off(..n)
            .ok_or(InsertPersistedError::Malformed)
    }

    fn u8(&mut self) -> Result<u8, InsertPersistedError> {
        Ok(self.take(1)?[0])
    }

    #[expect(
        clippy::unwrap_in_result,
        reason = "take(N) yields exactly N bytes, so the array conversion cannot fail"
    )]
    fn array<const N: usize>(&mut self) -> Result<[u8; N], InsertPersistedError> {
        Ok(self.take(N)?.try_into().expect("take(N) returns N bytes"))
    }

    fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &byte in bytes {
        crc ^= byte as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

fn creation_time_to_unix_millis(creation_time: Instant) -> u64 {
    let age = creation_time.elapsed();
    let created_wall = std::time::SystemTime::now().checked_sub(age);
    created_wall
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn unix_millis_to_creation_time(created_at_unix_millis: u64) -> Instant {
    let now_wall = std::time::SystemTime::now();
    let created_wall =
        std::time::UNIX_EPOCH + std::time::Duration::from_millis(created_at_unix_millis);
    let elapsed = now_wall
        .duration_since(created_wall)
        .unwrap_or(std::time::Duration::ZERO);
    Instant::now()
        .checked_sub(elapsed)
        .unwrap_or_else(Instant::now)
}

#[cfg(test)]
impl Entry {
    fn persisted_state_for_test(&self) -> PersistedStateV1 {
        decode(&self.to_persisted_bytes()).unwrap()
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::panic_in_result_fn,
    reason = "test code may unwrap and panic to surface failures"
)]
mod tests {
    use super::*;
    use crate::{
        credentials::{Credentials, KeyId},
        path::secret::schedule::Ciphersuite,
    };
    use rand::RngExt as _;
    use s2n_quic_core::{endpoint, varint::VarInt};
    use std::{net::SocketAddr, sync::Arc};

    fn peer() -> SocketAddr {
        "127.0.0.1:4433".parse().unwrap()
    }

    fn live_entry(sender_current: u64, receiver_max_seen: Option<u64>) -> Entry {
        let export_secret = [7u8; 32];
        let secret = schedule::Secret::new(
            Ciphersuite::AES_GCM_128_SHA256,
            dc::SUPPORTED_VERSIONS[0],
            endpoint::Type::Server,
            &export_secret,
        );

        let sender = sender::State::new([3u8; 16]);
        for _ in 0..sender_current {
            sender.next_key_id();
        }

        let receiver = receiver::State::new();
        if let Some(max) = receiver_max_seen {
            let id = Credentials {
                id: *secret.id(),
                key_id: KeyId::new(max).unwrap(),
            };
            receiver.post_authentication(&id).unwrap();
        }

        Entry::from_restored_parts(
            peer(),
            secret,
            sender,
            receiver,
            dc::ApplicationParams::new(1200, &Default::default(), &Default::default()),
            Instant::now(),
            None,
        )
    }

    fn min_advance() -> RestoreParams {
        RestoreParams { advance: 0 }
    }

    fn verify_one(
        sealer: &crate::path::secret::seal::Once,
        credentials: &Credentials,
        opener: &crate::path::secret::map::Map,
        msg: &[u8],
    ) -> crate::crypto::open::Result {
        use crate::crypto::{open::Application as _, seal::Application as _};

        let mut buf = vec![0u8; msg.len() + sealer.tag_len()];
        sealer.encrypt(0, &[], Some(msg), &mut buf);

        let mut control_out = Vec::new();
        let opener = opener
            .open_once(credentials, None, &mut control_out)
            .expect("opener map has the credential");

        let (payload, tag) = buf.split_at_mut(msg.len());
        opener.decrypt_in_place(sealer.key_phase(), 0, &[], payload, tag)?;
        assert_eq!(msg, payload);
        Ok(())
    }

    fn verify_one_by_id(
        sealer_map: &crate::path::secret::map::Map,
        id: crate::credentials::Id,
        opener: &crate::path::secret::map::Map,
        msg: &[u8],
    ) -> crate::crypto::open::Result {
        let (sealer, credentials, _params) = sealer_map
            .seal_once_id(id)
            .expect("sealer map can seal by id");
        verify_one(&sealer, &credentials, opener, msg)
    }

    fn verify_one_by_addr(
        sealer_map: &crate::path::secret::map::Map,
        peer_addr: SocketAddr,
        opener: &crate::path::secret::map::Map,
        msg: &[u8],
    ) -> crate::crypto::open::Result {
        let peer = sealer_map
            .get_untracked(peer_addr)
            .expect("sealer map reachable by peer address");
        let (sealer, credentials, _params) = peer.seal_once().expect("peer entry can seal");
        verify_one(&sealer, &credentials, opener, msg)
    }

    fn send_random_by_addr(
        sealer_map: &crate::path::secret::map::Map,
        peer_addr: SocketAddr,
        opener: &crate::path::secret::map::Map,
    ) {
        let count = rand::rng().random_range(1..10);
        for _ in 0..count {
            verify_one_by_addr(sealer_map, peer_addr, opener, b"warm-up")
                .expect("warm-up datagram decrypts");
        }
    }

    fn assert_rejected_too_old(result: crate::crypto::open::Result) {
        assert!(
            matches!(
                result,
                Err(crate::crypto::open::Error::ReplayPotentiallyDetected { .. })
            ),
            "expected rejection as too old (ReplayPotentiallyDetected), got {result:?}"
        );
    }

    #[test]
    fn endpoint_byte_round_trips() {
        for endpoint in [endpoint::Type::Client, endpoint::Type::Server] {
            assert_eq!(
                endpoint_byte::decode(endpoint_byte::encode(endpoint)),
                Some(endpoint)
            );
        }
        assert_eq!(endpoint_byte::decode(200), None);
    }

    #[test]
    fn largest_shape_exactly_fills_the_pre_sized_field_region() {
        let mut state = live_entry(10, Some(7)).persisted_state_for_test();
        state.peer = SocketAddr::V6(SocketAddrV6::new(
            "2001:db8::1".parse().unwrap(),
            443,
            0xFEED,
            0xBEEF,
        ));
        assert!(
            state.receiver_max_seen_key_id.is_some(),
            "largest shape needs a receiver"
        );

        let bytes = encode(&state);

        let field_region_len = u16::from_le_bytes([bytes[2], bytes[3]]);
        assert_eq!(
            usize::from(field_region_len),
            112,
            "largest shape must match MAX_FIELD_REGION_LEN"
        );
    }

    #[test]
    fn blob_round_trips_v4_and_v6() {
        for addr in [
            "127.0.0.1:4433".parse::<SocketAddr>().unwrap(),
            "[2001:db8::1]:443".parse::<SocketAddr>().unwrap(),
        ] {
            let mut state = live_entry(0, None).persisted_state_for_test();
            state.peer = addr;
            let bytes = encode(&state);
            let decoded = decode(&bytes).unwrap();
            assert_eq!(decoded.peer, addr);
        }
    }

    #[test]
    fn sender_counter_advances_by_minimum() {
        let bytes = live_entry(10, None).to_persisted_bytes();
        let restored = Entry::restore(&bytes, None, &min_advance()).unwrap();
        assert_eq!(restored.sender().current_id(), 10 + MIN_RESTORE_ADVANCE);
    }

    #[test]
    fn receiver_max_advances_by_minimum() {
        let bytes = live_entry(0, Some(500)).to_persisted_bytes();
        let restored = Entry::restore(&bytes, None, &min_advance()).unwrap();
        assert_eq!(
            restored.receiver().max_seen_key_id(),
            Some(500 + MIN_RESTORE_ADVANCE)
        );
    }

    #[test]
    fn advance_over_minimum_is_honoured() {
        let advance = 5 * MIN_RESTORE_ADVANCE;
        let params = RestoreParams { advance };

        let sender_bytes = live_entry(10, None).to_persisted_bytes();
        let restored = Entry::restore(&sender_bytes, None, &params).unwrap();
        assert_eq!(restored.sender().current_id(), 10 + advance);

        let receiver_bytes = live_entry(0, Some(500)).to_persisted_bytes();
        let restored = Entry::restore(&receiver_bytes, None, &params).unwrap();
        assert_eq!(restored.receiver().max_seen_key_id(), Some(500 + advance));
    }

    #[test]
    fn advance_below_minimum_is_raised_to_it() {
        let params = RestoreParams { advance: 1 };
        let bytes = live_entry(10, None).to_persisted_bytes();
        let restored = Entry::restore(&bytes, None, &params).unwrap();
        assert_eq!(restored.sender().current_id(), 10 + MIN_RESTORE_ADVANCE);
    }

    #[test]
    fn receiver_never_seen_is_advanced_too() {
        let bytes = live_entry(0, None).to_persisted_bytes();
        let restored = Entry::restore(&bytes, None, &min_advance()).unwrap();
        assert_eq!(
            restored.receiver().max_seen_key_id(),
            Some(MIN_RESTORE_ADVANCE)
        );
    }

    #[test]
    fn restored_never_seen_entry_rejects_replay_of_post_write_key_id() {
        let bytes = live_entry(0, None).to_persisted_bytes();
        let restored = Entry::restore(&bytes, None, &min_advance()).unwrap();

        let replay = Credentials {
            id: *restored.secret().id(),
            key_id: KeyId::new(0).unwrap(),
        };
        assert_eq!(
            restored.receiver().post_authentication(&replay),
            Err(receiver::Error::Unknown)
        );
    }

    #[test]
    fn restored_entry_rejects_replay_of_pre_restart_key_id() {
        let bytes = live_entry(0, Some(500)).to_persisted_bytes();
        let restored = Entry::restore(&bytes, None, &min_advance()).unwrap();

        let replay = Credentials {
            id: *restored.secret().id(),
            key_id: KeyId::new(500).unwrap(),
        };
        assert!(restored.receiver().post_authentication(&replay).is_err());
    }

    #[test]
    fn created_at_round_trips_within_one_second() {
        let entry = live_entry(0, None);
        let original_age = entry.age();
        let bytes = entry.to_persisted_bytes();
        let restored = Entry::restore(&bytes, None, &min_advance()).unwrap();
        let delta = restored.age().abs_diff(original_age);
        assert!(
            delta <= std::time::Duration::from_secs(1),
            "created_at drifted by {delta:?}"
        );
    }

    #[test]
    fn credential_id_is_rederived() {
        let entry = live_entry(3, Some(9));
        let id = *entry.id();
        let bytes = entry.to_persisted_bytes();
        let restored = Entry::restore(&bytes, None, &min_advance()).unwrap();
        assert_eq!(restored.id(), &id);
    }

    #[test]
    fn newer_reader_required_is_rejected() {
        let mut bytes = live_entry(0, None).to_persisted_bytes().to_vec();
        bytes.truncate(bytes.len() - 4);
        bytes[1] = THIS_READER_VERSION + 1; // min_reader_version is the second byte
        let crc = crc32(&bytes);
        bytes.extend_from_slice(&crc.to_le_bytes());
        assert!(matches!(
            Entry::restore(&bytes, None, &min_advance()),
            Err(InsertPersistedError::RequiresNewerReader { .. })
        ));
    }

    #[test]
    fn forwards_compatible_additive_blob_loads() {
        let entry = live_entry(7, Some(42));
        let expected_id = *entry.id();
        let good = encode(&entry.persisted_state_for_test());

        let body = &good[..good.len() - 4];
        let orig_region_len = u16::from_le_bytes([body[2], body[3]]) as usize;
        let region = &body[4..4 + orig_region_len];

        let future_tail = [0xAB, 0xCD, 0xEF]; // fields this build does not know
        let new_region_len = (orig_region_len + future_tail.len()) as u16;

        let mut blob = Vec::new();
        blob.push(VERSION + 1); // newer writer's layout version
        blob.push(THIS_READER_VERSION); // but still readable by this build
        blob.extend_from_slice(&new_region_len.to_le_bytes());
        blob.extend_from_slice(region);
        blob.extend_from_slice(&future_tail);
        let crc = crc32(&blob);
        blob.extend_from_slice(&crc.to_le_bytes());

        let restored = Entry::restore(&blob, None, &min_advance()).unwrap();
        assert_eq!(restored.id(), &expected_id);
        assert_eq!(restored.sender().current_id(), 7 + MIN_RESTORE_ADVANCE);
        assert_eq!(
            restored.receiver().max_seen_key_id(),
            Some(42 + MIN_RESTORE_ADVANCE)
        );
    }

    #[test]
    fn bytes_between_region_and_crc_are_malformed() {
        let good = encode(&live_entry(0, None).persisted_state_for_test());
        let body = &good[..good.len() - 4];
        let mut blob = body.to_vec();
        blob.push(0x00); // one byte past the field region
        let crc = crc32(&blob);
        blob.extend_from_slice(&crc.to_le_bytes());
        assert!(matches!(
            Entry::restore(&blob, None, &min_advance()),
            Err(InsertPersistedError::Malformed)
        ));
    }

    #[test]
    fn truncated_blob_is_rejected() {
        let bytes = live_entry(0, None).to_persisted_bytes();
        let truncated = &bytes[..bytes.len() - 6];
        assert!(matches!(
            Entry::restore(truncated, None, &min_advance()),
            Err(InsertPersistedError::Malformed | InsertPersistedError::BadChecksum)
        ));
    }

    #[test]
    fn trailing_bytes_are_rejected() {
        let mut bytes = live_entry(0, None).to_persisted_bytes().to_vec();
        bytes.push(0); // shifts what is read as the trailing CRC
        assert!(matches!(
            Entry::restore(&bytes, None, &min_advance()),
            Err(InsertPersistedError::BadChecksum)
        ));
    }

    #[test]
    fn bad_checksum_is_rejected() {
        let mut bytes = live_entry(0, None).to_persisted_bytes().to_vec();
        bytes[10] ^= 0xFF;
        assert!(matches!(
            Entry::restore(&bytes, None, &min_advance()),
            Err(InsertPersistedError::BadChecksum)
        ));
    }

    #[test]
    fn crc32_matches_known_vector() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn error_dispositions_are_classified() {
        use InsertPersistedError as E;
        use RestoreDisposition::*;

        assert_eq!(E::CounterOverflow.disposition(), SkipEntry);
        assert_eq!(E::ReceiverSentinel.disposition(), SkipEntry);
        assert_eq!(E::InvalidEndpoint(9).disposition(), SkipEntry);
        assert_eq!(E::InvalidCiphersuite(9).disposition(), SkipEntry);
        assert_eq!(E::InvalidPeerTag(9).disposition(), SkipEntry);
        assert_eq!(E::InvalidRemoteMaxData(u64::MAX).disposition(), SkipEntry);
        assert_eq!(E::DuplicateCredentialId.disposition(), SkipEntry);

        assert_eq!(E::Malformed.disposition(), StopFile);
        assert_eq!(E::BadChecksum.disposition(), StopFile);

        assert_eq!(
            E::RequiresNewerReader {
                min_reader_version: 9,
                this_reader: 1
            }
            .disposition(),
            RejectFile
        );
    }

    #[test]
    fn invalid_endpoint_byte_is_rejected() {
        let mut state = live_entry(0, None).persisted_state_for_test();
        state.endpoint = 200;
        let bytes = encode(&state);
        assert!(matches!(
            Entry::restore(&bytes, None, &min_advance()),
            Err(InsertPersistedError::InvalidEndpoint(200))
        ));
    }

    #[test]
    fn invalid_ciphersuite_byte_is_rejected() {
        let mut state = live_entry(0, None).persisted_state_for_test();
        state.ciphersuite = 200;
        let bytes = encode(&state);
        assert!(matches!(
            Entry::restore(&bytes, None, &min_advance()),
            Err(InsertPersistedError::InvalidCiphersuite(200))
        ));
    }

    #[test]
    fn invalid_peer_tag_is_rejected() {
        let state = live_entry(0, None).persisted_state_for_test();
        let mut bytes = encode(&state);
        bytes.truncate(bytes.len() - 4);
        bytes[4] = 200;
        let crc = crc32(&bytes);
        bytes.extend_from_slice(&crc.to_le_bytes());
        assert!(matches!(
            Entry::restore(&bytes, None, &min_advance()),
            Err(InsertPersistedError::InvalidPeerTag(200))
        ));
    }

    #[test]
    fn receiver_sentinel_is_rejected() {
        assert_eq!(
            receiver::State::restore(u64::MAX, 0).unwrap_err(),
            receiver::RestoreError::Sentinel
        );

        let mut state = live_entry(0, Some(1)).persisted_state_for_test();
        state.receiver_max_seen_key_id = Some(u64::MAX);
        let bytes = encode(&state);
        assert!(matches!(
            Entry::restore(&bytes, None, &min_advance()),
            Err(InsertPersistedError::ReceiverSentinel)
        ));
    }

    #[test]
    fn sender_overflow_is_rejected() {
        assert_eq!(
            sender::State::restore(*KeyId::MAX, [0u8; 16], 0).unwrap_err(),
            sender::RestoreError::CounterOverflow
        );

        let mut state = live_entry(0, None).persisted_state_for_test();
        state.sender_current_id = *KeyId::MAX;
        let bytes = encode(&state);
        assert!(matches!(
            Entry::restore(&bytes, None, &min_advance()),
            Err(InsertPersistedError::CounterOverflow)
        ));
    }

    #[test]
    fn remote_max_data_is_persisted() {
        let mut state = live_entry(0, None).persisted_state_for_test();
        state.remote_max_data = 4242;
        let bytes = encode(&state);
        let restored = Entry::restore(&bytes, None, &min_advance()).unwrap();
        assert_eq!(*restored.parameters().remote_max_data, 4242);
    }

    #[test]
    fn remote_max_data_at_varint_max_is_accepted() {
        let mut state = live_entry(0, None).persisted_state_for_test();
        state.remote_max_data = *VarInt::MAX;
        let bytes = encode(&state);
        let restored = Entry::restore(&bytes, None, &min_advance()).unwrap();
        assert_eq!(restored.parameters().remote_max_data, VarInt::MAX);
    }

    #[test]
    fn remote_max_data_out_of_varint_range_is_rejected() {
        let mut state = live_entry(0, None).persisted_state_for_test();
        state.remote_max_data = *VarInt::MAX + 1;
        let bytes = encode(&state);
        assert!(matches!(
            Entry::restore(&bytes, None, &min_advance()),
            Err(InsertPersistedError::InvalidRemoteMaxData(value)) if value == *VarInt::MAX + 1
        ));
    }

    #[test]
    fn max_datagram_size_is_persisted() {
        let mut state = live_entry(0, None).persisted_state_for_test();
        state.max_datagram_size = 8940;
        let bytes = encode(&state);
        let restored = Entry::restore(&bytes, None, &min_advance()).unwrap();
        assert_eq!(restored.parameters().max_datagram_size(), 8940);
    }

    #[test]
    fn inspect_reports_counters_verbatim() {
        let entry = live_entry(10, Some(500));
        let bytes = entry.to_persisted_bytes();

        let report = Entry::inspect_persisted_bytes(&bytes, None).unwrap();

        assert_eq!(report.id, *entry.id());
        assert_eq!(report.peer, *entry.peer());
        assert_eq!(report.sender_current_id, 10);
        assert_eq!(report.receiver_max_seen_key_id, Some(500));

        let restored = Entry::restore(&bytes, None, &min_advance()).unwrap();
        assert_eq!(restored.sender().current_id(), 10 + MIN_RESTORE_ADVANCE);
        assert_eq!(
            restored.receiver().max_seen_key_id(),
            Some(500 + MIN_RESTORE_ADVANCE)
        );
    }

    #[test]
    fn inspect_reports_never_seen_receiver_as_none() {
        let bytes = live_entry(0, None).to_persisted_bytes();
        let report = Entry::inspect_persisted_bytes(&bytes, None).unwrap();
        assert_eq!(report.receiver_max_seen_key_id, None);
    }

    #[test]
    fn inspect_passes_application_data_through() {
        let bytes = live_entry(1, None).to_persisted_bytes();
        let data: ApplicationData = std::sync::Arc::new(42u32);

        let report = Entry::inspect_persisted_bytes(&bytes, Some(data)).unwrap();

        let carried = report
            .application_data
            .expect("application data is returned");
        assert_eq!(carried.downcast_ref::<u32>(), Some(&42));
    }

    #[test]
    fn inspect_reports_a_counter_too_large_to_restore() {
        let near_max = *s2n_quic_core::varint::VarInt::MAX - 1;
        let mut state = live_entry(0, None).persisted_state_for_test();
        state.sender_current_id = near_max;
        let bytes = encode(&state);

        let report = Entry::inspect_persisted_bytes(&bytes, None).unwrap();
        assert_eq!(report.sender_current_id, near_max);

        assert!(matches!(
            Entry::restore(&bytes, None, &min_advance()),
            Err(InsertPersistedError::CounterOverflow)
        ));
    }

    #[test]
    fn restore_needs_no_map() {
        let entry = live_entry(4, Some(7));
        let id = *entry.id();
        let peer_addr = *entry.peer();
        let bytes = entry.to_persisted_bytes();

        let restored = Entry::restore(&bytes, None, &min_advance()).unwrap();

        assert_eq!(restored.id(), &id);
        assert_eq!(restored.peer(), &peer_addr);
    }

    #[test]
    fn restore_and_inspect_agree() {
        let bytes = live_entry(4, Some(7)).to_persisted_bytes();

        let restored = Entry::restore(&bytes, None, &min_advance()).unwrap();
        let report = Entry::inspect_persisted_bytes(&bytes, None).unwrap();

        assert_eq!(&report.id, restored.id());
        assert_eq!(&report.peer, restored.peer());

        assert_eq!(report.sender_current_id, 4);
        assert_eq!(report.receiver_max_seen_key_id, Some(7));

        assert_eq!(
            restored.sender().current_id(),
            report.sender_current_id + MIN_RESTORE_ADVANCE
        );
        assert_eq!(
            restored.receiver().max_seen_key_id(),
            report
                .receiver_max_seen_key_id
                .map(|id| id + MIN_RESTORE_ADVANCE)
        );
    }

    #[test]
    fn restore_reports_errors() {
        let err = Entry::restore(b"not a blob", None, &min_advance())
            .expect_err("garbage blob must fail");
        assert!(matches!(
            err,
            InsertPersistedError::BadChecksum | InsertPersistedError::Malformed
        ));
        assert_eq!(err.disposition(), RestoreDisposition::StopFile);

        let mut state = live_entry(0, None).persisted_state_for_test();
        state.ciphersuite = 0xff;
        let bytes = encode(&state);
        let err =
            Entry::restore(&bytes, None, &min_advance()).expect_err("bad ciphersuite must fail");
        assert!(matches!(
            err,
            InsertPersistedError::InvalidCiphersuite(0xff)
        ));
        assert_eq!(err.disposition(), RestoreDisposition::SkipEntry);
    }

    fn test_map_with_entries(
        capacity: usize,
        entries: Vec<Arc<Entry>>,
    ) -> crate::path::secret::map::Map {
        use crate::{event, path::secret::stateless_reset};
        use s2n_quic_core::time;

        crate::path::secret::map::Map::builder()
            .with_signer(stateless_reset::Signer::random())
            .with_capacity(capacity)
            .with_clock(time::NoopClock)
            .with_subscriber(event::testing::Subscriber::no_snapshot())
            .with_entries(entries)
            .build()
            .expect("map builds with restored entries")
    }

    fn restored(bytes: &[u8]) -> Arc<Entry> {
        Arc::new(Entry::restore(bytes, None, &min_advance()).expect("persisted bytes restore"))
    }

    #[test]
    fn restored_entry_is_reachable_by_id_and_address_with_entries() {
        let entry = live_entry(4, Some(7));
        let id = *entry.id();
        let bytes = entry.to_persisted_bytes();

        let map = test_map_with_entries(10, vec![restored(&bytes)]);

        assert!(map.contains(&peer()));
        assert!(map.get_untracked(peer()).is_some());

        let (_sealer, credentials, _params) = map.seal_once_id(id).expect("entry reachable by id");
        assert_eq!(credentials.id, id);
    }

    #[test]
    fn restored_entry_produces_working_sealer_and_opener_with_entries() {
        let client = test_map();
        let server = test_map();
        let client_addr = "127.0.0.1:1234".parse().unwrap();
        let server_addr = "127.0.0.1:5678".parse().unwrap();
        let id = client.test_insert_pair(client_addr, None, &server, server_addr, None);

        let server_bytes = server
            .store
            .get_by_id_untracked(&id)
            .unwrap()
            .to_persisted_bytes();
        let restored_server = test_map_with_entries(10, vec![restored(&server_bytes)]);

        assert_rejected_too_old(verify_one_by_id(
            &client,
            id,
            &restored_server,
            b"hello from client",
        ));
        simulate_stale_key(&restored_server, &client, id);

        verify_one_by_id(&client, id, &restored_server, b"hello from client")
            .expect("restored server decrypts the client's datagram after StaleKey recovery");
        verify_one_by_id(&restored_server, id, &client, b"hello from restored server")
            .expect("client decrypts the restored server's reply");
    }

    #[test]
    fn restored_entry_is_reachable_by_peer_address_for_crypto_with_entries() {
        let client = test_map();
        let server = test_map();
        let client_addr = "127.0.0.1:1234".parse().unwrap();
        let server_addr = "127.0.0.1:5678".parse().unwrap();
        let id = client.test_insert_pair(client_addr, None, &server, server_addr, None);

        let server_bytes = server
            .store
            .get_by_id_untracked(&id)
            .unwrap()
            .to_persisted_bytes();
        let restored_server = test_map_with_entries(10, vec![restored(&server_bytes)]);

        assert_eq!(
            restored_server.get_untracked(client_addr).unwrap().id(),
            &id
        );

        verify_one_by_addr(&restored_server, client_addr, &client, b"hello by address")
            .expect("client decrypts the restored server's datagram");

        assert_rejected_too_old(verify_one_by_addr(
            &client,
            server_addr,
            &restored_server,
            b"reply by address",
        ));
        simulate_stale_key(&restored_server, &client, id);
        verify_one_by_addr(&client, server_addr, &restored_server, b"reply by address")
            .expect("restored server decrypts the client's reply after StaleKey recovery");
    }

    #[test]
    fn restored_client_entry_produces_working_sealer_and_opener_with_entries() {
        let client = test_map();
        let server = test_map();
        let client_addr = "127.0.0.1:1234".parse().unwrap();
        let server_addr = "127.0.0.1:5678".parse().unwrap();
        let id = client.test_insert_pair(client_addr, None, &server, server_addr, None);

        let client_bytes = client
            .store
            .get_by_id_untracked(&id)
            .unwrap()
            .to_persisted_bytes();
        let restored_client = test_map_with_entries(10, vec![restored(&client_bytes)]);

        verify_one_by_id(&restored_client, id, &server, b"hello from restored client")
            .expect("server decrypts the restored client's datagram");

        assert_rejected_too_old(verify_one_by_id(
            &server,
            id,
            &restored_client,
            b"hello from server",
        ));
        simulate_stale_key(&restored_client, &server, id);
        verify_one_by_id(&server, id, &restored_client, b"hello from server")
            .expect("restored client decrypts the server's reply after StaleKey recovery");
    }

    #[test]
    fn restored_client_entry_is_reachable_by_peer_address_for_crypto_with_entries() {
        let client = test_map();
        let server = test_map();
        let client_addr = "127.0.0.1:1234".parse().unwrap();
        let server_addr = "127.0.0.1:5678".parse().unwrap();
        let id = client.test_insert_pair(client_addr, None, &server, server_addr, None);

        let client_bytes = client
            .store
            .get_by_id_untracked(&id)
            .unwrap()
            .to_persisted_bytes();
        let restored_client = test_map_with_entries(10, vec![restored(&client_bytes)]);

        assert_eq!(
            restored_client.get_untracked(server_addr).unwrap().id(),
            &id
        );

        verify_one_by_addr(&restored_client, server_addr, &server, b"hello by address")
            .expect("server decrypts the restored client's datagram");

        assert_rejected_too_old(verify_one_by_addr(
            &server,
            client_addr,
            &restored_client,
            b"reply by address",
        ));
        simulate_stale_key(&restored_client, &server, id);
        verify_one_by_addr(&server, client_addr, &restored_client, b"reply by address")
            .expect("restored client decrypts the server's reply after StaleKey recovery");
    }

    #[test]
    fn both_restored_entries_produce_working_sealer_and_opener_with_entries() {
        let client = test_map();
        let server = test_map();
        let client_addr = "127.0.0.1:1234".parse().unwrap();
        let server_addr = "127.0.0.1:5678".parse().unwrap();
        let id = client.test_insert_pair(client_addr, None, &server, server_addr, None);

        let client_bytes = client
            .store
            .get_by_id_untracked(&id)
            .unwrap()
            .to_persisted_bytes();
        let server_bytes = server
            .store
            .get_by_id_untracked(&id)
            .unwrap()
            .to_persisted_bytes();

        let restored_client = test_map_with_entries(10, vec![restored(&client_bytes)]);
        let restored_server = test_map_with_entries(10, vec![restored(&server_bytes)]);

        verify_one_by_id(
            &restored_client,
            id,
            &restored_server,
            b"hello between restored peers",
        )
        .expect("restored server decrypts the restored client's datagram");
        verify_one_by_id(
            &restored_server,
            id,
            &restored_client,
            b"reply between restored peers",
        )
        .expect("restored client decrypts the restored server's reply");
    }

    #[test]
    fn both_restored_entries_reachable_by_peer_address_for_crypto_with_entries() {
        let client = test_map();
        let server = test_map();
        let client_addr = "127.0.0.1:1234".parse().unwrap();
        let server_addr = "127.0.0.1:5678".parse().unwrap();
        let id = client.test_insert_pair(client_addr, None, &server, server_addr, None);

        let client_bytes = client
            .store
            .get_by_id_untracked(&id)
            .unwrap()
            .to_persisted_bytes();
        let server_bytes = server
            .store
            .get_by_id_untracked(&id)
            .unwrap()
            .to_persisted_bytes();

        let restored_client = test_map_with_entries(10, vec![restored(&client_bytes)]);
        let restored_server = test_map_with_entries(10, vec![restored(&server_bytes)]);

        assert_eq!(
            restored_client.get_untracked(server_addr).unwrap().id(),
            &id
        );
        assert_eq!(
            restored_server.get_untracked(client_addr).unwrap().id(),
            &id
        );

        verify_one_by_addr(
            &restored_client,
            server_addr,
            &restored_server,
            b"hello between restored peers",
        )
        .expect("restored server decrypts the restored client's datagram");
        verify_one_by_addr(
            &restored_server,
            client_addr,
            &restored_client,
            b"reply between restored peers",
        )
        .expect("restored client decrypts the restored server's reply");
    }

    #[test]
    fn server_restore_stale_key_recovery_by_id_with_entries() {
        let client = test_map();
        let server = test_map();
        let client_addr = "127.0.0.1:1234".parse().unwrap();
        let server_addr = "127.0.0.1:5678".parse().unwrap();
        let id = client.test_insert_pair(client_addr, None, &server, server_addr, None);
        warm_up_both_directions(&client, client_addr, &server, server_addr);

        let server_bytes = server
            .store
            .get_by_id_untracked(&id)
            .unwrap()
            .to_persisted_bytes();
        let restored_server = test_map_with_entries(10, vec![restored(&server_bytes)]);

        assert!(
            restored_server
                .store
                .get_by_id_untracked(&id)
                .unwrap()
                .receiver()
                .max_seen_key_id()
                .is_some(),
            "the restored receiver carries a maximum-seen key id"
        );

        assert_rejected_too_old(verify_one_by_id(&client, id, &restored_server, b"first"));
        simulate_stale_key(&restored_server, &client, id);
        verify_one_by_id(&client, id, &restored_server, b"second")
            .expect("after StaleKey recovery the client's datagram decrypts");
    }

    #[test]
    fn server_restore_stale_key_recovery_by_addr_with_entries() {
        let client = test_map();
        let server = test_map();
        let client_addr = "127.0.0.1:1234".parse().unwrap();
        let server_addr = "127.0.0.1:5678".parse().unwrap();
        let id = client.test_insert_pair(client_addr, None, &server, server_addr, None);
        warm_up_both_directions(&client, client_addr, &server, server_addr);

        let server_bytes = server
            .store
            .get_by_id_untracked(&id)
            .unwrap()
            .to_persisted_bytes();
        let restored_server = test_map_with_entries(10, vec![restored(&server_bytes)]);

        assert_rejected_too_old(verify_one_by_addr(
            &client,
            server_addr,
            &restored_server,
            b"first",
        ));
        simulate_stale_key(&restored_server, &client, id);
        verify_one_by_addr(&client, server_addr, &restored_server, b"second")
            .expect("after StaleKey recovery the client's datagram decrypts");
    }

    #[test]
    fn client_restore_stale_key_recovery_by_id_with_entries() {
        let client = test_map();
        let server = test_map();
        let client_addr = "127.0.0.1:1234".parse().unwrap();
        let server_addr = "127.0.0.1:5678".parse().unwrap();
        let id = client.test_insert_pair(client_addr, None, &server, server_addr, None);
        warm_up_both_directions(&client, client_addr, &server, server_addr);

        let client_bytes = client
            .store
            .get_by_id_untracked(&id)
            .unwrap()
            .to_persisted_bytes();
        let restored_client = test_map_with_entries(10, vec![restored(&client_bytes)]);

        assert_rejected_too_old(verify_one_by_id(&server, id, &restored_client, b"first"));
        simulate_stale_key(&restored_client, &server, id);
        verify_one_by_id(&server, id, &restored_client, b"second")
            .expect("after StaleKey recovery the server's datagram decrypts");
    }

    #[test]
    fn client_restore_stale_key_recovery_by_addr_with_entries() {
        let client = test_map();
        let server = test_map();
        let client_addr = "127.0.0.1:1234".parse().unwrap();
        let server_addr = "127.0.0.1:5678".parse().unwrap();
        let id = client.test_insert_pair(client_addr, None, &server, server_addr, None);
        warm_up_both_directions(&client, client_addr, &server, server_addr);

        let client_bytes = client
            .store
            .get_by_id_untracked(&id)
            .unwrap()
            .to_persisted_bytes();
        let restored_client = test_map_with_entries(10, vec![restored(&client_bytes)]);

        assert_rejected_too_old(verify_one_by_addr(
            &server,
            client_addr,
            &restored_client,
            b"first",
        ));
        simulate_stale_key(&restored_client, &server, id);
        verify_one_by_addr(&server, client_addr, &restored_client, b"second")
            .expect("after StaleKey recovery the server's datagram decrypts");
    }

    #[test]
    fn both_restore_needs_no_stale_key_recovery_by_id_with_entries() {
        let client = test_map();
        let server = test_map();
        let client_addr = "127.0.0.1:1234".parse().unwrap();
        let server_addr = "127.0.0.1:5678".parse().unwrap();
        let id = client.test_insert_pair(client_addr, None, &server, server_addr, None);
        warm_up_both_directions(&client, client_addr, &server, server_addr);

        let client_bytes = client
            .store
            .get_by_id_untracked(&id)
            .unwrap()
            .to_persisted_bytes();
        let server_bytes = server
            .store
            .get_by_id_untracked(&id)
            .unwrap()
            .to_persisted_bytes();

        let restored_client = test_map_with_entries(10, vec![restored(&client_bytes)]);
        let restored_server = test_map_with_entries(10, vec![restored(&server_bytes)]);

        verify_one_by_id(&restored_client, id, &restored_server, b"first")
            .expect("the very first datagram between two restored peers decrypts");
    }

    #[test]
    fn both_restore_needs_no_stale_key_recovery_by_addr_with_entries() {
        let client = test_map();
        let server = test_map();
        let client_addr = "127.0.0.1:1234".parse().unwrap();
        let server_addr = "127.0.0.1:5678".parse().unwrap();
        let id = client.test_insert_pair(client_addr, None, &server, server_addr, None);
        warm_up_both_directions(&client, client_addr, &server, server_addr);

        let client_bytes = client
            .store
            .get_by_id_untracked(&id)
            .unwrap()
            .to_persisted_bytes();
        let server_bytes = server
            .store
            .get_by_id_untracked(&id)
            .unwrap()
            .to_persisted_bytes();

        let restored_client = test_map_with_entries(10, vec![restored(&client_bytes)]);
        let restored_server = test_map_with_entries(10, vec![restored(&server_bytes)]);

        verify_one_by_addr(&restored_client, server_addr, &restored_server, b"first")
            .expect("the very first datagram between two restored peers decrypts");
    }

    #[test]
    fn surviving_entry_after_over_capacity_with_entries_still_works() {
        let client = test_map();
        let server = test_map();
        let client_addr = "127.0.0.1:1234".parse().unwrap();
        let server_addr = "127.0.0.1:5678".parse().unwrap();
        let id = client.test_insert_pair(client_addr, None, &server, server_addr, None);

        let client_bytes = client
            .store
            .get_by_id_untracked(&id)
            .unwrap()
            .to_persisted_bytes();
        let server_bytes = server
            .store
            .get_by_id_untracked(&id)
            .unwrap()
            .to_persisted_bytes();

        let stale = live_entry(9, None).to_persisted_bytes();
        let restored_client =
            test_map_with_entries(1, vec![restored(&stale), restored(&client_bytes)]);
        let restored_server = test_map_with_entries(10, vec![restored(&server_bytes)]);

        assert_eq!(
            restored_client.get_untracked(server_addr).unwrap().id(),
            &id,
            "the surviving entry is the youngest one"
        );

        verify_one_by_addr(
            &restored_client,
            server_addr,
            &restored_server,
            b"the survivor still has working keys",
        )
        .expect("the entry that survived the trim still seals correctly");
    }

    fn test_map() -> crate::path::secret::map::Map {
        use crate::{event, path::secret::stateless_reset};
        use s2n_quic_core::time;

        crate::path::secret::map::Map::new(
            stateless_reset::Signer::random(),
            10,
            false,
            time::NoopClock,
            event::testing::Subscriber::no_snapshot(),
        )
    }

    #[test]
    fn restored_entry_is_reachable_by_id_and_address() {
        let map = test_map();
        let entry = live_entry(4, Some(7));
        let id = *entry.id();
        let bytes = entry.to_persisted_bytes();

        let restored = map.insert_persisted(&bytes, None, &min_advance()).unwrap();
        assert_eq!(restored.id(), &id);

        assert!(map.contains(&peer()));
        assert!(map.get_untracked(peer()).is_some());

        let (_sealer, credentials, _params) = map.seal_once_id(id).expect("entry reachable by id");
        assert_eq!(credentials.id, id);
    }

    #[test]
    fn restored_entry_produces_working_sealer_and_opener() {
        let client = test_map();
        let server = test_map();
        let client_addr = "127.0.0.1:1234".parse().unwrap();
        let server_addr = "127.0.0.1:5678".parse().unwrap();
        let id = client.test_insert_pair(client_addr, None, &server, server_addr, None);

        let server_entry = server.store.get_by_id_untracked(&id).unwrap();
        let bytes = server_entry.to_persisted_bytes();
        let restored_server = test_map();
        restored_server
            .insert_persisted(&bytes, None, &min_advance())
            .unwrap();

        assert_rejected_too_old(verify_one_by_id(
            &client,
            id,
            &restored_server,
            b"hello from client",
        ));
        simulate_stale_key(&restored_server, &client, id);
        verify_one_by_id(&client, id, &restored_server, b"hello from client")
            .expect("restored server decrypts the client's datagram after StaleKey recovery");

        verify_one_by_id(
            &restored_server,
            id,
            &client,
            b"hello back from restored server",
        )
        .expect("client decrypts the restored server's reply");
    }

    #[test]
    fn restored_entry_is_reachable_by_peer_address_for_crypto() {
        let client = test_map();
        let server = test_map();
        let client_addr = "127.0.0.1:1234".parse().unwrap();
        let server_addr = "127.0.0.1:5678".parse().unwrap();
        let id = client.test_insert_pair(client_addr, None, &server, server_addr, None);

        let server_entry = server.store.get_by_id_untracked(&id).unwrap();
        let bytes = server_entry.to_persisted_bytes();
        let restored_server = test_map();
        restored_server
            .insert_persisted(&bytes, None, &min_advance())
            .unwrap();

        assert_eq!(
            restored_server.get_untracked(client_addr).unwrap().id(),
            &id
        );

        verify_one_by_addr(
            &restored_server,
            client_addr,
            &client,
            b"hello from restored server",
        )
        .expect("client decrypts the restored server's datagram");

        assert_rejected_too_old(verify_one_by_addr(
            &client,
            server_addr,
            &restored_server,
            b"hello back from client",
        ));
        simulate_stale_key(&restored_server, &client, id);
        verify_one_by_addr(
            &client,
            server_addr,
            &restored_server,
            b"hello back from client",
        )
        .expect("restored server decrypts the client's reply after StaleKey recovery");
    }

    #[test]
    fn restored_client_entry_produces_working_sealer_and_opener() {
        let client = test_map();
        let server = test_map();
        let client_addr = "127.0.0.1:1234".parse().unwrap();
        let server_addr = "127.0.0.1:5678".parse().unwrap();
        let id = client.test_insert_pair(client_addr, None, &server, server_addr, None);

        let client_entry = client.store.get_by_id_untracked(&id).unwrap();
        let bytes = client_entry.to_persisted_bytes();
        let restored_client = test_map();
        restored_client
            .insert_persisted(&bytes, None, &min_advance())
            .unwrap();

        verify_one_by_id(&restored_client, id, &server, b"hello from restored client")
            .expect("server decrypts the restored client's datagram");

        assert_rejected_too_old(verify_one_by_id(
            &server,
            id,
            &restored_client,
            b"hello back from server",
        ));
        simulate_stale_key(&restored_client, &server, id);
        verify_one_by_id(&server, id, &restored_client, b"hello back from server")
            .expect("restored client decrypts the server's reply after StaleKey recovery");
    }

    #[test]
    fn restored_client_entry_is_reachable_by_peer_address_for_crypto() {
        let client = test_map();
        let server = test_map();
        let client_addr = "127.0.0.1:1234".parse().unwrap();
        let server_addr = "127.0.0.1:5678".parse().unwrap();
        let id = client.test_insert_pair(client_addr, None, &server, server_addr, None);

        let client_entry = client.store.get_by_id_untracked(&id).unwrap();
        let bytes = client_entry.to_persisted_bytes();
        let restored_client = test_map();
        restored_client
            .insert_persisted(&bytes, None, &min_advance())
            .unwrap();

        assert_eq!(
            restored_client.get_untracked(server_addr).unwrap().id(),
            &id
        );

        verify_one_by_addr(
            &restored_client,
            server_addr,
            &server,
            b"hello from restored client",
        )
        .expect("server decrypts the restored client's datagram");

        assert_rejected_too_old(verify_one_by_addr(
            &server,
            client_addr,
            &restored_client,
            b"hello back from server",
        ));
        simulate_stale_key(&restored_client, &server, id);
        verify_one_by_addr(
            &server,
            client_addr,
            &restored_client,
            b"hello back from server",
        )
        .expect("restored client decrypts the server's reply after StaleKey recovery");
    }

    #[test]
    fn both_restored_entries_produce_working_sealer_and_opener() {
        let client = test_map();
        let server = test_map();
        let client_addr = "127.0.0.1:1234".parse().unwrap();
        let server_addr = "127.0.0.1:5678".parse().unwrap();
        let id = client.test_insert_pair(client_addr, None, &server, server_addr, None);

        let client_bytes = client
            .store
            .get_by_id_untracked(&id)
            .unwrap()
            .to_persisted_bytes();
        let server_bytes = server
            .store
            .get_by_id_untracked(&id)
            .unwrap()
            .to_persisted_bytes();
        let restored_client = test_map();
        let restored_server = test_map();
        restored_client
            .insert_persisted(&client_bytes, None, &min_advance())
            .unwrap();
        restored_server
            .insert_persisted(&server_bytes, None, &min_advance())
            .unwrap();

        verify_one_by_id(
            &restored_client,
            id,
            &restored_server,
            b"hello between restored peers",
        )
        .expect("restored server decrypts the restored client's datagram");

        verify_one_by_id(
            &restored_server,
            id,
            &restored_client,
            b"reply between restored peers",
        )
        .expect("restored client decrypts the restored server's reply");
    }

    #[test]
    fn both_restored_entries_reachable_by_peer_address_for_crypto() {
        let client = test_map();
        let server = test_map();
        let client_addr = "127.0.0.1:1234".parse().unwrap();
        let server_addr = "127.0.0.1:5678".parse().unwrap();
        let id = client.test_insert_pair(client_addr, None, &server, server_addr, None);

        let client_bytes = client
            .store
            .get_by_id_untracked(&id)
            .unwrap()
            .to_persisted_bytes();
        let server_bytes = server
            .store
            .get_by_id_untracked(&id)
            .unwrap()
            .to_persisted_bytes();
        let restored_client = test_map();
        let restored_server = test_map();
        restored_client
            .insert_persisted(&client_bytes, None, &min_advance())
            .unwrap();
        restored_server
            .insert_persisted(&server_bytes, None, &min_advance())
            .unwrap();

        assert_eq!(
            restored_client.get_untracked(server_addr).unwrap().id(),
            &id
        );
        assert_eq!(
            restored_server.get_untracked(client_addr).unwrap().id(),
            &id
        );

        verify_one_by_addr(
            &restored_client,
            server_addr,
            &restored_server,
            b"hello between restored peers",
        )
        .expect("restored server decrypts the restored client's datagram");

        verify_one_by_addr(
            &restored_server,
            client_addr,
            &restored_client,
            b"reply between restored peers",
        )
        .expect("restored client decrypts the restored server's reply");
    }

    fn warm_up_both_directions(
        client: &crate::path::secret::map::Map,
        client_addr: SocketAddr,
        server: &crate::path::secret::map::Map,
        server_addr: SocketAddr,
    ) {
        send_random_by_addr(client, server_addr, server); // client -> server
        send_random_by_addr(server, client_addr, client); // server -> client
    }

    fn simulate_stale_key(
        restored: &crate::path::secret::map::Map,
        peer: &crate::path::secret::map::Map,
        id: crate::credentials::Id,
    ) {
        let min = restored
            .store
            .get_by_id_untracked(&id)
            .unwrap()
            .receiver()
            .minimum_unseen_key_id();
        peer.store
            .get_by_id_untracked(&id)
            .unwrap()
            .simulate_stale_key(min);
    }

    #[test]
    fn server_restore_stale_key_recovery_by_id() {
        let client = test_map();
        let server = test_map();
        let client_addr = "127.0.0.1:1234".parse().unwrap();
        let server_addr = "127.0.0.1:5678".parse().unwrap();
        let id = client.test_insert_pair(client_addr, None, &server, server_addr, None);
        warm_up_both_directions(&client, client_addr, &server, server_addr);

        let bytes = server
            .store
            .get_by_id_untracked(&id)
            .unwrap()
            .to_persisted_bytes();
        let restored_server = test_map();
        restored_server
            .insert_persisted(&bytes, None, &min_advance())
            .unwrap();

        assert!(restored_server
            .store
            .get_by_id_untracked(&id)
            .unwrap()
            .receiver()
            .max_seen_key_id()
            .is_some());

        assert_rejected_too_old(verify_one_by_id(&client, id, &restored_server, b"first"));

        simulate_stale_key(&restored_server, &client, id);
        verify_one_by_id(&client, id, &restored_server, b"second")
            .expect("client's packet decrypts after StaleKey recovery");
    }

    #[test]
    fn server_restore_stale_key_recovery_by_addr() {
        let client = test_map();
        let server = test_map();
        let client_addr = "127.0.0.1:1234".parse().unwrap();
        let server_addr = "127.0.0.1:5678".parse().unwrap();
        let id = client.test_insert_pair(client_addr, None, &server, server_addr, None);
        warm_up_both_directions(&client, client_addr, &server, server_addr);

        let bytes = server
            .store
            .get_by_id_untracked(&id)
            .unwrap()
            .to_persisted_bytes();
        let restored_server = test_map();
        restored_server
            .insert_persisted(&bytes, None, &min_advance())
            .unwrap();

        assert_rejected_too_old(verify_one_by_addr(
            &client,
            server_addr,
            &restored_server,
            b"first",
        ));

        simulate_stale_key(&restored_server, &client, id);
        verify_one_by_addr(&client, server_addr, &restored_server, b"second")
            .expect("client's packet decrypts after StaleKey recovery");
    }

    #[test]
    fn client_restore_stale_key_recovery_by_id() {
        let client = test_map();
        let server = test_map();
        let client_addr = "127.0.0.1:1234".parse().unwrap();
        let server_addr = "127.0.0.1:5678".parse().unwrap();
        let id = client.test_insert_pair(client_addr, None, &server, server_addr, None);
        warm_up_both_directions(&client, client_addr, &server, server_addr);

        let bytes = client
            .store
            .get_by_id_untracked(&id)
            .unwrap()
            .to_persisted_bytes();
        let restored_client = test_map();
        restored_client
            .insert_persisted(&bytes, None, &min_advance())
            .unwrap();

        assert_rejected_too_old(verify_one_by_id(&server, id, &restored_client, b"first"));

        simulate_stale_key(&restored_client, &server, id);
        verify_one_by_id(&server, id, &restored_client, b"second")
            .expect("server's packet decrypts after StaleKey recovery");
    }

    #[test]
    fn client_restore_stale_key_recovery_by_addr() {
        let client = test_map();
        let server = test_map();
        let client_addr = "127.0.0.1:1234".parse().unwrap();
        let server_addr = "127.0.0.1:5678".parse().unwrap();
        let id = client.test_insert_pair(client_addr, None, &server, server_addr, None);
        warm_up_both_directions(&client, client_addr, &server, server_addr);

        let bytes = client
            .store
            .get_by_id_untracked(&id)
            .unwrap()
            .to_persisted_bytes();
        let restored_client = test_map();
        restored_client
            .insert_persisted(&bytes, None, &min_advance())
            .unwrap();

        assert_rejected_too_old(verify_one_by_addr(
            &server,
            client_addr,
            &restored_client,
            b"first",
        ));

        simulate_stale_key(&restored_client, &server, id);
        verify_one_by_addr(&server, client_addr, &restored_client, b"second")
            .expect("server's packet decrypts after StaleKey recovery");
    }

    #[test]
    fn both_restore_needs_no_stale_key_recovery_by_id() {
        let client = test_map();
        let server = test_map();
        let client_addr = "127.0.0.1:1234".parse().unwrap();
        let server_addr = "127.0.0.1:5678".parse().unwrap();
        let id = client.test_insert_pair(client_addr, None, &server, server_addr, None);
        warm_up_both_directions(&client, client_addr, &server, server_addr);

        let client_bytes = client
            .store
            .get_by_id_untracked(&id)
            .unwrap()
            .to_persisted_bytes();
        let server_bytes = server
            .store
            .get_by_id_untracked(&id)
            .unwrap()
            .to_persisted_bytes();
        let restored_client = test_map();
        let restored_server = test_map();
        restored_client
            .insert_persisted(&client_bytes, None, &min_advance())
            .unwrap();
        restored_server
            .insert_persisted(&server_bytes, None, &min_advance())
            .unwrap();

        verify_one_by_id(&restored_client, id, &restored_server, b"first")
            .expect("restored client's first packet decrypts without any recovery");
    }

    #[test]
    fn both_restore_needs_no_stale_key_recovery_by_addr() {
        let client = test_map();
        let server = test_map();
        let client_addr = "127.0.0.1:1234".parse().unwrap();
        let server_addr = "127.0.0.1:5678".parse().unwrap();
        let id = client.test_insert_pair(client_addr, None, &server, server_addr, None);
        warm_up_both_directions(&client, client_addr, &server, server_addr);

        let client_bytes = client
            .store
            .get_by_id_untracked(&id)
            .unwrap()
            .to_persisted_bytes();
        let server_bytes = server
            .store
            .get_by_id_untracked(&id)
            .unwrap()
            .to_persisted_bytes();
        let restored_client = test_map();
        let restored_server = test_map();
        restored_client
            .insert_persisted(&client_bytes, None, &min_advance())
            .unwrap();
        restored_server
            .insert_persisted(&server_bytes, None, &min_advance())
            .unwrap();

        verify_one_by_addr(&restored_client, server_addr, &restored_server, b"first")
            .expect("restored client's first packet decrypts without any recovery");
    }

    #[test]
    fn duplicate_credential_id_is_rejected_not_panicked() {
        let map = test_map();
        let bytes = live_entry(0, None).to_persisted_bytes();

        map.insert_persisted(&bytes, None, &min_advance()).unwrap();
        assert!(matches!(
            map.insert_persisted(&bytes, None, &min_advance()),
            Err(InsertPersistedError::DuplicateCredentialId)
        ));
    }

    // --- assumptions about `Instant` -------------------------------------------------------------
    //
    // `unix_millis_to_creation_time` rebuilds an age by subtracting from `Instant::now()`, and an
    // entry older than the host's uptime is the normal case after a reboot. These pin that, using
    // an age no machine could have been up for rather than reading the uptime: a duration that
    // merely *might* exceed it would stop testing anything on a long-lived builder. `Instant` is
    // signed only where it is a timespec, so an unsigned-counter platform fails the first of these.

    /// Longer than any host has been up, so subtracting it necessarily crosses the monotonic
    /// origin. Also within `SystemTime`'s range on the way through the wall clock.
    const BEYOND_ANY_UPTIME: std::time::Duration =
        std::time::Duration::from_secs(50 * 365 * 86_400);

    #[test]
    fn instant_subtracts_past_its_origin() {
        let now = Instant::now();

        let before_origin = now
            .checked_sub(BEYOND_ANY_UPTIME)
            .expect("Instant must represent a point before its own origin");

        assert_eq!(
            now.duration_since(before_origin),
            BEYOND_ANY_UPTIME,
            "the age must survive the round trip exactly, not saturate at the origin"
        );
    }

    #[test]
    fn creation_time_round_trip_preserves_the_age() {
        let created = Instant::now()
            .checked_sub(BEYOND_ANY_UPTIME)
            .expect("see instant_subtracts_past_its_origin");

        let restored = unix_millis_to_creation_time(creation_time_to_unix_millis(created));

        // Two `SystemTime::now()` readings bracket the conversion, and the wall clock is recorded
        // in truncated milliseconds, so allow a second of slack.
        let age = restored.elapsed();
        assert!(
            age.abs_diff(BEYOND_ANY_UPTIME) < std::time::Duration::from_secs(1),
            "the helper pair must preserve an age that predates the monotonic origin, got {age:?}"
        );
    }

    #[test]
    fn creation_time_ahead_of_the_wall_clock_reads_as_brand_new() {
        // A writer whose clock ran ahead of ours. `duration_since` errors and the fallback takes
        // zero, so the entry looks freshly created -- which defers its rehandshake.
        let future_millis = creation_time_to_unix_millis(Instant::now()) + 60_000;

        let restored = unix_millis_to_creation_time(future_millis);

        assert!(
            restored.elapsed() < std::time::Duration::from_secs(1),
            "a future timestamp must clamp to an age of zero rather than wrap"
        );
    }
}

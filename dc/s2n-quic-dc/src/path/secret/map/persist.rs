// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Serialising a map entry to an opaque, versioned byte blob, and rebuilding it on load.
//!
//! # The boundary is a blob
//!
//! Everything about how an entry's own state is laid out on disk lives here, in `s2n-quic-dc`, and
//! is **opaque to the embedding application** (SaltyLib-Rust). The extract side returns bytes
//! ([`Entry::to_persisted_bytes`]); the restore side takes bytes
//! ([`Map::insert_persisted`](crate::path::secret::map::Map::insert_persisted)). SaltyLib-Rust
//! frames, checksums, versions its file, and schedules writes around these blobs, but never inspects
//! or constructs their contents.
//!
//! This is deliberate. The blob is a *versioned format*: the layout below is what `V1` means, and it
//! must never drift. Keeping the codec next to the `Entry` fields it serialises means a change to any
//! of those fields (a key-schedule type, a new `Ciphersuite`, an added `Entry` field) is made by the
//! same people, in this file, next to the version byte -- and forces a conscious new version rather
//! than silently altering what `V1` means. The embedding application cannot desync from a layout it
//! never sees.
//!
//! # Observed values in, transforms on the way out
//!
//! The blob records values as *observed* on the live entry. The security-critical transforms --
//! advancing the sender and receiver counters so no key material is reused (see
//! [`sender::State::restore`](crate::path::secret::sender::State::restore) and
//! [`receiver::State::restore`](crate::path::secret::receiver::State::restore)), and rebuilding the
//! opaque `created_at` clock reading -- happen on load. So the file records the truth, and the
//! advance can be tuned without a format change.
//!
//! # Application data
//!
//! The blob carries **no** application data. It is an opaque `Arc<dyn Any>` the map cannot encode,
//! and the embedding application already holds both the entry and the concrete type. The application
//! writes and reads its own application data, and on load hands the resolved value back alongside the
//! blob (see [`Map::insert_persisted`](crate::path::secret::map::Map::insert_persisted)). `None` -- a
//! client-side entry, say -- is a normal state.

use crate::path::secret::{receiver, schedule, sender};
use s2n_quic_core::dc;
use std::{
    net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6},
    time::Instant,
};
use zeroize::Zeroizing;

use super::{entry::ApplicationData, Entry};

/// The layout version this build *writes*.
///
/// Bumped whenever the field region changes at all. A purely **additive** change (appending a field)
/// keeps [`MIN_READER_VERSION`] at the old value, so older readers still load the blob; an
/// incompatible change (reordering, resizing, removing, or repurposing an existing field) must bump
/// [`MIN_READER_VERSION`] to this value too.
const VERSION: u8 = 1;

/// The oldest reader version this build's blobs may be read by.
///
/// This is the mechanism behind the rollback guarantee: a team may deploy writer `N+1` and, if it
/// goes wrong, roll back to reader `N` **without losing their persisted keys**. For that to be safe,
/// an `N` reader handed an `N+1` blob must be able to read the fields it knows as a bit-for-bit
/// prefix and ignore anything appended. A writer promises exactly that by leaving
/// `MIN_READER_VERSION` at or below the reader's version.
///
/// **The invariant this asserts, which future edits MUST uphold:** a field may only ever be
/// *appended*, never reordered, resized, removed, or repurposed, without raising
/// `MIN_READER_VERSION` to the version that made the change. The prefix an older reader reads must
/// mean exactly what it meant in that older version. When that cannot be promised, bump this to
/// `VERSION`, and older readers will [reject the file](RestoreDisposition::RejectFile) and fall back
/// to an older generation (`R.REL4`) rather than misread key material.
const MIN_READER_VERSION: u8 = 1;

/// The reader version this build implements: the highest layout it knows how to parse in full. A
/// blob whose `min_reader_version` exceeds this is rejected. Equal to [`VERSION`] -- a build writes
/// the newest layout it understands.
const THIS_READER_VERSION: u8 = VERSION;

/// The wire encoding of [`endpoint::Type`] within the blob.
///
/// `endpoint::Type` has no stable integer representation of its own, so the mapping is pinned here
/// rather than relying on the enum's declaration order. Changing these values is a format change.
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

/// SaltyLib-Rust-tunable knobs for restoring an entry. `s2n-quic-dc` clamps each to its own security
/// floor, so a value that is too small (or zero) cannot defeat a key-reuse or replay invariant.
#[derive(Clone, Copy, Debug, Default)]
pub struct RestoreParams {
    /// How far to advance the persisted key-id counters, over and above the built-in floors.
    ///
    /// This exists because a persisted file lags the live state: between the last write and the
    /// restart, the sender may have issued more key ids and the receiver accepted more, none of them
    /// captured. The gap grows with the file's age, which only the embedding application knows (it
    /// holds the snapshot's creation time), so it supplies the estimate here.
    ///
    /// The advance is applied as `max(advance, floor)` to each counter, where the floor is the
    /// key-schedule minimum for the sender and the replay-window width for the receiver. Passing `0`
    /// falls back to those floors.
    pub advance: u64,
}

/// A version-1 snapshot of one map entry.
///
/// Private on purpose: this is the in-memory shape of the on-disk `V1` layout, and nothing outside
/// this module -- least of all the embedding application -- should depend on it. The public boundary
/// is the byte blob produced by [`encode`] and consumed by [`decode`]. Fields are pinned primitive
/// types (`[u8; 32]`, `u8`, `u64`, ...), never crate aliases, so the layout cannot change out from
/// under the version byte if a type elsewhere is redefined.
#[derive(Clone, Debug)]
struct PersistedStateV1 {
    peer: SocketAddr,
    export_secret: [u8; 32],
    endpoint: u8,
    ciphersuite: u8,
    stateless_reset: [u8; 16],
    /// Wall-clock seconds since the Unix epoch at which the entry was created. `Instant` is opaque
    /// and boot-relative, so it cannot be persisted directly; the wall clock is recorded and the
    /// `Instant` is rebuilt relative to `Instant::now()` on load.
    created_at_unix_secs: u64,
    /// The sender's next key id, as observed. The advance is applied on load.
    sender_current_id: u64,
    /// The receiver's maximum-seen key id, as observed, or `None` if it has accepted nothing. The
    /// advance is applied on load.
    receiver_max_seen_key_id: Option<u64>,
    /// The peer's flow-control limit, persisted rather than reconstructed so the restore path makes
    /// no assumption about fleet homogeneity (see the plan's OPEN-17).
    remote_max_data: u64,
}

/// What a loader should do when `insert_persisted` fails, so the decision lives next to the errors
/// that carry it rather than being re-derived at every call site.
///
/// A loader replaying a file of records maps each failure through
/// [`InsertPersistedError::disposition`] and acts on the result. `s2n-quic-dc` owns the format and
/// the invariants, so it owns this classification; the loader (SaltyLib-Rust, item B4) obeys it
/// without needing to know *why* a given error is fatal to a file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RestoreDisposition {
    /// The record decoded and its framing is intact, but this one entry is unusable. Skip it, count
    /// it (`restore.rejected`), and continue with the next record. This is the `R.REL1`/OPEN-12
    /// ceiling: the worst case is a smaller map, never a failed startup.
    SkipEntry,

    /// The byte stream is no longer trustworthy -- the length that frames the next record cannot be
    /// believed -- so stop reading this file but keep every record already accepted from it (the
    /// format's "replay stops at the first bad record" rule).
    StopFile,

    /// The file as a whole cannot be used. Reject it and fall back to an older generation
    /// (`R.REL4`).
    RejectFile,
}

/// Why decoding a blob or turning it back into a live entry failed.
///
/// Each variant carries a [`RestoreDisposition`] (see [`Self::disposition`]) telling a loader whether
/// to skip just this entry, stop reading the current file, or reject the file entirely. No path here
/// panics; nothing here aborts startup. A corrupt file must at worst leave the map empty (`R.REL1`).
#[derive(Debug, thiserror::Error)]
pub enum InsertPersistedError {
    /// The blob was shorter than its declared layout, or trailing bytes remained.
    #[error("persisted entry blob was truncated or malformed")]
    Malformed,

    /// The trailing CRC32 did not match the blob's bytes: a structurally-valid corruption (a flipped
    /// bit in a counter or the export secret) that the layout checks alone would have accepted. This
    /// is the check that keeps a corrupted sender/receiver counter from defeating `R.SEC1`/`R.SEC2`.
    #[error("persisted entry blob failed its checksum")]
    BadChecksum,

    /// The blob was written by a version that declared it needs a newer reader than this build.
    /// This is the rollback escape hatch: the file is not misread, it is rejected and an older
    /// generation is used instead.
    #[error(
        "persisted entry needs reader version {min_reader_version}, this build is {this_reader}"
    )]
    RequiresNewerReader {
        min_reader_version: u8,
        this_reader: u8,
    },

    /// The peer address tag byte named no known address family.
    #[error("invalid peer address tag {0}")]
    InvalidPeerTag(u8),

    /// The endpoint byte was neither the client nor the server value.
    #[error("invalid endpoint byte {0}")]
    InvalidEndpoint(u8),

    /// The ciphersuite byte named no known ciphersuite.
    #[error("invalid ciphersuite byte {0}")]
    InvalidCiphersuite(u8),

    /// The persisted receiver maximum was the reserved sentinel; see
    /// [`receiver::RestoreError::Sentinel`].
    #[error("receiver key id was the reserved sentinel value")]
    ReceiverSentinel,

    /// Restoring one of the counters would overflow the usable id space; see
    /// [`sender::RestoreError`] and [`receiver::RestoreError`].
    #[error("restored counter would overflow the usable id space")]
    CounterOverflow,

    /// An entry with this credential id is already present in the map. The credential id is derived
    /// from the export secret, so this means the same secret was inserted twice.
    #[error("an entry with this credential id already exists")]
    DuplicateCredentialId,
}

impl InsertPersistedError {
    /// How a loader should respond to this failure: skip the one entry, stop reading the current
    /// file, or reject the file entirely. See [`RestoreDisposition`].
    ///
    /// The match is exhaustive with no wildcard on purpose: a new error variant will not compile
    /// until it is classified here, so the disposition can never be left to a default.
    pub fn disposition(&self) -> RestoreDisposition {
        use RestoreDisposition::*;
        match self {
            // Framing intact, one entry unusable: skip and keep loading (OPEN-12).
            //
            // `DuplicateCredentialId` is normal, not a fault to stop on: after a failed write an
            // entry legitimately appears in two files of one generation, and the loader keeps the
            // later record and counts `restore.duplicates` (C9). Stopping here would break that
            // recovery path.
            Self::CounterOverflow
            | Self::ReceiverSentinel
            | Self::InvalidEndpoint(_)
            | Self::InvalidCiphersuite(_)
            | Self::InvalidPeerTag(_)
            | Self::DuplicateCredentialId => SkipEntry,

            // The bytes are corrupt, so the length framing the next record cannot be trusted. Stop
            // this file but keep everything already accepted from it.
            Self::Malformed | Self::BadChecksum => StopFile,

            // A whole-file property: the writer requires a newer reader than this build. Reject the
            // file and fall back to an older generation (R.REL4). This is the rollback escape hatch.
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
    /// Serialises this entry to an opaque, versioned byte blob for persistence.
    ///
    /// The bytes are self-describing and self-contained: the embedding application frames,
    /// checksums, and files them without inspecting their contents, and hands them back verbatim to
    /// [`Map::insert_persisted`](crate::path::secret::map::Map::insert_persisted) on load. Reads
    /// observed values only; the restore transforms are applied on the way back in.
    ///
    /// The result is [`Zeroizing`] because it contains raw key material (the export secret); the
    /// caller should keep it zeroizing until it is written and no longer needed.
    pub fn to_persisted_bytes(&self) -> Zeroizing<Vec<u8>> {
        let secret = self.secret();

        let state = PersistedStateV1 {
            peer: *self.peer(),
            export_secret: *secret.export_secret(),
            endpoint: endpoint_byte::encode(secret.endpoint()),
            ciphersuite: u8::from(*secret.ciphersuite()),
            stateless_reset: self.sender().stateless_reset,
            created_at_unix_secs: creation_time_to_unix_secs(self.creation_time()),
            sender_current_id: self.sender().current_id(),
            receiver_max_seen_key_id: self.receiver().max_seen_key_id(),
            remote_max_data: *self.parameters().remote_max_data,
        };

        Zeroizing::new(encode(&state))
    }

    /// Rebuilds an entry from a persisted blob and the application data resolved for it.
    ///
    /// The map treats application data as opaque, so decoding it into a concrete type is the caller's
    /// job: the loader decodes its own record and hands the resulting `Arc<dyn Any>` here. `None`
    /// means the entry had no application data, which is a normal state.
    ///
    /// `params` carries the caller's tunable advance; the security floors are applied inside the
    /// counter restores, so a caller cannot construct an entry that reuses key material.
    pub(super) fn restore(
        bytes: &[u8],
        application_data: Option<ApplicationData>,
        params: &RestoreParams,
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

        let sender = sender::State::restore(
            state.sender_current_id,
            state.stateless_reset,
            params.advance,
        )?;

        let receiver = match state.receiver_max_seen_key_id {
            Some(max_seen) => receiver::State::restore(max_seen, params.advance)?,
            None => receiver::State::new(),
        };

        // Reconstruct the application parameters from local limits, then apply the one peer-derived
        // value we persist. `remote_max_data` is a `VarInt`; a persisted value out of `VarInt` range
        // is clamped rather than rejected, since it only bounds flow control and a wrong bound costs
        // at most a stall, never key reuse.
        let mut parameters = dc::ApplicationParams::new(
            crate::stream::MAX_DATAGRAM_SIZE as u16,
            &Default::default(),
            &Default::default(),
        );
        parameters.remote_max_data = s2n_quic_core::varint::VarInt::new(state.remote_max_data)
            .unwrap_or(s2n_quic_core::varint::VarInt::MAX);

        // Rebuild the boot-relative `Instant` from the persisted wall clock. A creation time in the
        // future (clock skew across the restart) or unrepresentable as an offset collapses to
        // "now", which merely makes the entry look freshly created.
        let creation_time = unix_secs_to_creation_time(state.created_at_unix_secs);

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

/// Peer address family tags. Mirror the existing address-only serializer so the two agree.
mod peer_tag {
    pub(super) const V4: u8 = 0;
    pub(super) const V6_MINIMAL: u8 = 1;
    pub(super) const V6_FULL: u8 = 2;
}

/// Encodes a [`PersistedStateV1`] into the `V1` byte layout.
///
/// Layout (all integers little-endian):
/// ```text
/// version               u8    layout the writer used
/// min_reader_version    u8    oldest reader allowed to read this blob
/// field_region_len      u16   byte length of the field region that follows
/// --- field region (exactly field_region_len bytes) ---
/// peer                  tag u8 + address (see peer_tag)
/// export_secret         [u8; 32]
/// endpoint              u8
/// ciphersuite           u8
/// stateless_reset       [u8; 16]
/// created_at_unix_secs  u64
/// sender_current_id     u64
/// receiver_present      u8   (0 = None, 1 = Some)
/// receiver_max_seen     u64  (present only when receiver_present == 1)
/// remote_max_data       u64
/// --- end field region ---
/// crc32                 u32  (IEEE, over every preceding byte)
/// ```
///
/// A future version may only *append* fields to the region. An older reader reads the fields it
/// knows from the front and skips to `field_region_len`; the CRC covers the whole region. See
/// [`MIN_READER_VERSION`].
fn encode(state: &PersistedStateV1) -> Vec<u8> {
    // Build the field region on its own so its length can be measured and written into the header.
    let mut fields = Vec::with_capacity(96);
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

    fields.extend_from_slice(&state.export_secret);
    fields.push(state.endpoint);
    fields.push(state.ciphersuite);
    fields.extend_from_slice(&state.stateless_reset);
    fields.extend_from_slice(&state.created_at_unix_secs.to_le_bytes());
    fields.extend_from_slice(&state.sender_current_id.to_le_bytes());
    match state.receiver_max_seen_key_id {
        Some(max) => {
            fields.push(1);
            fields.extend_from_slice(&max.to_le_bytes());
        }
        None => fields.push(0),
    }
    fields.extend_from_slice(&state.remote_max_data.to_le_bytes());

    // The field region is far below the u16 ceiling; the ceiling is a format property, not a limit
    // we expect to approach.
    debug_assert!(fields.len() <= u16::MAX as usize);
    let field_region_len = fields.len() as u16;

    let mut out = Vec::with_capacity(4 + fields.len() + 4);
    out.push(VERSION);
    out.push(MIN_READER_VERSION);
    out.extend_from_slice(&field_region_len.to_le_bytes());
    out.extend_from_slice(&fields);

    // Trailing CRC32 over everything above, so a structurally-valid bit-flip (which the layout checks
    // would accept) is caught before it can rebuild an entry with a corrupted counter or secret.
    out.extend_from_slice(&crc32(&out).to_le_bytes());

    out
}

/// Decodes a blob. Rejects a bad checksum, a blob that needs a newer reader, a bad tag, or a short
/// buffer. Forwards compatible: a blob written by a newer version whose `min_reader_version` this
/// build satisfies is read by taking the known fields from the front of the field region and
/// skipping anything appended after them.
fn decode(bytes: &[u8]) -> Result<PersistedStateV1, InsertPersistedError> {
    // Split and verify the trailing CRC32 before parsing anything. A blob shorter than the checksum
    // itself is malformed; a checksum mismatch is a corruption the layout checks would not catch.
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

    // Header. `version` records which layout the writer used; `min_reader_version` is the gate that
    // makes rollback safe. If the writer says it needs a newer reader than this build, reject the
    // file (the loader falls back to an older generation) rather than risk misreading the layout.
    let _version = r.u8()?;
    let min_reader_version = r.u8()?;
    if min_reader_version > THIS_READER_VERSION {
        return Err(InsertPersistedError::RequiresNewerReader {
            min_reader_version,
            this_reader: THIS_READER_VERSION,
        });
    }

    // Carve out exactly the field region. A newer writer may have appended fields we do not know;
    // we read the ones we do from a cursor bounded to the region, and whatever remains in it is
    // skipped. Bytes between the region and the CRC are not allowed -- that is a malformed blob, not
    // a forwards-compatible one.
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

    let export_secret = r.array::<32>()?;
    let endpoint = r.u8()?;
    let ciphersuite = r.u8()?;
    let stateless_reset = r.array::<16>()?;
    let created_at_unix_secs = u64::from_le_bytes(r.array::<8>()?);
    let sender_current_id = u64::from_le_bytes(r.array::<8>()?);
    let receiver_max_seen_key_id = match r.u8()? {
        0 => None,
        1 => Some(u64::from_le_bytes(r.array::<8>()?)),
        _ => return Err(InsertPersistedError::Malformed),
    };
    let remote_max_data = u64::from_le_bytes(r.array::<8>()?);

    // Any bytes left in the field region are fields appended by a newer, compatible writer. We do
    // not know them, and the `min_reader_version` gate has already vouched that ignoring them is
    // safe, so we drop them rather than reject. (Bytes *outside* the region were already rejected
    // as malformed above.)

    Ok(PersistedStateV1 {
        peer,
        export_secret,
        endpoint,
        ciphersuite,
        stateless_reset,
        created_at_unix_secs,
        sender_current_id,
        receiver_max_seen_key_id,
        remote_max_data,
    })
}

/// A minimal forward cursor over a byte slice; every read is bounds-checked and maps a short buffer
/// to [`InsertPersistedError::Malformed`].
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

/// CRC32 (IEEE 802.3, reflected, polynomial `0xEDB88420`) over `bytes`.
///
/// Computed bit-by-bit rather than pulling in a dependency: the blob is ~100 bytes and this is off
/// any hot path (once per entry, at write and at load). The polynomial and reflection match
/// `crc32fast`, so a value computed here agrees with one SaltyLib-Rust might compute over the same
/// bytes with that crate.
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

/// Converts a creation-time `Instant` to wall-clock seconds since the Unix epoch for persistence.
///
/// `Instant` carries no wall-clock anchor, so this pairs the entry's age (`now - creation_time`)
/// with the current `SystemTime`. A tiny amount of skew between the two reads is immaterial at
/// second granularity.
fn creation_time_to_unix_secs(creation_time: Instant) -> u64 {
    let age = creation_time.elapsed();
    let created_wall = std::time::SystemTime::now().checked_sub(age);
    created_wall
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Rebuilds a creation-time `Instant` from persisted wall-clock seconds.
///
/// Returns an `Instant` equal to `Instant::now() - elapsed`, where `elapsed` is how long ago the
/// wall-clock time was. A time in the future, or one too far in the past to represent as an offset,
/// collapses to `Instant::now()`.
fn unix_secs_to_creation_time(created_at_unix_secs: u64) -> Instant {
    let now_wall = std::time::SystemTime::now();
    let created_wall = std::time::UNIX_EPOCH + std::time::Duration::from_secs(created_at_unix_secs);
    let elapsed = now_wall
        .duration_since(created_wall)
        .unwrap_or(std::time::Duration::ZERO);
    Instant::now()
        .checked_sub(elapsed)
        .unwrap_or_else(Instant::now)
}

/// Test-only access to the intermediate [`PersistedStateV1`], so tests can perturb a single field
/// before encoding without hand-building a whole blob.
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
    use s2n_quic_core::endpoint;
    use std::net::SocketAddr;

    const SENDER_FLOOR: u64 = 1 << 32;
    const RECEIVER_FLOOR: u64 = 1 << 32;

    fn peer() -> SocketAddr {
        "127.0.0.1:4433".parse().unwrap()
    }

    /// Builds a live entry with a known sender counter and receiver maximum, so the restore
    /// transforms can be asserted exactly.
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

    fn no_advance() -> RestoreParams {
        RestoreParams { advance: 0 }
    }

    /// Seals `msg` with `sealer`/`credentials` and opens it on `opener` through the public `Map`
    /// API, returning the decrypt result so a caller can assert success or a specific rejection.
    /// On success it also asserts the recovered plaintext matches. This is the one place the
    /// encrypt -> open_once -> decrypt_in_place dance lives.
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

    /// A send/receive/verify where `sealer_map` seals by credential id (`seal_once_id`, e.g. the
    /// datagram server's `encrypt_id`).
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

    /// A send/receive/verify where `sealer_map` seals by peer address (the datagram server's reply
    /// path: `get_tracked(addr).seal_once`).
    fn verify_one_by_addr(
        sealer_map: &crate::path::secret::map::Map,
        peer_addr: SocketAddr,
        opener: &crate::path::secret::map::Map,
        msg: &[u8],
    ) -> crate::crypto::open::Result {
        let peer = sealer_map
            .get_untracked(peer_addr)
            .expect("sealer map reachable by peer address");
        let (sealer, credentials, _params) = peer.seal_once();
        verify_one(&sealer, &credentials, opener, msg)
    }

    /// Sends a random number (1..=9) of datagrams by peer address, asserting each is decrypted, to
    /// warm the opener's receiver to a non-trivial `max_seen` before a persist/restore. The exact
    /// count does not matter: the tests assert behaviour (reject then recover), not a number.
    /// This is used in the warm-up phase of the test, not verification, so there is no matching
    /// send_random_by_id.
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

    /// Asserts a `verify_one_*` result was rejected specifically because the key id was too old --
    /// below the restored receiver's advanced maximum, outside the replay window. This is the
    /// distinct signal that the receiver advance did its job (vs. `InvalidTag` for broken keys, or
    /// `ReplayDefinitelyDetected` for a within-window replay).
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
    fn sender_counter_advances_by_floor() {
        let bytes = live_entry(10, None).to_persisted_bytes();
        let restored = Entry::restore(&bytes, None, &no_advance()).unwrap();
        assert_eq!(restored.sender().current_id(), 10 + SENDER_FLOOR);
    }

    #[test]
    fn receiver_max_advances_by_floor() {
        let bytes = live_entry(0, Some(500)).to_persisted_bytes();
        let restored = Entry::restore(&bytes, None, &no_advance()).unwrap();
        assert_eq!(
            restored.receiver().max_seen_key_id(),
            Some(500 + RECEIVER_FLOOR)
        );
    }

    #[test]
    fn advance_over_floor_is_honoured() {
        let advance = 5u64 << 32;
        let params = RestoreParams { advance };

        let sender_bytes = live_entry(10, None).to_persisted_bytes();
        let restored = Entry::restore(&sender_bytes, None, &params).unwrap();
        assert_eq!(restored.sender().current_id(), 10 + advance);

        let receiver_bytes = live_entry(0, Some(500)).to_persisted_bytes();
        let restored = Entry::restore(&receiver_bytes, None, &params).unwrap();
        assert_eq!(restored.receiver().max_seen_key_id(), Some(500 + advance));
    }

    #[test]
    fn advance_below_floor_is_clamped_up() {
        // A caller passing a too-small advance still gets at least the floor -- the security
        // property the floor exists to guarantee.
        let params = RestoreParams { advance: 1 };
        let bytes = live_entry(10, None).to_persisted_bytes();
        let restored = Entry::restore(&bytes, None, &params).unwrap();
        assert_eq!(restored.sender().current_id(), 10 + SENDER_FLOOR);
    }

    #[test]
    fn receiver_never_seen_restores_as_fresh() {
        let bytes = live_entry(0, None).to_persisted_bytes();
        let restored = Entry::restore(&bytes, None, &no_advance()).unwrap();
        assert_eq!(restored.receiver().max_seen_key_id(), None);
    }

    #[test]
    fn restored_entry_rejects_replay_of_pre_restart_key_id() {
        let bytes = live_entry(0, Some(500)).to_persisted_bytes();
        let restored = Entry::restore(&bytes, None, &no_advance()).unwrap();

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
        let restored = Entry::restore(&bytes, None, &no_advance()).unwrap();
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
        let restored = Entry::restore(&bytes, None, &no_advance()).unwrap();
        assert_eq!(restored.id(), &id);
    }

    #[test]
    fn newer_reader_required_is_rejected() {
        // A blob whose min_reader_version exceeds this build must be rejected (RejectFile), not
        // misread -- this is the rollback escape hatch. min_reader_version is the second byte.
        let mut bytes = live_entry(0, None).to_persisted_bytes().to_vec();
        bytes.truncate(bytes.len() - 4);
        bytes[1] = THIS_READER_VERSION + 1;
        let crc = crc32(&bytes);
        bytes.extend_from_slice(&crc.to_le_bytes());
        assert!(matches!(
            Entry::restore(&bytes, None, &no_advance()),
            Err(InsertPersistedError::RequiresNewerReader { .. })
        ));
    }

    #[test]
    fn forwards_compatible_additive_blob_loads() {
        // Simulate a blob written by a newer, *compatible* version (N+1): a higher `version`, extra
        // fields appended inside an enlarged field region, and min_reader_version left at this
        // build's version. This build must read the fields it knows and ignore the appended tail --
        // the rollback guarantee, in reverse.
        let entry = live_entry(7, Some(42));
        let expected_id = *entry.id();
        let good = encode(&entry.persisted_state_for_test());

        // Strip CRC, then rebuild the header with version+1 and a field region enlarged by some
        // trailing "future field" bytes.
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

        let restored = Entry::restore(&blob, None, &no_advance()).unwrap();
        assert_eq!(restored.id(), &expected_id);
        assert_eq!(restored.sender().current_id(), 7 + SENDER_FLOOR);
        assert_eq!(
            restored.receiver().max_seen_key_id(),
            Some(42 + RECEIVER_FLOOR)
        );
    }

    #[test]
    fn bytes_between_region_and_crc_are_malformed() {
        // Trailing bytes *outside* the declared field region (between it and the CRC) are not a
        // forwards-compatible append; they are a malformed blob.
        let good = encode(&live_entry(0, None).persisted_state_for_test());
        let body = &good[..good.len() - 4];
        let mut blob = body.to_vec();
        blob.push(0x00); // one byte past the field region
        let crc = crc32(&blob);
        blob.extend_from_slice(&crc.to_le_bytes());
        assert!(matches!(
            Entry::restore(&blob, None, &no_advance()),
            Err(InsertPersistedError::Malformed)
        ));
    }

    #[test]
    fn truncated_blob_is_rejected() {
        let bytes = live_entry(0, None).to_persisted_bytes();
        // Drop the last few bytes: the layout no longer matches and the CRC region is wrong.
        let truncated = &bytes[..bytes.len() - 6];
        assert!(matches!(
            Entry::restore(truncated, None, &no_advance()),
            Err(InsertPersistedError::Malformed | InsertPersistedError::BadChecksum)
        ));
    }

    #[test]
    fn trailing_bytes_are_rejected() {
        let mut bytes = live_entry(0, None).to_persisted_bytes().to_vec();
        bytes.push(0);
        // The appended byte shifts what is read as the trailing CRC, so this now fails the checksum.
        assert!(matches!(
            Entry::restore(&bytes, None, &no_advance()),
            Err(InsertPersistedError::BadChecksum)
        ));
    }

    #[test]
    fn bad_checksum_is_rejected() {
        // Flip a byte in the payload but leave the trailing CRC alone: a structurally-valid
        // corruption that only the checksum catches. Byte 10 is inside the field region.
        let mut bytes = live_entry(0, None).to_persisted_bytes().to_vec();
        bytes[10] ^= 0xFF;
        assert!(matches!(
            Entry::restore(&bytes, None, &no_advance()),
            Err(InsertPersistedError::BadChecksum)
        ));
    }

    #[test]
    fn crc32_matches_known_vector() {
        // IEEE CRC32 of "123456789" is 0xCBF43926 -- the standard check value. Guards against a
        // polynomial or reflection mistake that would still round-trip with itself.
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn error_dispositions_are_classified() {
        use InsertPersistedError as E;
        use RestoreDisposition::*;

        // Per-entry: framing intact, skip and continue.
        assert_eq!(E::CounterOverflow.disposition(), SkipEntry);
        assert_eq!(E::ReceiverSentinel.disposition(), SkipEntry);
        assert_eq!(E::InvalidEndpoint(9).disposition(), SkipEntry);
        assert_eq!(E::InvalidCiphersuite(9).disposition(), SkipEntry);
        assert_eq!(E::InvalidPeerTag(9).disposition(), SkipEntry);
        assert_eq!(E::DuplicateCredentialId.disposition(), SkipEntry);

        // Byte stream no longer trustworthy: stop this file, keep prior records.
        assert_eq!(E::Malformed.disposition(), StopFile);
        assert_eq!(E::BadChecksum.disposition(), StopFile);

        // Whole file unusable: reject and fall back a generation.
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
            Entry::restore(&bytes, None, &no_advance()),
            Err(InsertPersistedError::InvalidEndpoint(200))
        ));
    }

    #[test]
    fn invalid_ciphersuite_byte_is_rejected() {
        let mut state = live_entry(0, None).persisted_state_for_test();
        state.ciphersuite = 200;
        let bytes = encode(&state);
        assert!(matches!(
            Entry::restore(&bytes, None, &no_advance()),
            Err(InsertPersistedError::InvalidCiphersuite(200))
        ));
    }

    #[test]
    fn invalid_peer_tag_is_rejected() {
        let state = live_entry(0, None).persisted_state_for_test();
        let mut bytes = encode(&state);
        // Corrupt the peer tag (the first byte of the field region) and re-seal the CRC, so the blob is
        // structurally reachable and fails on the tag rather than on the checksum.
        bytes.truncate(bytes.len() - 4);
        bytes[4] = 200;
        let crc = crc32(&bytes);
        bytes.extend_from_slice(&crc.to_le_bytes());
        assert!(matches!(
            Entry::restore(&bytes, None, &no_advance()),
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
            Entry::restore(&bytes, None, &no_advance()),
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
            Entry::restore(&bytes, None, &no_advance()),
            Err(InsertPersistedError::CounterOverflow)
        ));
    }

    #[test]
    fn remote_max_data_is_persisted() {
        let mut state = live_entry(0, None).persisted_state_for_test();
        state.remote_max_data = 4242;
        let bytes = encode(&state);
        let restored = Entry::restore(&bytes, None, &no_advance()).unwrap();
        assert_eq!(*restored.parameters().remote_max_data, 4242);
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

        let restored = map.insert_persisted(&bytes, None, &no_advance()).unwrap();
        assert_eq!(restored.id(), &id);

        assert!(map.contains(&peer()));
        assert!(map.get_untracked(peer()).is_some());

        let (_sealer, credentials, _params) = map.seal_once_id(id).expect("entry reachable by id");
        assert_eq!(credentials.id, id);
    }

    #[test]
    fn restored_entry_produces_working_sealer_and_opener() {
        // The strongest reachability check, driven entirely through the public `Map` API: a
        // restored entry must produce a sealer/opener that interoperate with a live peer. Build a
        // matching client/server pair (one shared export secret), persist the *server* side, restore
        // it into a fresh server map, then seal on the client and open on the restored server.
        let client = test_map();
        let server = test_map();
        let client_addr = "127.0.0.1:1234".parse().unwrap();
        let server_addr = "127.0.0.1:5678".parse().unwrap();
        let id = client.test_insert_pair(client_addr, None, &server, server_addr, None);

        // Persist the server-side entry and restore it into a brand-new server map.
        let server_entry = server.store.get_by_id_untracked(&id).unwrap();
        let bytes = server_entry.to_persisted_bytes();
        let restored_server = test_map();
        restored_server
            .insert_persisted(&bytes, None, &no_advance())
            .unwrap();

        // Forward: client seals by id -> restored server opens.
        verify_one_by_id(&client, id, &restored_server, b"hello from client")
            .expect("restored server decrypts the client's datagram");

        // Return: restored server seals by id -> client opens. Proves both directions.
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
        // Companion to the id-based reachability test, exercising the *address* index of a restored
        // entry through real crypto. On the server, an entry is stored under its peer's (the
        // client's) address -- that is how an initiator finds an existing secret before opening a
        // stream. So: restore the server entry, look it up by the client's address, seal with it,
        // and have the client open by id.
        let client = test_map();
        let server = test_map();
        let client_addr = "127.0.0.1:1234".parse().unwrap();
        let server_addr = "127.0.0.1:5678".parse().unwrap();
        let id = client.test_insert_pair(client_addr, None, &server, server_addr, None);

        // `test_insert_pair` stores the server-role entry under the client's address (the peer it
        // talks to). Persist it and restore into a fresh server map.
        let server_entry = server.store.get_by_id_untracked(&id).unwrap();
        let bytes = server_entry.to_persisted_bytes();
        let restored_server = test_map();
        restored_server
            .insert_persisted(&bytes, None, &no_advance())
            .unwrap();

        // The restored entry must be findable by the client's address (the address index).
        assert_eq!(
            restored_server.get_untracked(client_addr).unwrap().id(),
            &id
        );

        // Forward: restored server, found by the client's address, seals -> client opens by id.
        verify_one_by_addr(
            &restored_server,
            client_addr,
            &client,
            b"hello from restored server",
        )
        .expect("client decrypts the restored server's datagram");

        // Return: client, found by the server's address, seals -> restored server opens by id.
        verify_one_by_addr(
            &client,
            server_addr,
            &restored_server,
            b"hello back from client",
        )
        .expect("restored server decrypts the client's reply");
    }

    #[test]
    fn restored_client_entry_produces_working_sealer_and_opener() {
        // As `restored_entry_produces_working_sealer_and_opener`, but the *client* side is the one
        // persisted and restored. A client-side entry carries no application data, so this also
        // covers that case. Round trip is driven by id through the public `Map` API.
        let client = test_map();
        let server = test_map();
        let client_addr = "127.0.0.1:1234".parse().unwrap();
        let server_addr = "127.0.0.1:5678".parse().unwrap();
        let id = client.test_insert_pair(client_addr, None, &server, server_addr, None);

        // Persist the client-side entry and restore it into a brand-new client map.
        let client_entry = client.store.get_by_id_untracked(&id).unwrap();
        let bytes = client_entry.to_persisted_bytes();
        let restored_client = test_map();
        restored_client
            .insert_persisted(&bytes, None, &no_advance())
            .unwrap();

        // Forward: restored client seals by id -> server opens.
        verify_one_by_id(&restored_client, id, &server, b"hello from restored client")
            .expect("server decrypts the restored client's datagram");

        // Return: server seals by id -> restored client opens.
        verify_one_by_id(&server, id, &restored_client, b"hello back from server")
            .expect("restored client decrypts the server's reply");
    }

    #[test]
    fn restored_client_entry_is_reachable_by_peer_address_for_crypto() {
        // As `restored_entry_is_reachable_by_peer_address_for_crypto`, but the *client* side is
        // persisted and restored. The client-role entry is stored under its peer's (the server's)
        // address, so the restored client is looked up by the server's address.
        let client = test_map();
        let server = test_map();
        let client_addr = "127.0.0.1:1234".parse().unwrap();
        let server_addr = "127.0.0.1:5678".parse().unwrap();
        let id = client.test_insert_pair(client_addr, None, &server, server_addr, None);

        let client_entry = client.store.get_by_id_untracked(&id).unwrap();
        let bytes = client_entry.to_persisted_bytes();
        let restored_client = test_map();
        restored_client
            .insert_persisted(&bytes, None, &no_advance())
            .unwrap();

        // The restored client entry must be findable by the server's address.
        assert_eq!(
            restored_client.get_untracked(server_addr).unwrap().id(),
            &id
        );

        // Forward: restored client, found by the server's address, seals -> server opens by id.
        verify_one_by_addr(
            &restored_client,
            server_addr,
            &server,
            b"hello from restored client",
        )
        .expect("server decrypts the restored client's datagram");

        // Return: server, found by the client's address, seals -> restored client opens by id.
        verify_one_by_addr(
            &server,
            client_addr,
            &restored_client,
            b"hello back from server",
        )
        .expect("restored client decrypts the server's reply");
    }

    #[test]
    fn both_restored_entries_produce_working_sealer_and_opener() {
        // Both sides persisted and restored into fresh maps, then a full round trip between the two
        // restored entries -- the restart case where neither peer kept its live state. Driven by id.
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
            .insert_persisted(&client_bytes, None, &no_advance())
            .unwrap();
        restored_server
            .insert_persisted(&server_bytes, None, &no_advance())
            .unwrap();

        // Forward: restored client seals by id -> restored server opens.
        verify_one_by_id(
            &restored_client,
            id,
            &restored_server,
            b"hello between restored peers",
        )
        .expect("restored server decrypts the restored client's datagram");

        // Return: restored server seals by id -> restored client opens.
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
        // Both sides persisted and restored, round trip driven by the address index on each side:
        // each restored entry is found by its peer's address.
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
            .insert_persisted(&client_bytes, None, &no_advance())
            .unwrap();
        restored_server
            .insert_persisted(&server_bytes, None, &no_advance())
            .unwrap();

        // Each restored entry must be findable by its peer's address.
        assert_eq!(
            restored_client.get_untracked(server_addr).unwrap().id(),
            &id
        );
        assert_eq!(
            restored_server.get_untracked(client_addr).unwrap().id(),
            &id
        );

        // Forward: restored client, found by the server's address, seals -> restored server opens.
        verify_one_by_addr(
            &restored_client,
            server_addr,
            &restored_server,
            b"hello between restored peers",
        )
        .expect("restored server decrypts the restored client's datagram");

        // Return: restored server, found by the client's address, seals -> restored client opens.
        verify_one_by_addr(
            &restored_server,
            client_addr,
            &restored_client,
            b"reply between restored peers",
        )
        .expect("restored client decrypts the restored server's reply");
    }

    // The following six tests cover the reactive (no-Peer_HI) StaleKey recovery for every
    // production restore shape {server, client, both} x lookup path {id, addr}. Each:
    //   1. warms up both receivers with a random number of datagrams (so max_seen is non-sentinel),
    //   2. persists and restores the relevant side(s), advancing the restored receiver by 2^32,
    //   3. asserts the peer's next datagram is REJECTED (its key id is far below the advanced max),
    //   4. simulates the StaleKey the receiver would send, bumping the peer's sender forward, and
    //   5. asserts the peer's next datagram now DECRYPTS.
    // The exact max_seen value is not asserted (that exactness is covered by the *_advances_by_floor
    // unit tests); these verify the observable reject-then-recover behaviour end to end.

    fn warm_up_both_directions(
        client: &crate::path::secret::map::Map,
        client_addr: SocketAddr,
        server: &crate::path::secret::map::Map,
        server_addr: SocketAddr,
    ) {
        send_random_by_addr(client, server_addr, server); // client -> server
        send_random_by_addr(server, client_addr, client); // server -> client
    }

    /// Reads the `minimum_unseen_key_id` the restored receiver would advertise in a StaleKey, and
    /// bumps the peer's sender to it -- the effect of the peer receiving that StaleKey.
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
            .insert_persisted(&bytes, None, &no_advance())
            .unwrap();

        // The restored receiver advanced, so nothing was lost as the sentinel.
        assert!(restored_server
            .store
            .get_by_id_untracked(&id)
            .unwrap()
            .receiver()
            .max_seen_key_id()
            .is_some());

        // First packet from the client is rejected (its key id is below the advanced max).
        assert_rejected_too_old(verify_one_by_id(&client, id, &restored_server, b"first"));

        // StaleKey bumps the client's sender; the next packet decrypts.
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
            .insert_persisted(&bytes, None, &no_advance())
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
            .insert_persisted(&bytes, None, &no_advance())
            .unwrap();

        // The server (peer) is rejected by the restored client, then recovers via StaleKey.
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
            .insert_persisted(&bytes, None, &no_advance())
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
        // When *both* sides restart, both counters advance by 2^32 in lockstep: the restored
        // sender seals at ~2^32, which is already at/above the restored receiver's advanced max, so
        // there is no rejection and no StaleKey recovery is needed. (Asymmetric restart -- only the
        // receiver restored -- is what triggers rejection; see the server/client tests above.)
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
            .insert_persisted(&client_bytes, None, &no_advance())
            .unwrap();
        restored_server
            .insert_persisted(&server_bytes, None, &no_advance())
            .unwrap();

        // First packet already decrypts: both advanced together, so no gap to recover from.
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
            .insert_persisted(&client_bytes, None, &no_advance())
            .unwrap();
        restored_server
            .insert_persisted(&server_bytes, None, &no_advance())
            .unwrap();

        verify_one_by_addr(&restored_client, server_addr, &restored_server, b"first")
            .expect("restored client's first packet decrypts without any recovery");
    }

    #[test]
    fn duplicate_credential_id_is_rejected_not_panicked() {
        let map = test_map();
        let bytes = live_entry(0, None).to_persisted_bytes();

        map.insert_persisted(&bytes, None, &no_advance()).unwrap();
        assert!(matches!(
            map.insert_persisted(&bytes, None, &no_advance()),
            Err(InsertPersistedError::DuplicateCredentialId)
        ));
    }
}

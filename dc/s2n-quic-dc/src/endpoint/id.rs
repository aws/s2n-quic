// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Newtype wrappers for sender and worker identifiers.
//!
//! The endpoint pipeline has several distinct ID roles that were previously all typed as
//! `VarInt` or `usize`. This module introduces compile-time-distinct newtypes so that:
//!
//! - A local sender ID cannot be accidentally used where a remote sender ID is expected
//! - A sender index cannot be confused with a worker ID
//! - Wire-level encoding/decoding sites are explicit about which role they're handling
//!
//! ## ID Roles
//!
//! - [`SenderIdx`]: Index into the local send cache array. Assigned at send context
//!   creation. Each send socket/worker has a unique `SenderIdx`. This value is also used
//!   as `source_sender_id` on the wire (via conversion to [`LocalSenderId`]).
//!
//! - [`LocalSenderId`]: Our own sender identity as seen on the wire. When we send a
//!   packet, our `SenderIdx` is encoded as `source_sender_id` in the packet header.
//!   Peers read this and echo it back as `dest_sender_id` in ACK packets.
//!
//! - [`RemoteSenderId`]: The peer's sender identity read from an incoming packet's
//!   `source_sender_id` field. We store this and write it into ACK headers as
//!   `dest_sender_id` so the peer can route the ACK to the correct loss detector.

use s2n_codec::{Encoder, EncoderValue};
use s2n_quic_core::varint::VarInt;

// ── Sender IDs ──────────────────────────────────────────────────────────────

/// Index into the local send cache array. Each send socket has a unique `SenderIdx`.
///
/// This is an internal routing concept — it determines which send::Cache and Assembler
/// own a given flow. On the wire, this value is transmitted as a [`LocalSenderId`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SenderIdx(usize);

impl SenderIdx {
    #[inline]
    pub const fn new(idx: usize) -> Self {
        Self(idx)
    }

    #[inline]
    pub const fn as_usize(self) -> usize {
        self.0
    }

    #[inline]
    pub fn to_local_sender_id(self) -> LocalSenderId {
        LocalSenderId(VarInt::new(self.0 as u64).unwrap())
    }
}

impl core::fmt::Display for SenderIdx {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<SenderIdx> for usize {
    #[inline]
    fn from(idx: SenderIdx) -> usize {
        idx.0
    }
}

impl<T> core::ops::Index<SenderIdx> for [T] {
    type Output = T;
    #[inline]
    fn index(&self, idx: SenderIdx) -> &T {
        &self[idx.0]
    }
}

impl<T> core::ops::IndexMut<SenderIdx> for [T] {
    #[inline]
    fn index_mut(&mut self, idx: SenderIdx) -> &mut T {
        &mut self[idx.0]
    }
}

impl<T> core::ops::Index<SenderIdx> for Vec<T> {
    type Output = T;
    #[inline]
    fn index(&self, idx: SenderIdx) -> &T {
        &self.as_slice()[idx]
    }
}

impl<T> core::ops::IndexMut<SenderIdx> for Vec<T> {
    #[inline]
    fn index_mut(&mut self, idx: SenderIdx) -> &mut T {
        &mut self.as_mut_slice()[idx]
    }
}


/// Our own sender identity on the wire.
///
/// Encoded as `source_sender_id` in outgoing data/control packets. When a peer receives
/// our packet, they store this as their [`RemoteSenderId`] and echo it back in ACK
/// `dest_sender_id` fields so we can route the ACK to the correct send worker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LocalSenderId(VarInt);

impl LocalSenderId {
    #[inline]
    pub fn new(v: VarInt) -> Self {
        Self(v)
    }

    #[inline]
    pub fn as_varint(self) -> VarInt {
        self.0
    }

    #[inline]
    pub fn to_sender_idx(self) -> SenderIdx {
        SenderIdx(self.0.as_u64() as usize)
    }
}

impl core::fmt::Display for LocalSenderId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0.as_u64())
    }
}

impl EncoderValue for LocalSenderId {
    #[inline]
    fn encode<E: Encoder>(&self, encoder: &mut E) {
        self.0.encode(encoder);
    }
}


/// The peer's sender identity read from an incoming packet.
///
/// This is the `source_sender_id` from a received packet — it identifies which send
/// worker on the REMOTE host sent the packet. We echo this back as `dest_sender_id`
/// in ACK frames so the remote can route the ACK to its loss detection context.
///
/// IMPORTANT: This must NOT be confused with [`LocalSenderId`]. A `RemoteSenderId`
/// is always written into outgoing ACK headers as-is — never hashed or re-routed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RemoteSenderId(VarInt);

impl RemoteSenderId {
    #[inline]
    pub fn new(v: VarInt) -> Self {
        Self(v)
    }

    #[inline]
    pub fn as_varint(self) -> VarInt {
        self.0
    }
}

impl core::fmt::Display for RemoteSenderId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0.as_u64())
    }
}

impl EncoderValue for RemoteSenderId {
    #[inline]
    fn encode<E: Encoder>(&self, encoder: &mut E) {
        self.0.encode(encoder);
    }
}


/// Index of a send socket within a single send worker.
///
/// A send worker owns multiple sockets; `LocalSocketId` distinguishes them
/// within that worker. This is the second step of the two-step lookup:
/// `SenderIdx` → `LocalSocketId` → `send::Cache`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LocalSocketId(usize);

impl LocalSocketId {
    #[inline]
    pub const fn new(id: usize) -> Self {
        Self(id)
    }

    #[inline]
    pub const fn as_usize(self) -> usize {
        self.0
    }
}

impl From<LocalSocketId> for usize {
    #[inline]
    fn from(id: LocalSocketId) -> usize {
        id.0
    }
}

impl core::fmt::Display for LocalSocketId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl<T> core::ops::Index<LocalSocketId> for [T] {
    type Output = T;
    #[inline]
    fn index(&self, idx: LocalSocketId) -> &T {
        &self[idx.0]
    }
}

impl<T> core::ops::IndexMut<LocalSocketId> for [T] {
    #[inline]
    fn index_mut(&mut self, idx: LocalSocketId) -> &mut T {
        &mut self[idx.0]
    }
}

impl<T> core::ops::Index<LocalSocketId> for Vec<T> {
    type Output = T;
    #[inline]
    fn index(&self, idx: LocalSocketId) -> &T {
        &self.as_slice()[idx]
    }
}

impl<T> core::ops::IndexMut<LocalSocketId> for Vec<T> {
    #[inline]
    fn index_mut(&mut self, idx: LocalSocketId) -> &mut T {
        &mut self.as_mut_slice()[idx]
    }
}

// ── Worker IDs ──────────────────────────────────────────────────────────────
//
// Each worker type gets its own newtype so you can't accidentally pass a
// recv dispatch worker index where a send worker index is expected.

macro_rules! worker_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub struct $name(usize);

        impl $name {
            #[inline]
            pub const fn new(id: usize) -> Self {
                Self(id)
            }

            #[inline]
            pub const fn as_usize(self) -> usize {
                self.0
            }
        }

        impl From<$name> for usize {
            #[inline]
            fn from(id: $name) -> usize {
                id.0
            }
        }

        impl core::fmt::Display for $name {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                write!(f, "{}", self.0)
            }
        }

        impl<T> core::ops::Index<$name> for [T] {
            type Output = T;
            #[inline]
            fn index(&self, idx: $name) -> &T {
                &self[idx.0]
            }
        }

        impl<T> core::ops::IndexMut<$name> for [T] {
            #[inline]
            fn index_mut(&mut self, idx: $name) -> &mut T {
                &mut self[idx.0]
            }
        }

        impl<T> core::ops::Index<$name> for Vec<T> {
            type Output = T;
            #[inline]
            fn index(&self, idx: $name) -> &T {
                &self.as_slice()[idx]
            }
        }

        impl<T> core::ops::IndexMut<$name> for Vec<T> {
            #[inline]
            fn index_mut(&mut self, idx: $name) -> &mut T {
                &mut self.as_mut_slice()[idx]
            }
        }

    };
}

worker_id! {
    /// Index of a send worker thread.
    ///
    /// Send workers own send::Cache instances, assemblers, PTO wheels, and TX wheels.
    /// Each send worker manages one or more send sockets (indexed by [`SenderIdx`]).
    SendWorkerId
}

worker_id! {
    /// Index of a recv IO worker thread.
    ///
    /// Recv IO workers read packets from sockets and route them to recv dispatch
    /// workers based on `(credentials, source_sender_id)` hashing.
    RecvIoWorkerId
}

worker_id! {
    /// Index of a recv dispatch worker thread.
    ///
    /// Recv dispatch workers decrypt packets, manage recv::Cache/Context,
    /// generate ACKs, and dispatch frames to the frame dispatch worker.
    RecvDispatchWorkerId
}

worker_id! {
    /// Index of a frame dispatch worker thread.
    ///
    /// Frame dispatch workers route decoded frames to acceptors and stream queues.
    FrameDispatchWorkerId
}

/// Generic worker ID for cases where the specific worker type isn't yet distinguished.
///
/// Prefer the specific types above when the worker role is known.
/// This exists as a migration aid — new code should use the specific types.
#[deprecated(note = "use a specific worker ID type (SendWorkerId, RecvDispatchWorkerId, etc.)")]
pub type WorkerId = RecvDispatchWorkerId;

// ── Typed ID Mapping ────────────────────────────────────────────────────────

/// A typed lookup table mapping one ID type to another.
///
/// Replaces raw `Vec<usize>` with compile-time-checked indexing. The key type
/// (`K`) is used as the index; the value type (`V`) is returned from lookups.
///
/// # Example
///
/// ```ignore
/// let map: IdMap<SenderIdx, usize> = IdMap::new(64, usize::MAX);
/// map[SenderIdx::new(3)] = 7;
/// assert_eq!(map[SenderIdx::new(3)], 7);
/// ```
#[derive(Clone)]
pub struct IdMap<K, V> {
    values: Vec<V>,
    _key: core::marker::PhantomData<fn(K) -> K>,
}

impl<K, V: Clone> IdMap<K, V> {
    /// Create a new map with `len` slots, all initialized to `default`.
    pub fn new(len: usize, default: V) -> Self {
        Self {
            values: vec![default; len],
            _key: core::marker::PhantomData,
        }
    }
}

impl<K, V: Default + Clone> IdMap<K, V> {
    pub fn with_default(len: usize) -> Self {
        Self {
            values: vec![V::default(); len],
            _key: core::marker::PhantomData,
        }
    }
}

impl<K, V> IdMap<K, V> {
    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    pub fn get(&self, key: K) -> Option<&V>
    where
        K: Into<usize>,
    {
        self.values.get(key.into())
    }

    pub fn get_mut(&mut self, key: K) -> Option<&mut V>
    where
        K: Into<usize>,
    {
        self.values.get_mut(key.into())
    }

    pub fn iter(&self) -> impl Iterator<Item = &V> {
        self.values.iter()
    }
}

impl<K: Into<usize>, V> core::ops::Index<K> for IdMap<K, V> {
    type Output = V;
    #[inline]
    fn index(&self, idx: K) -> &V {
        &self.values[idx.into()]
    }
}

impl<K: Into<usize>, V> core::ops::IndexMut<K> for IdMap<K, V> {
    #[inline]
    fn index_mut(&mut self, idx: K) -> &mut V {
        &mut self.values[idx.into()]
    }
}

impl<'a, K, V> IntoIterator for &'a IdMap<K, V> {
    type Item = &'a V;
    type IntoIter = core::slice::Iter<'a, V>;

    fn into_iter(self) -> Self::IntoIter {
        self.values.iter()
    }
}

impl<'a, K, V> IntoIterator for &'a mut IdMap<K, V> {
    type Item = &'a mut V;
    type IntoIter = core::slice::IterMut<'a, V>;

    fn into_iter(self) -> Self::IntoIter {
        self.values.iter_mut()
    }
}

impl<K, V> From<Vec<V>> for IdMap<K, V> {
    fn from(values: Vec<V>) -> Self {
        Self {
            values,
            _key: core::marker::PhantomData,
        }
    }
}

impl<K, V> core::iter::FromIterator<V> for IdMap<K, V> {
    fn from_iter<I: IntoIterator<Item = V>>(iter: I) -> Self {
        Self {
            values: iter.into_iter().collect(),
            _key: core::marker::PhantomData,
        }
    }
}

impl<K, V: core::fmt::Debug> core::fmt::Debug for IdMap<K, V> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_list().entries(self.values.iter()).finish()
    }
}

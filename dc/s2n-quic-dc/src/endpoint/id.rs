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

/// Our own sender identity on the wire.
///
/// Encoded as `source_sender_id` in outgoing data/control packets. When a peer receives
/// our packet, they store this as their [`RemoteSenderId`] and echo it back in ACK
/// `dest_sender_id` fields so we can route the ACK to the correct send worker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LocalSenderId(VarInt);

impl LocalSenderId {
    /// Sentinel value for an unspecified sender ID
    pub const UNSPECIFIED: Self = Self(VarInt::MAX);

    #[inline]
    pub fn new(v: VarInt) -> Self {
        Self(v)
    }

    #[inline]
    pub fn as_varint(self) -> VarInt {
        self.0
    }

    #[inline]
    pub fn as_usize(self) -> usize {
        self.0.as_u64() as _
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

impl From<LocalSenderId> for usize {
    #[inline]
    fn from(idx: LocalSenderId) -> usize {
        idx.as_usize()
    }
}

impl<T> core::ops::Index<LocalSenderId> for [T] {
    type Output = T;
    #[inline]
    fn index(&self, idx: LocalSenderId) -> &T {
        &self[idx.as_usize()]
    }
}

impl<T> core::ops::IndexMut<LocalSenderId> for [T] {
    #[inline]
    fn index_mut(&mut self, idx: LocalSenderId) -> &mut T {
        &mut self[idx.as_usize()]
    }
}

impl<T> core::ops::Index<LocalSenderId> for Vec<T> {
    type Output = T;
    #[inline]
    fn index(&self, idx: LocalSenderId) -> &T {
        &self.as_slice()[idx]
    }
}

impl<T> core::ops::IndexMut<LocalSenderId> for Vec<T> {
    #[inline]
    fn index_mut(&mut self, idx: LocalSenderId) -> &mut T {
        &mut self.as_mut_slice()[idx]
    }
}

impl<V> core::ops::Index<LocalSenderId> for IdMap<LocalSenderId, V> {
    type Output = V;
    #[inline]
    fn index(&self, idx: LocalSenderId) -> &V {
        &self.values[idx.as_usize()]
    }
}

impl<V> core::ops::IndexMut<LocalSenderId> for IdMap<LocalSenderId, V> {
    #[inline]
    fn index_mut(&mut self, idx: LocalSenderId) -> &mut V {
        &mut self.values[idx.as_usize()]
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

        impl Id for $name {
            #[inline]
            fn from_index(idx: usize) -> Self {
                Self(idx)
            }

            #[inline]
            fn to_index(self) -> usize {
                self.0
            }
        }

        impl<V> core::ops::Index<$name> for IdMap<$name, V> {
            type Output = V;
            #[inline]
            fn index(&self, idx: $name) -> &V {
                &self.values[idx.as_usize()]
            }
        }

        impl<V> core::ops::IndexMut<$name> for IdMap<$name, V> {
            #[inline]
            fn index_mut(&mut self, idx: $name) -> &mut V {
                &mut self.values[idx.as_usize()]
            }
        }
    };
}

worker_id! {
    /// Index of a send socket within a single send worker.
    ///
    /// A send worker owns multiple sockets; `LocalSocketId` distinguishes them
    /// within that worker. This is the second step of the two-step lookup:
    /// `SenderIdx` → `LocalSocketId` → `send::Cache`.
    LocalSendSocketId
}

worker_id! {
    /// Index of a recv socket within a single recv worker.
    LocalRecvSocketId
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

// ── ID trait ────────────────────────────────────────────────────────────────

/// Trait for ID types that can be constructed from a positional index.
///
/// This allows [`IdMap`] iterators to yield `(K, &V)` pairs, propagating
/// strongly-typed keys through iteration rather than discarding them.
pub trait Id: Copy {
    fn from_index(idx: usize) -> Self;
    fn to_index(self) -> usize;

    /// Returns an iterator over all IDs in `[0, count)`.
    #[inline]
    fn range(count: usize) -> impl Iterator<Item = Self> {
        (0..count).map(Self::from_index)
    }
}

impl Id for LocalSenderId {
    #[inline]
    fn from_index(idx: usize) -> Self {
        Self::new(VarInt::new(idx as u64).unwrap())
    }

    #[inline]
    fn to_index(self) -> usize {
        self.as_usize()
    }
}

// ── Typed ID Mapping ────────────────────────────────────────────────────────

/// A typed lookup table mapping one ID type to another.
///
/// Replaces raw `Vec<usize>` with compile-time-checked indexing. The key type
/// (`K`) is used as the index; the value type (`V`) is returned from lookups.
///
/// # Example
///
/// ```ignore
/// let map: IdMap<LocalSenderId, usize> = IdMap::new(64, usize::MAX);
/// map[LocalSenderId::new(3)] = 7;
/// assert_eq!(map[LocalSenderId::new(3)], 7);
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

    pub fn iter(&self) -> impl Iterator<Item = (K, &V)>
    where
        K: Id,
    {
        self.values
            .iter()
            .enumerate()
            .map(|(i, v)| (K::from_index(i), v))
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (K, &mut V)>
    where
        K: Id,
    {
        self.values
            .iter_mut()
            .enumerate()
            .map(|(i, v)| (K::from_index(i), v))
    }
}

impl<K: Id, V> IntoIterator for IdMap<K, V> {
    type Item = (K, V);
    type IntoIter = IdMapIntoIter<K, V>;

    fn into_iter(self) -> Self::IntoIter {
        IdMapIntoIter {
            inner: self.values.into_iter().enumerate(),
            _key: core::marker::PhantomData,
        }
    }
}

pub struct IdMapIntoIter<K, V> {
    inner: core::iter::Enumerate<std::vec::IntoIter<V>>,
    _key: core::marker::PhantomData<fn() -> K>,
}

impl<K: Id, V> Iterator for IdMapIntoIter<K, V> {
    type Item = (K, V);

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next().map(|(i, v)| (K::from_index(i), v))
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl<'a, K: Id, V> IntoIterator for &'a IdMap<K, V> {
    type Item = (K, &'a V);
    type IntoIter = IdMapIter<'a, K, V>;

    fn into_iter(self) -> Self::IntoIter {
        IdMapIter {
            inner: self.values.iter().enumerate(),
            _key: core::marker::PhantomData,
        }
    }
}

pub struct IdMapIter<'a, K, V> {
    inner: core::iter::Enumerate<core::slice::Iter<'a, V>>,
    _key: core::marker::PhantomData<fn() -> K>,
}

impl<'a, K: Id, V> Iterator for IdMapIter<'a, K, V> {
    type Item = (K, &'a V);

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next().map(|(i, v)| (K::from_index(i), v))
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl<'a, K: Id, V> IntoIterator for &'a mut IdMap<K, V> {
    type Item = (K, &'a mut V);
    type IntoIter = IdMapIterMut<'a, K, V>;

    fn into_iter(self) -> Self::IntoIter {
        IdMapIterMut {
            inner: self.values.iter_mut().enumerate(),
            _key: core::marker::PhantomData,
        }
    }
}

pub struct IdMapIterMut<'a, K, V> {
    inner: core::iter::Enumerate<core::slice::IterMut<'a, V>>,
    _key: core::marker::PhantomData<fn() -> K>,
}

impl<'a, K: Id, V> Iterator for IdMapIterMut<'a, K, V> {
    type Item = (K, &'a mut V);

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next().map(|(i, v)| (K::from_index(i), v))
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
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

impl<K: Id, V> core::iter::FromIterator<(K, V)> for IdMap<K, V> {
    fn from_iter<I: IntoIterator<Item = (K, V)>>(iter: I) -> Self {
        let mut values = Vec::new();
        for (k, v) in iter {
            debug_assert_eq!(
                k.to_index(),
                values.len(),
                "IdMap::from_iter: keys must be in sequential order"
            );
            values.push(v);
        }
        Self {
            values,
            _key: core::marker::PhantomData,
        }
    }
}

impl<K, V> Default for IdMap<K, V> {
    fn default() -> Self {
        Self {
            values: Vec::new(),
            _key: core::marker::PhantomData,
        }
    }
}

impl<K: Id, V> Extend<(K, V)> for IdMap<K, V> {
    fn extend<I: IntoIterator<Item = (K, V)>>(&mut self, iter: I) {
        for (k, v) in iter {
            debug_assert_eq!(
                k.to_index(),
                self.values.len(),
                "IdMap::extend: keys must be in sequential order"
            );
            self.values.push(v);
        }
    }
}

impl<K, V: core::fmt::Debug> core::fmt::Debug for IdMap<K, V> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_list().entries(self.values.iter()).finish()
    }
}

// ── Tuple zip iteration ──────────────────────────────────────────────────────

/// Joins multiple `IdMap`s with the same key into a lockstep iterator.
///
/// Call `.join()` on a tuple of `IdMap`s to iterate them together,
/// yielding `(K, A, B, ...)` on each step.
pub trait IdJoin: Sized {
    type Iter: Iterator;
    fn join(self) -> Self::Iter;
}

macro_rules! impl_id_maps {
    (($($T:ident),+), ($($idx:tt),+), $iter_name:ident) => {
        impl<K: Id, $($T),+> IdJoin for ($(IdMap<K, $T>,)+) {
            type Iter = $iter_name<K, $($T),+>;

            fn join(self) -> Self::Iter {
                let lens = [$(self.$idx.values.len(),)+];
                let first = lens[0];
                debug_assert!(
                    lens.iter().all(|&l| l == first),
                    "IdMaps: all maps must have the same length"
                );
                $iter_name {
                    idx: 0,
                    len: first,
                    values: ($(self.$idx.values,)+),
                    _key: core::marker::PhantomData,
                }
            }
        }

        pub struct $iter_name<K, $($T),+> {
            idx: usize,
            len: usize,
            values: ($(Vec<$T>,)+),
            _key: core::marker::PhantomData<fn() -> K>,
        }

        impl<K: Id, $($T),+> Iterator for $iter_name<K, $($T),+> {
            type Item = (K, $($T),+);

            #[inline]
            fn next(&mut self) -> Option<Self::Item> {
                if self.idx >= self.len {
                    return None;
                }
                let i = self.idx;
                self.idx += 1;
                unsafe {
                    Some((
                        K::from_index(i),
                        $(self.values.$idx.as_ptr().add(i).read(),)+
                    ))
                }
            }

            #[inline]
            fn size_hint(&self) -> (usize, Option<usize>) {
                let remaining = self.len - self.idx;
                (remaining, Some(remaining))
            }
        }

        impl<K, $($T),+> Drop for $iter_name<K, $($T),+> {
            fn drop(&mut self) {
                while self.idx < self.len {
                    let i = self.idx;
                    self.idx += 1;
                    unsafe {
                        $(core::ptr::drop_in_place(self.values.$idx.as_mut_ptr().add(i));)+
                    }
                }
                $(unsafe { self.values.$idx.set_len(0); })+
            }
        }
    };
}

impl_id_maps!((A, B), (0, 1), IdMapTupleIter2);
impl_id_maps!((A, B, C), (0, 1, 2), IdMapTupleIter3);
impl_id_maps!((A, B, C, D), (0, 1, 2, 3), IdMapTupleIter4);
impl_id_maps!((A, B, C, D, E), (0, 1, 2, 3, 4), IdMapTupleIter5);

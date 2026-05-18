// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::fmt;
use s2n_quic_core::buffer::{
    reader::{
        self,
        storage::{Chunk, Infallible as _},
        Storage as _,
    },
    writer::{self, Storage as _},
};
use std::{collections::VecDeque, io, ops};

mod builder;
pub mod tagged;
#[cfg(test)]
mod tests;

pub use builder::Builder;
pub use bytes::{Bytes, BytesMut};
pub use tagged::Tagged;

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum ByteVecError {
    OutOfBounds(usize),
    OutOfBoundsRange(usize, usize),
}

impl fmt::Display for ByteVecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutOfBounds(len) => write!(f, "index out of bounds: {len}"),
            Self::OutOfBoundsRange(start, end) => write!(f, "range out of bounds: {start}..{end}"),
        }
    }
}

impl core::error::Error for ByteVecError {}

impl From<ByteVecError> for io::Error {
    #[inline]
    fn from(error: ByteVecError) -> Self {
        match error {
            ByteVecError::OutOfBounds(_) => Self::new(io::ErrorKind::UnexpectedEof, error),
            ByteVecError::OutOfBoundsRange(_, _) => Self::new(io::ErrorKind::UnexpectedEof, error),
        }
    }
}

/// A vector of [`Bytes`] chunks
///
/// Useful when you want to use a [`Vec<Bytes>`] in a "chunked" way, without
/// using [`bytes::BytesMut`] to build a new [`Bytes`] for each chunk.
///
/// ```
/// use s2n_quic_dc::byte_vec::ByteVec;
/// use bytes::Bytes;
///
/// let mut buf = ByteVec::default();
/// let data = Bytes::from(vec![0u8; 1000]);
/// buf.push_back(data);
///
/// for _ in 0..10 {
///     let chunk: ByteVec = buf.split_to(100).unwrap();
///     assert_eq!(chunk.len(), 100);
/// }
/// assert!(buf.pop_front().is_none());
/// ```
#[derive(Clone, Default)]
pub struct ByteVec {
    len: usize,
    head: Bytes,
    additional: VecDeque<Bytes>,
}

impl ByteVec {
    #[inline]
    pub const fn new() -> Self {
        Self {
            len: 0,
            head: Bytes::new(),
            additional: VecDeque::new(),
        }
    }

    #[inline]
    pub fn with_capacity(cap: usize) -> Self {
        Self {
            len: 0,
            head: Bytes::new(),
            additional: VecDeque::with_capacity(cap.saturating_sub(1)),
        }
    }

    #[inline]
    pub fn builder(chunk_capacity: usize) -> builder::Builder {
        builder::Builder::new(chunk_capacity)
    }

    /// Returns the total number of bytes in the buffer
    ///
    /// # Examples
    ///
    /// ```
    /// use s2n_quic_dc::byte_vec::ByteVec;
    /// use bytes::Bytes;
    ///
    /// let mut buf = ByteVec::new();
    ///
    /// buf.push_back(Bytes::from("hello"));
    /// assert_eq!(buf.len(), 5);
    ///
    /// buf.push_back(Bytes::from(" world"));
    /// assert_eq!(buf.len(), 11);
    /// ```
    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    /// Returns `true` if the buffer is empty
    ///
    /// # Examples
    ///
    /// ```
    /// use s2n_quic_dc::byte_vec::ByteVec;
    /// use bytes::Bytes;
    ///
    /// let mut buf = ByteVec::new();
    /// assert!(buf.is_empty());
    ///
    /// buf.push_back(Bytes::from("hello"));
    /// assert!(!buf.is_empty());
    /// ```
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Clears out all of the chunks in the buffer
    ///
    /// # Examples
    ///
    /// ```
    /// use s2n_quic_dc::byte_vec::ByteVec;
    ///
    /// let mut buf = ByteVec::from(b"hello");
    /// assert_eq!(buf.len(), 5);
    ///
    /// buf.clear();
    /// assert_eq!(buf.len(), 0);
    /// ```
    #[inline]
    pub fn clear(&mut self) {
        self.head = Bytes::new();
        self.additional.clear();
        self.len = 0;
        self.invariants();
    }

    /// Iterates over all of the [`Bytes`] chunks in the buffer
    ///
    /// # Examples
    ///
    /// ```
    /// use s2n_quic_dc::byte_vec::ByteVec;
    /// use bytes::Bytes;
    ///
    /// let mut buf = ByteVec::new();
    /// buf.push_back(Bytes::from("hello"));
    /// buf.push_back(Bytes::from(" world"));
    ///
    /// buf.chunks().map(|v| &v[..]).eq([&b"hello"[..], b" world"]);
    /// ```
    #[inline]
    pub fn chunks(&'_ self) -> ChunkIter<'_> {
        ChunkIter {
            head: if self.head.is_empty() {
                None
            } else {
                Some(&self.head)
            },
            tail: self.additional.iter(),
        }
    }

    /// Creates a reader from a reference
    ///
    /// This will avoid cloning the tail and just clone the individual chunks
    #[inline]
    pub fn reader(&'_ self) -> Reader<'_> {
        Reader {
            head: self.head.clone(),
            tail: self.additional.iter(),
            len: self.len,
        }
    }

    /// Pushes a chunk into the buffer
    ///
    /// # Examples
    ///
    /// ```
    /// use s2n_quic_dc::byte_vec::ByteVec;
    /// use bytes::Bytes;
    ///
    /// let mut buf = ByteVec::new();
    /// buf.push_back(Bytes::from("hello"));
    /// buf.push_back(Bytes::from(" world"));
    ///
    /// assert_eq!(buf, b"hello world");
    /// ```
    #[inline]
    pub fn push_back(&mut self, bytes: Bytes) {
        if bytes.is_empty() {
            return;
        }

        self.len += bytes.len();

        if self.head.is_empty() {
            self.head = bytes;
        } else {
            self.additional.push_back(bytes);
        }

        self.invariants();
    }

    /// Pushes a chunk into the front of the buffer
    ///
    /// # Examples
    ///
    /// ```
    /// use s2n_quic_dc::byte_vec::ByteVec;
    /// use bytes::Bytes;
    ///
    /// let mut buf = ByteVec::new();
    /// buf.push_front(Bytes::from(" world"));
    /// buf.push_front(Bytes::from("hello"));
    ///
    /// assert_eq!(buf, b"hello world");
    /// ```
    #[inline]
    pub fn push_front(&mut self, bytes: Bytes) {
        if bytes.is_empty() {
            return;
        }

        self.len += bytes.len();

        if self.head.is_empty() {
            self.head = bytes;
        } else {
            let prev_head = core::mem::replace(&mut self.head, bytes);
            self.additional.push_front(prev_head);
        }

        self.invariants();
    }

    /// Inserts a chunk at the given index
    ///
    /// This is currently only used by the [`Builder`] to insert length prefixes.
    pub(crate) fn insert(&mut self, chunk_index: usize, bytes: Bytes) {
        if bytes.is_empty() {
            return;
        }

        if chunk_index == 0 {
            self.push_front(bytes);
            return;
        }

        if chunk_index == self.chunks().len() {
            self.push_back(bytes);
            return;
        }

        // get the index for the `additional` list
        let chunk_index = chunk_index - 1;
        self.len += bytes.len();
        self.additional.insert(chunk_index, bytes);

        self.invariants();
    }

    /// Splits the bytes into two at the given index.
    ///
    /// Afterwards `self` contains elements `[at, len)`, and the returned
    /// [`ByteVec`] contains elements `[0, at)`.
    ///
    /// # Examples
    ///
    /// ```
    /// use s2n_quic_dc::byte_vec::ByteVec;
    ///
    /// let mut a = ByteVec::from(&b"hello world"[..]);
    /// let b = a.split_to(5).unwrap();
    ///
    /// assert_eq!(a, b" world");
    /// assert_eq!(b, b"hello");
    /// ```
    #[must_use = "consider ByteVec::advance if you don't need the other half"]
    #[inline]
    pub fn split_to(&mut self, at: usize) -> Result<Self, ByteVecError> {
        if self.len < at {
            return Err(ByteVecError::OutOfBounds(at));
        }
        let mut out = Self::new();

        let mut limited = out.with_write_limit(at);
        self.infallible_copy_into(&mut limited);

        self.invariants();

        Ok(out)
    }

    /// Splits the bytes into two at the given index.
    ///
    /// Afterwards `self` contains elements `[at, len)`, and the returned
    /// [`ByteVec`] contains elements `[0, at)`.
    ///
    /// ## Note
    ///
    /// This is unnecessarily expensive since it copies all of the chunks into
    /// a single [`Bytes`]. In most cases [`ByteVec::split_to`] should be preferred.
    ///
    /// # Examples
    ///
    /// ```
    /// use s2n_quic_dc::byte_vec::ByteVec;
    ///
    /// let mut a = ByteVec::from(&b"hello world"[..]);
    /// let b = a.split_to_copy(5).unwrap();
    ///
    /// assert_eq!(a, &b" world"[..]);
    /// assert_eq!(b, &b"hello"[..]);
    /// ```
    #[must_use = "consider ByteVec::advance if you don't need the other half"]
    #[inline]
    pub fn split_to_copy(&mut self, bytes: usize) -> Result<Bytes, ByteVecError> {
        if self.len < bytes {
            return Err(ByteVecError::OutOfBounds(bytes));
        }
        let mut out = BytesMut::with_capacity(bytes);

        let mut limited = out.with_write_limit(bytes);
        self.infallible_copy_into(&mut limited);

        self.invariants();

        Ok(out.freeze())
    }

    /// Returns the chunk at the front of the buffer
    ///
    /// # Examples
    ///
    /// ```
    /// use s2n_quic_dc::byte_vec::ByteVec;
    /// use bytes::Bytes;
    ///
    /// let mut buf = ByteVec::new();
    /// buf.push_back(Bytes::from("hello"));
    /// buf.push_back(Bytes::from(" world"));
    ///
    /// assert_eq!(buf.pop_front().unwrap(), &b"hello"[..]);
    /// assert_eq!(buf.pop_front().unwrap(), &b" world"[..]);
    /// assert_eq!(buf.pop_front(), None);
    /// ```
    #[inline]
    pub fn pop_front(&mut self) -> Option<Bytes> {
        if self.is_empty() {
            return None;
        }

        let out = self.read_chunk_bytes(self.head.len());

        debug_assert!(!out.is_empty());

        self.invariants();

        Some(out)
    }

    /// Returns the chunk at the back of the buffer
    ///
    /// # Examples
    ///
    /// ```
    /// use s2n_quic_dc::byte_vec::ByteVec;
    /// use bytes::Bytes;
    ///
    /// let mut buf = ByteVec::new();
    /// buf.push_back(Bytes::from("hello"));
    /// buf.push_back(Bytes::from(" world"));
    ///
    /// assert_eq!(buf.pop_back().unwrap(), &b" world"[..]);
    /// assert_eq!(buf.pop_back().unwrap(), &b"hello"[..]);
    /// assert_eq!(buf.pop_back(), None);
    /// ```
    #[inline]
    pub fn pop_back(&mut self) -> Option<Bytes> {
        if self.is_empty() {
            return None;
        }

        let chunk = if let Some(chunk) = self.additional.pop_back() {
            chunk
        } else {
            core::mem::take(&mut self.head)
        };

        self.len -= chunk.len();

        self.invariants();

        Some(chunk)
    }

    /// Moves all the elements of `other` into `self`, leaving `other` empty.
    ///
    /// # Panics
    ///
    /// Panics if the new number of elements in self overflows a `usize`.
    ///
    /// # Examples
    ///
    /// ```
    /// use s2n_quic_dc::byte_vec::ByteVec;
    /// use bytes::Bytes;
    ///
    /// let mut buf = ByteVec::new();
    /// buf.push_back(Bytes::from("hello"));
    ///
    /// let mut buf2 = ByteVec::new();
    /// buf2.push_back(Bytes::from(" world"));
    ///
    /// buf.append(&mut buf2);
    /// assert_eq!(buf, b"hello world");
    /// assert_eq!(buf2, b"");
    /// ```
    #[inline]
    pub fn append(&mut self, other: &mut Self) {
        // compute the new length with the 2 previous values
        // update the final length to avoid iterating over every chunk
        let new_len = self.len + other.len;
        // set the old one to 0
        other.len = 0;

        // first take the head from `other`
        self.push_back(core::mem::take(&mut other.head));
        // move all of the additional chunks to the back of `self`
        self.additional.append(&mut other.additional);
        // update the new length
        self.len = new_len;

        self.invariants();
        other.invariants();
    }

    /// Returns a chunk at the given index
    ///
    /// # Examples
    ///
    /// ```
    /// use s2n_quic_dc::byte_vec::ByteVec;
    /// use bytes::Bytes;
    ///
    /// let mut buf = ByteVec::new();
    /// buf.push_back(Bytes::from("hello"));
    /// buf.push_back(Bytes::from(" world"));
    ///
    /// assert_eq!(buf.get(0).unwrap(), &b"hello"[..]);
    /// assert_eq!(buf.get(1).unwrap(), &b" world"[..]);
    /// assert_eq!(buf.get(2), None);
    /// ```
    #[inline]
    pub fn get(&self, index: usize) -> Option<&Bytes> {
        if index == 0 {
            Some(&self.head)
        } else {
            self.additional.get(index - 1)
        }
    }

    /// Shortens the buffer, keeping the first `len` bytes and dropping the
    /// rest.
    ///
    /// If `len` is greater than the buffer's current length, this has no
    /// effect.
    ///
    /// # Examples
    ///
    /// ```
    /// use s2n_quic_dc::byte_vec::ByteVec;
    ///
    /// let mut buf = ByteVec::from(b"hello world");
    /// buf.truncate(5);
    /// assert_eq!(buf, b"hello");
    /// ```
    ///
    /// ```
    /// use s2n_quic_dc::byte_vec::ByteVec;
    ///
    /// let mut buf = ByteVec::from(b"hello world");
    /// buf.truncate(0);
    /// assert_eq!(buf, b"");
    /// ```
    ///
    /// ```
    /// use s2n_quic_dc::byte_vec::ByteVec;
    ///
    /// let mut buf = ByteVec::from(b"hello world");
    /// buf.truncate(11);
    /// assert_eq!(buf, b"hello world");
    /// ```
    #[inline]
    pub fn truncate(&mut self, len: usize) {
        if len == 0 {
            self.clear();
            return;
        }

        if self.len <= len {
            return;
        }

        let mut remaining = self.len - len;

        while remaining > 0 {
            let mut bytes = self.pop_back().unwrap();
            if bytes.len() > remaining {
                bytes.truncate(bytes.len() - remaining);
                self.push_back(bytes);
                break;
            } else {
                remaining -= bytes.len();
            }
        }

        self.invariants();
    }

    /// Flattens the [`ByteVec`] into a single [`Bytes`] buffer
    ///
    /// This should generally be avoided, since it forces a copy of the data
    /// into a [`Bytes`]. If you need to read the data, use [`ByteVec::chunks`]
    /// instead.
    ///
    /// # Examples
    ///
    /// ```
    /// use s2n_quic_dc::byte_vec::ByteVec;
    /// use bytes::Bytes;
    ///
    /// let mut buf = ByteVec::new();
    /// buf.push_back(Bytes::from("hello"));
    /// buf.push_back(Bytes::from(" world"));
    ///
    /// let bytes = buf.copy_to_bytes();
    ///
    /// assert_eq!(bytes, &b"hello world"[..]);
    /// ```
    #[inline]
    pub fn copy_to_bytes(&self) -> Bytes {
        if self.len == 0 {
            return Bytes::new();
        }

        // we don't need to copy anything if we only have one chunk
        if self.additional.is_empty() {
            return self.head.clone();
        }

        let mut out = BytesMut::with_capacity(self.len);

        for chunk in self.chunks() {
            out.extend_from_slice(chunk);
        }

        out.freeze()
    }

    /// Flattens the [`ByteVec`] into a single [`BytesMut`] buffer
    ///
    /// This should generally be avoided, since it forces a copy of the data
    /// into a [`BytesMut`]. If you need to read the data, use [`ByteVec::chunks`]
    /// instead.
    ///
    /// # Examples
    ///
    /// ```
    /// use s2n_quic_dc::byte_vec::ByteVec;
    /// use bytes::Bytes;
    ///
    /// let mut buf = ByteVec::new();
    /// buf.push_back(Bytes::from("hello"));
    /// buf.push_back(Bytes::from(" world"));
    ///
    /// let bytes = buf.copy_to_bytes_mut();
    ///
    /// assert_eq!(bytes, &b"hello world"[..]);
    /// ```
    pub fn copy_to_bytes_mut(self) -> BytesMut {
        if self.len == 0 {
            return BytesMut::new();
        }

        // we don't need to copy anything if we only have one chunk
        if self.additional.is_empty() {
            return BytesMut::from(self.head);
        }

        let mut out = BytesMut::with_capacity(self.len);

        for chunk in self.chunks() {
            out.extend_from_slice(chunk);
        }

        out
    }

    #[inline]
    pub fn tag<O: tagged::Owner>(self, owner: &O) -> tagged::Tagged<O> {
        tagged::Tagged::new(self, owner)
    }

    #[inline]
    fn advance_tail(&mut self) -> Option<Bytes> {
        debug_assert!(self.head.is_empty());
        self.additional.pop_front()
    }

    #[inline(always)]
    fn invariants(&self) {
        if cfg!(test) {
            if self.head.is_empty() {
                assert!(self.additional.is_empty());
            }

            let mut len = self.head.len();
            for chunk in &self.additional {
                len += chunk.len();
            }
            assert_eq!(len, self.len);
        }
    }
}

impl fmt::Debug for ByteVec {
    #[inline]
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.chunks()).finish()
    }
}

impl PartialEq for ByteVec {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        if self.len() != other.len() {
            return false;
        }

        // short cut looking at `additional` if we only have a `head`
        match (self.additional.is_empty(), other.additional.is_empty()) {
            (true, true) => self.head.eq(&other.head),
            (true, false) => other.eq(&self.head[..]),
            (false, true) => self.eq(&other.head[..]),
            (false, false) => {
                // TODO optimize this if needed.
                self.chunks().flatten().eq(other.chunks().flatten())
            }
        }
    }
}

impl Eq for ByteVec {}

impl PartialEq<[u8]> for ByteVec {
    #[inline]
    fn eq(&self, mut other: &[u8]) -> bool {
        if self.len() != other.len() {
            return false;
        }

        if self.additional.is_empty() {
            return self.head.eq(other);
        }

        for chunk in self.chunks() {
            let len = chunk.len();
            let (v, remaining) = other.split_at(len);
            if !chunk.eq(v) {
                return false;
            }
            other = remaining;
        }

        true
    }
}

impl PartialEq<&[u8]> for ByteVec {
    #[inline]
    fn eq(&self, other: &&[u8]) -> bool {
        self.eq(*other)
    }
}

impl PartialEq<str> for ByteVec {
    #[inline]
    fn eq(&self, other: &str) -> bool {
        self.eq(other.as_bytes())
    }
}

impl PartialEq<&str> for ByteVec {
    #[inline]
    fn eq(&self, other: &&str) -> bool {
        self.eq(other.as_bytes())
    }
}

impl<const LEN: usize> PartialEq<[u8; LEN]> for ByteVec {
    #[inline]
    fn eq(&self, other: &[u8; LEN]) -> bool {
        self.eq(&other[..])
    }
}

impl<const LEN: usize> PartialEq<&[u8; LEN]> for ByteVec {
    #[inline]
    fn eq(&self, other: &&[u8; LEN]) -> bool {
        self.eq(&other[..])
    }
}

impl PartialEq<[Bytes]> for ByteVec {
    #[inline]
    fn eq(&self, other: &[Bytes]) -> bool {
        if self.is_empty() != other.is_empty() {
            return false;
        }

        if other.len() == 1 {
            return self.eq(&other[0][..]);
        }

        // TODO optimize this if needed.
        self.chunks().flatten().eq(other.iter().flatten())
    }
}

impl PartialEq<&[Bytes]> for ByteVec {
    #[inline]
    fn eq(&self, other: &&[Bytes]) -> bool {
        self.eq(*other)
    }
}

impl<const LEN: usize> PartialEq<[Bytes; LEN]> for ByteVec {
    #[inline]
    fn eq(&self, other: &[Bytes; LEN]) -> bool {
        self.eq(&other[..])
    }
}

impl<const LEN: usize> PartialEq<&[Bytes; LEN]> for ByteVec {
    #[inline]
    fn eq(&self, other: &&[Bytes; LEN]) -> bool {
        self.eq(&other[..])
    }
}

impl PartialEq<Vec<u8>> for ByteVec {
    #[inline]
    fn eq(&self, other: &Vec<u8>) -> bool {
        self.eq(&other[..])
    }
}

impl PartialEq<Bytes> for ByteVec {
    #[inline]
    fn eq(&self, other: &Bytes) -> bool {
        self.eq(&other[..])
    }
}

impl From<Bytes> for ByteVec {
    #[inline]
    fn from(value: Bytes) -> Self {
        Self {
            len: value.len(),
            head: value,
            additional: VecDeque::new(),
        }
    }
}

impl From<BytesMut> for ByteVec {
    #[inline]
    fn from(value: BytesMut) -> Self {
        value.freeze().into()
    }
}

impl From<Vec<u8>> for ByteVec {
    #[inline]
    fn from(value: Vec<u8>) -> Self {
        Bytes::from(value).into()
    }
}

impl From<String> for ByteVec {
    #[inline]
    fn from(value: String) -> Self {
        value.into_bytes().into()
    }
}

impl From<Vec<Bytes>> for ByteVec {
    #[inline]
    fn from(value: Vec<Bytes>) -> Self {
        // reuse the allocation from the `Vec`
        let mut additional: VecDeque<_> = value.into();
        let head = additional.pop_front().unwrap_or_default();
        Self {
            len: head.len() + additional.iter().map(|b| b.len()).sum::<usize>(),
            head,
            additional,
        }
    }
}

impl From<&'static [u8]> for ByteVec {
    #[inline]
    fn from(value: &'static [u8]) -> Self {
        Bytes::from_static(value).into()
    }
}

impl<const LEN: usize> From<&'static [u8; LEN]> for ByteVec {
    #[inline]
    fn from(value: &'static [u8; LEN]) -> Self {
        Bytes::from_static(value).into()
    }
}

impl From<&'static str> for ByteVec {
    #[inline]
    fn from(value: &'static str) -> Self {
        Bytes::from_static(value.as_bytes()).into()
    }
}

impl ops::Index<usize> for ByteVec {
    type Output = Bytes;

    #[inline]
    fn index(&self, index: usize) -> &Self::Output {
        if index == 0 {
            assert!(!self.head.is_empty());
            return &self.head;
        }
        &self.additional[index - 1]
    }
}

impl Extend<ByteVec> for ByteVec {
    #[inline]
    fn extend<T: IntoIterator<Item = ByteVec>>(&mut self, iter: T) {
        for mut vec in iter {
            self.append(&mut vec);
        }
    }
}

impl Extend<Bytes> for ByteVec {
    #[inline]
    fn extend<T: IntoIterator<Item = Bytes>>(&mut self, iter: T) {
        let iter = iter.into_iter();
        let (min, max) = iter.size_hint();
        let mut len = min.max(max.unwrap_or(0));

        // we don't need capacity for the head chunk
        if self.is_empty() {
            len = len.saturating_sub(1);
        }

        self.additional.reserve(len);

        for bytes in iter {
            self.push_back(bytes);
        }
    }
}

impl Extend<Vec<u8>> for ByteVec {
    #[inline]
    fn extend<T: IntoIterator<Item = Vec<u8>>>(&mut self, iter: T) {
        let iter = iter.into_iter();
        let (min, max) = iter.size_hint();
        let mut len = min.max(max.unwrap_or(0));

        // we don't need capacity for the head chunk
        if self.is_empty() {
            len = len.saturating_sub(1);
        }

        self.additional.reserve(len);

        for bytes in iter {
            self.push_back(bytes.into());
        }
    }
}

impl FromIterator<Bytes> for ByteVec {
    #[inline]
    fn from_iter<T: IntoIterator<Item = Bytes>>(iter: T) -> Self {
        let mut this = Self::new();
        this.extend(iter);
        this
    }
}

impl IntoIterator for ByteVec {
    type Item = Bytes;

    type IntoIter = DrainIter;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        DrainIter {
            head: self.head,
            tail: self.additional,
            len: self.len,
        }
    }
}

pub struct ChunkIter<'a> {
    head: Option<&'a Bytes>,
    tail: std::collections::vec_deque::Iter<'a, Bytes>,
}

impl ChunkIter<'_> {
    #[inline]
    pub fn len(&self) -> usize {
        self.size_hint().0
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.head.is_none()
    }
}

impl<'a> Iterator for ChunkIter<'a> {
    type Item = &'a Bytes;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        core::mem::replace(&mut self.head, self.tail.next())
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let mut len = self.tail.len();
        if self.head.is_some() {
            len += 1;
        }
        (len, Some(len))
    }
}

#[derive(Clone)]
pub struct Reader<'a> {
    head: Bytes,
    tail: std::collections::vec_deque::Iter<'a, Bytes>,
    len: usize,
}

impl Reader<'_> {
    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    #[inline]
    fn advance_tail(&mut self) -> Option<Bytes> {
        debug_assert!(self.head.is_empty());
        self.tail.next().cloned()
    }

    #[inline(always)]
    fn invariants(&self) {
        if cfg!(test) {
            let mut len = self.head.len();
            for chunk in self.tail.clone() {
                len += chunk.len();
            }
            assert_eq!(len, self.len);
        }
    }
}

pub struct DrainIter {
    head: Bytes,
    tail: VecDeque<Bytes>,
    len: usize,
}

impl DrainIter {
    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    #[inline]
    fn advance_tail(&mut self) -> Option<Bytes> {
        debug_assert!(self.head.is_empty());
        self.tail.pop_front()
    }

    #[inline(always)]
    fn invariants(&self) {
        if cfg!(test) {
            if self.head.is_empty() {
                assert!(self.tail.is_empty());
            }

            let mut len = self.head.len();
            for chunk in &self.tail {
                len += chunk.len();
            }
            assert_eq!(len, self.len);
        }
    }
}

macro_rules! impl_reader_traits {
    ($ty:ty) => {
        impl $ty {
            /// Advance the internal cursor of the [`ByteVec`]
            ///
            /// # Examples
            ///
            /// ```
            /// use s2n_quic_dc::byte_vec::ByteVec;
            ///
            /// let mut buf = ByteVec::from(b"hello world");
            ///
            /// assert_eq!(buf, b"hello world");
            ///
            /// buf.advance(6).unwrap();
            ///
            /// assert_eq!(buf, b"world");
            /// ```
            #[inline]
            pub fn advance(&mut self, len: usize) -> Result<(), ByteVecError> {
                if len == 0 {
                    return Ok(());
                }

                if len > self.len {
                    return Err(ByteVecError::OutOfBounds(len));
                }

                let mut out = writer::storage::Discard;
                let mut out = out.with_write_limit(len);
                self.infallible_copy_into(&mut out);

                Ok(())
            }

            #[inline]
            fn read_chunk_bytes(&mut self, watermark: usize) -> Bytes {
                if watermark == 0 {
                    return Bytes::new();
                }

                if self.head.is_empty() {
                    return Bytes::new();
                }

                let Chunk::Bytes(head) = self
                    .head
                    .infallible_read_chunk(watermark.min(self.head.len()))
                else {
                    unreachable!()
                };

                self.len -= head.len();

                if self.head.is_empty() {
                    if let Some(head) = self.advance_tail() {
                        self.head = head;
                    }
                }

                self.invariants();

                head
            }

            #[inline]
            fn copy_head<Dest>(&mut self, dest: &mut Dest)
            where
                Dest: writer::storage::Storage + ?Sized,
            {
                let mut dest = dest.track_write();

                self.head.infallible_copy_into(&mut dest);

                self.len -= dest.written_len();

                if self.head.is_empty() {
                    if let Some(head) = self.advance_tail() {
                        self.head = head;
                    }
                }

                self.invariants();
            }
        }

        impl reader::Storage for $ty {
            type Error = core::convert::Infallible;

            #[inline]
            fn buffered_len(&self) -> usize {
                self.len
            }

            #[inline]
            fn read_chunk(
                &mut self,
                watermark: usize,
            ) -> Result<reader::storage::Chunk<'_>, Self::Error> {
                Ok(self.read_chunk_bytes(watermark).into())
            }

            #[inline]
            fn partial_copy_into<Dest>(
                &mut self,
                dest: &mut Dest,
            ) -> Result<reader::storage::Chunk<'_>, Self::Error>
            where
                Dest: writer::Storage + ?Sized,
            {
                loop {
                    let head_len = self.head.len();
                    if head_len == 0 {
                        return Ok(reader::storage::Chunk::empty());
                    }

                    let remaining_capacity = dest.remaining_capacity();
                    // if it's the last chunk we can just return it without copying
                    if head_len >= remaining_capacity {
                        let chunk = self.read_chunk_bytes(remaining_capacity);
                        return Ok(chunk.into());
                    }
                    self.copy_head(dest);
                }
            }

            #[inline]
            fn copy_into<Dest>(&mut self, dest: &mut Dest) -> Result<(), Self::Error>
            where
                Dest: writer::Storage + ?Sized,
            {
                loop {
                    self.copy_head(dest);

                    if self.is_empty() || !dest.has_remaining_capacity() {
                        return Ok(());
                    }
                }
            }
        }

        impl std::io::Read for $ty {
            #[inline]
            fn read(&mut self, mut buf: &mut [u8]) -> std::io::Result<usize> {
                let mut dest = buf.track_write();
                self.infallible_copy_into(&mut dest);
                Ok(dest.written_len())
            }
        }

        impl bytes::Buf for $ty {
            #[inline]
            fn remaining(&self) -> usize {
                self.len()
            }

            #[inline]
            fn chunk(&self) -> &[u8] {
                &self.head
            }

            #[inline]
            fn copy_to_bytes(&mut self, len: usize) -> Bytes {
                assert!(len <= self.len);
                if self.head.len() >= len {
                    let chunk = self.read_chunk_bytes(len);
                    return chunk;
                }
                let mut buf = BytesMut::with_capacity(len);
                {
                    let mut buf = buf.with_write_limit(len);
                    self.infallible_copy_into(&mut buf);
                }
                buf.freeze()
            }

            #[inline]
            fn advance(&mut self, len: usize) {
                Self::advance(self, len).unwrap();
            }
        }
    };
}

macro_rules! impl_iter_traits {
    ($ty:ty) => {
        impl Iterator for $ty {
            type Item = Bytes;

            #[inline]
            fn next(&mut self) -> Option<Self::Item> {
                let chunk = self.read_chunk_bytes(usize::MAX);
                if chunk.is_empty() {
                    None
                } else {
                    Some(chunk)
                }
            }

            #[inline]
            fn size_hint(&self) -> (usize, Option<usize>) {
                let mut len = self.tail.len();
                if !self.head.is_empty() {
                    len += 1;
                }
                (len, Some(len))
            }
        }
    };
}

impl_reader_traits!(ByteVec);
impl_reader_traits!(Reader<'_>);
impl_iter_traits!(Reader<'_>);
impl_reader_traits!(DrainIter);
impl_iter_traits!(DrainIter);

impl writer::Storage for ByteVec {
    const SPECIALIZES_BYTES: bool = true;

    #[inline]
    fn put_slice(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        self.push_back(Bytes::copy_from_slice(bytes));
    }

    #[inline]
    fn put_bytes(&mut self, bytes: Bytes) {
        self.push_back(bytes);
    }

    #[inline]
    fn remaining_capacity(&self) -> usize {
        usize::MAX
    }
}

impl std::io::Write for ByteVec {
    #[inline]
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.put_slice(buf);
        Ok(buf.len())
    }

    #[inline]
    fn write_vectored(&mut self, bufs: &[std::io::IoSlice<'_>]) -> std::io::Result<usize> {
        let mut bufs = reader::storage::IoSlice::new(bufs);
        let len = bufs.buffered_len();
        let mut bytes = BytesMut::with_capacity(len);
        bufs.infallible_copy_into(&mut bytes);
        self.put_bytes_mut(bytes);
        Ok(len)
    }

    #[inline]
    fn write_all(&mut self, buf: &[u8]) -> std::io::Result<()> {
        self.put_slice(buf);
        Ok(())
    }

    #[inline]
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(any(test, feature = "testing"))]
impl bolero_generator::TypeGenerator for ByteVec {
    #[inline]
    fn generate<D>(driver: &mut D) -> std::option::Option<Self>
    where
        D: bolero_generator::Driver,
    {
        use bolero_generator::ValueGenerator as _;

        let count = (1..4).generate(driver)?;
        let mut out = ByteVec::with_capacity(count);
        for _ in 0..count {
            let bytes: Vec<u8> = bolero_generator::TypeGenerator::generate(driver)?;
            out.push_back(bytes.into());
        }
        Some(out)
    }
}

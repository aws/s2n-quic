use super::*;
use s2n_quic_core::buffer::writer::Storage;

const DEFAULT_CAPACITY: usize = 1 << 17;

/// A builder for efficiently constructing a [`ByteVec`] by buffering writes.
///
/// The builder maintains a head buffer for direct writes and a collection of
/// completed chunks. This allows for efficient buffering of writes while
/// maintaining the chunked nature of [`ByteVec`].
///
/// # Examples
///
/// ```
/// use s2n_quic_dc::byte_vec::ByteVec;
/// use bytes::Bytes;
/// use s2n_quic_core::buffer::writer::Storage;
///
/// let mut builder = ByteVec::builder(1024);
/// builder.put_slice(b"hello");
/// builder.put_slice(b" world");
///
/// let byte_vec = builder.finish();
/// assert_eq!(byte_vec, b"hello world");
/// ```
#[derive(Debug)]
pub struct Builder {
    chunks: ByteVec,
    head: BytesMut,
    capacity: usize,
}

impl Default for Builder {
    fn default() -> Self {
        Self::new(DEFAULT_CAPACITY)
    }
}

impl Builder {
    pub const DEFAULT_CAPACITY: usize = DEFAULT_CAPACITY;

    /// Creates a new [`Builder`] with the specified capacity for the head buffer.
    ///
    /// The capacity determines the size of the internal buffer used for direct writes.
    /// When this buffer is full, it will be flushed to the chunks collection.
    ///
    /// # Examples
    ///
    /// ```
    /// use s2n_quic_dc::byte_vec::ByteVec;
    ///
    /// let builder = ByteVec::builder(1024);
    /// ```
    pub fn new(capacity: usize) -> Self {
        Builder {
            chunks: ByteVec::new(),
            head: BytesMut::new(),
            capacity,
        }
    }

    /// Returns the total number of bytes in the builder.
    ///
    /// This includes both the bytes in the completed chunks and the head buffer.
    ///
    /// # Examples
    ///
    /// ```
    /// use s2n_quic_dc::byte_vec::ByteVec;
    /// use s2n_quic_core::buffer::writer::Storage;
    ///
    /// let mut builder = ByteVec::builder(1024);
    /// builder.put_slice(b"hello");
    /// assert_eq!(builder.len(), 5);
    ///
    /// builder.put_slice(b" world");
    /// assert_eq!(builder.len(), 11);
    /// ```
    pub fn len(&self) -> usize {
        self.chunks.len() + self.head.len()
    }

    /// Returns `true` if the builder contains no bytes.
    ///
    /// # Examples
    ///
    /// ```
    /// use s2n_quic_dc::byte_vec::ByteVec;
    /// use s2n_quic_core::buffer::writer::Storage;
    ///
    /// let mut builder = ByteVec::builder(1024);
    /// assert!(builder.is_empty());
    ///
    /// builder.put_slice(b"hello");
    /// assert!(!builder.is_empty());
    /// ```
    pub fn is_empty(&self) -> bool {
        self.head.is_empty() && self.chunks.is_empty()
    }

    /// Appends the contents of another [`ByteVec`] to this builder.
    ///
    /// This operation flushes the current head buffer before appending
    /// the new bytes.
    ///
    /// # Examples
    ///
    /// ```
    /// use s2n_quic_dc::byte_vec::ByteVec;
    /// use s2n_quic_core::buffer::writer::Storage;
    ///
    /// let mut builder = ByteVec::builder(1024);
    /// builder.put_slice(b"hello");
    ///
    /// let mut other = ByteVec::from(b" world");
    /// builder.append(&mut other);
    ///
    /// let result = builder.finish();
    /// assert_eq!(result, b"hello world");
    /// ```
    pub fn append(&mut self, bytes: &mut ByteVec) {
        if bytes.is_empty() {
            return;
        }
        self.flush();
        self.chunks.append(bytes);
    }

    /// Appends the contents of another [`ByteVec`] to this builder.
    ///
    /// This operation flushes the current head buffer before appending
    /// the new bytes.
    ///
    /// # Examples
    ///
    /// ```
    /// use s2n_quic_dc::byte_vec::ByteVec;
    /// use s2n_quic_core::buffer::writer::Storage;
    ///
    /// let mut builder = ByteVec::builder(1024);
    /// builder.put_slice(b"hello");
    ///
    /// let other = ByteVec::from(b" world");
    /// builder.extend(&other);
    ///
    /// let result = builder.finish();
    /// assert_eq!(result, b"hello world");
    /// ```
    pub fn extend(&mut self, bytes: &ByteVec) {
        if bytes.is_empty() {
            return;
        }
        self.flush();
        self.chunks.extend(bytes.chunks().cloned());
    }

    /// Splits the builder, taking all bytes and leaving it empty.
    ///
    /// This operation flushes the current head buffer before splitting.
    ///
    /// # Examples
    ///
    /// ```
    /// use s2n_quic_dc::byte_vec::ByteVec;
    /// use s2n_quic_core::buffer::writer::Storage;
    ///
    /// let mut builder = ByteVec::builder(1024);
    /// builder.put_slice(b"hello world");
    ///
    /// let bytes = builder.split();
    /// assert_eq!(bytes, b"hello world");
    /// assert!(builder.is_empty());
    /// ```
    pub fn split(&mut self) -> ByteVec {
        self.flush();
        core::mem::take(&mut self.chunks)
    }

    /// Splits the bytes into two at the given index.
    ///
    /// After this operation, `self` contains elements `[at, len)`, and the
    /// returned [`ByteVec`] contains elements `[0, at)`.
    ///
    /// # Errors
    ///
    /// Returns [`ByteVecError::OutOfBounds`] if `at` is greater than the
    /// builder's length.
    ///
    /// # Examples
    ///
    /// ```
    /// use s2n_quic_dc::byte_vec::ByteVec;
    /// use s2n_quic_core::buffer::writer::Storage;
    ///
    /// let mut builder = ByteVec::builder(1024);
    /// builder.put_slice(b"hello world");
    ///
    /// let hello = builder.split_to(5).unwrap();
    /// assert_eq!(hello, b"hello");
    ///
    /// let result = builder.finish();
    /// assert_eq!(result, b" world");
    /// ```
    pub fn split_to(&mut self, at: usize) -> Result<ByteVec, ByteVecError> {
        let len = self.len();

        if len < at {
            return Err(ByteVecError::OutOfBounds(at));
        }

        if len == at {
            return Ok(self.split());
        }

        // check if we need to move some of the head into the chunks
        if let Some(remaining) = at.checked_sub(self.chunks.len()).filter(|v| *v > 0) {
            self.chunks
                .push_back(self.head.split_to(remaining).freeze());
        }

        self.chunks.split_to(at)
    }

    /// Finishes building and returns the constructed [`ByteVec`].
    ///
    /// This operation consumes the builder and returns the final [`ByteVec`]
    /// containing all written bytes.
    ///
    /// # Examples
    ///
    /// ```
    /// use s2n_quic_dc::byte_vec::ByteVec;
    /// use s2n_quic_core::buffer::writer::Storage;
    ///
    /// let mut builder = ByteVec::builder(1024);
    /// builder.put_slice(b"hello");
    /// builder.put_slice(b" world");
    ///
    /// let result = builder.finish();
    /// assert_eq!(result, b"hello world");
    /// ```
    pub fn finish(self) -> ByteVec {
        let mut chunks = self.chunks;
        if !self.head.is_empty() {
            chunks.push_back(self.head.freeze());
        }
        chunks
    }

    /// Calls the provided function and prefixes the written data with a `u64` length
    pub fn write_with_len_prefix<F: FnOnce(&mut Self)>(&mut self, f: F) {
        // flush any data we have buffered
        self.flush();

        // record the chunk index where to insert the length
        let chunk_index = self.chunks.chunks().len();

        // record the starting length
        let before_len = self.len();

        // have the caller write into the buffer
        f(self);

        // flush any data we have buffered from the caller write
        self.flush();

        // compute the amount of data written by the caller
        let written_len = (self.len() - before_len) as u64;

        // write the length into the `head` buffer ensuring it stays in one chunk
        let written_len_bytes = written_len.to_be_bytes();
        if written_len_bytes.len() > self.head.spare_capacity_mut().len() {
            self.flush_and_reserve(written_len_bytes.len());
        }
        self.head.put_slice(&written_len_bytes);

        // insert the length chunk where we recorded initially
        let len_chunk = self.head.split().freeze();
        // make sure the length chunk is not torn
        debug_assert_eq!(len_chunk.len(), 8);
        self.chunks.insert(chunk_index, len_chunk);
    }

    /// Reserves buffer space for reading from a socket
    pub fn for_socket_read<F: FnOnce(&mut bytes::buf::UninitSlice) -> usize>(
        &mut self,
        preferred_read_size: usize,
        f: F,
    ) {
        if preferred_read_size > self.head.spare_capacity_mut().len() {
            self.flush_and_reserve(preferred_read_size);
        }

        let len = self
            .head
            .put_uninit_slice(preferred_read_size, |slice| {
                let len = f(slice);
                Err(len)
            })
            .unwrap_err();

        unsafe {
            use bytes::BufMut;
            self.head.advance_mut(len);
        }
    }

    // flushes and reserves at least the specified `min_len`
    fn flush_and_reserve(&mut self, min_len: usize) {
        let capacity = self.capacity.max(min_len);
        let head = core::mem::replace(&mut self.head, BytesMut::with_capacity(capacity));
        if !head.is_empty() {
            self.chunks.push_back(head.freeze());
        }
    }

    // flushes the current buffer, if non-empty
    fn flush(&mut self) {
        if !self.head.is_empty() {
            self.chunks.push_back(self.head.split().freeze());
        }
    }
}

impl From<ByteVec> for Builder {
    fn from(chunks: ByteVec) -> Self {
        Builder {
            chunks,
            head: BytesMut::new(),
            capacity: DEFAULT_CAPACITY,
        }
    }
}

impl From<Builder> for ByteVec {
    fn from(writer: Builder) -> Self {
        writer.finish()
    }
}

impl writer::Storage for Builder {
    // we prefer direct writes into the head chunk rather than appending byte chunks
    const SPECIALIZES_BYTES: bool = false;
    const SPECIALIZES_BYTES_MUT: bool = false;

    fn put_slice(&mut self, bytes: &[u8]) {
        let remaining_capacity = self.head.spare_capacity_mut().len();
        let len = bytes.len().min(remaining_capacity);
        let (head, tail) = bytes.split_at(len);

        // append the head if it has capacity
        if !head.is_empty() {
            self.head.put_slice(head);
        }

        // if tail is non-empty then we need to allocate a new chunk
        if !tail.is_empty() {
            self.flush_and_reserve(tail.len());
            self.head.put_slice(tail);
        }
    }

    fn remaining_capacity(&self) -> usize {
        usize::MAX
    }

    fn put_uninit_slice<F, Error>(&mut self, payload_len: usize, f: F) -> Result<bool, Error>
    where
        F: FnOnce(&mut bytes::buf::UninitSlice) -> Result<(), Error>,
    {
        if payload_len > self.head.spare_capacity_mut().len() {
            self.flush_and_reserve(payload_len);
        }

        self.head.put_uninit_slice(payload_len, f)
    }

    fn has_remaining_capacity(&self) -> bool {
        true
    }

    fn put_bytes(&mut self, bytes: Bytes) {
        if bytes.is_empty() {
            return;
        }
        self.flush();
        self.chunks.push_back(bytes);
    }

    fn put_bytes_mut(&mut self, bytes: BytesMut) {
        if bytes.is_empty() {
            return;
        }
        self.flush();
        self.chunks.push_back(bytes.freeze());
    }
}

impl reader::Storage for Builder {
    type Error = core::convert::Infallible;

    fn buffered_len(&self) -> usize {
        self.head.buffered_len() + self.chunks.buffered_len()
    }

    fn buffer_is_empty(&self) -> bool {
        self.head.is_empty() && self.chunks.is_empty()
    }

    fn read_chunk(&mut self, watermark: usize) -> Result<Chunk<'_>, Self::Error> {
        if self.chunks.is_empty() {
            self.head.read_chunk(watermark)
        } else {
            self.chunks.read_chunk(watermark)
        }
    }

    fn partial_copy_into<Dest>(&mut self, dest: &mut Dest) -> Result<Chunk<'_>, Self::Error>
    where
        Dest: writer::Storage + ?Sized,
    {
        // First drain chunks into dest
        if !self.chunks.buffer_is_empty() {
            let chunk = self.chunks.partial_copy_into(dest)?;

            if !chunk.is_empty() {
                let mut should_return = false;

                // If dest matches the chunk, return it
                should_return |= dest.remaining_capacity() == chunk.len();

                // if the `head` is empty then return early as well
                should_return |= self.head.buffer_is_empty();

                if should_return {
                    return Ok(chunk);
                }

                // Otherwise, write the trailing chunk to dest and continue to head
                dest.put_chunk(chunk);
            }
        }

        // Then drain head into dest
        self.head.partial_copy_into(dest)
    }

    fn copy_into<Dest>(&mut self, dest: &mut Dest) -> Result<(), Self::Error>
    where
        Dest: writer::Storage + ?Sized,
    {
        self.chunks.copy_into(dest)?;
        self.head.copy_into(dest)
    }
}

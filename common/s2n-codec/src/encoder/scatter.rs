// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{Encoder, EncoderBuffer};

pub struct Buffer<'a> {
    inner: EncoderBuffer<'a>,
    #[cfg(feature = "bytes")]
    extra: Option<bytes::Bytes>,
}

impl<'a> Buffer<'a> {
    /// Ensures an extra bytes are written into the main EncoderBuffer.
    #[inline]
    pub fn flatten(&mut self) -> &mut EncoderBuffer<'a> {
        self.flush();
        &mut self.inner
    }
}

/// Implement a version with `bytes` enabled
#[cfg(feature = "bytes")]
impl<'a> Buffer<'a> {
    /// Initializes a buffer without any extra bytes at the end
    #[inline]
    pub fn new(inner: EncoderBuffer<'a>) -> Self {
        Self { inner, extra: None }
    }

    /// Initializes the buffer with extra bytes
    ///
    /// NOTE the EncoderBuffer position should not include the extra bytes. This ensures the bytes
    /// can be "flushed" into the EncoderBuffer if another write happens or `flatten` is called.
    #[inline]
    pub fn new_with_extra(inner: EncoderBuffer<'a>, extra: Option<bytes::Bytes>) -> Self {
        Self { inner, extra }
    }

    /// Converts the buffer into its inner parts
    ///
    /// NOTE: the EncoderBuffer position will not include the extra bytes. The caller will need to
    /// account for this data.
    #[inline]
    pub fn into_inner(self) -> (EncoderBuffer<'a>, Option<bytes::Bytes>) {
        (self.inner, self.extra)
    }

    /// Returns the inner EncoderBuffer, along with the current extra bytes
    #[inline]
    pub fn inner_mut(&mut self) -> (&mut EncoderBuffer<'a>, &Option<bytes::Bytes>) {
        (&mut self.inner, &self.extra)
    }

    /// Resets the encoder to its initial state while also dropping any extra bytes at the end
    #[inline]
    pub fn clear(&mut self) {
        self.inner.set_position(0);
        self.extra = None;
    }

    #[inline]
    fn flush(&mut self) {
        // move the extra bytes into the main buffer
        if let Some(extra) = self.extra.take() {
            self.inner.write_slice(&extra);
        }
    }
}

/// Include a functional implementation when `bytes` is not available
#[cfg(not(feature = "bytes"))]
impl<'a> Buffer<'a> {
    #[inline]
    pub fn new(inner: EncoderBuffer<'a>) -> Self {
        Self { inner }
    }

    #[inline]
    pub fn new_with_extra(inner: EncoderBuffer<'a>, extra: Option<&'static [u8]>) -> Self {
        debug_assert!(extra.is_none());
        Self { inner }
    }

    #[inline]
    pub fn into_inner(self) -> (EncoderBuffer<'a>, Option<&'static [u8]>) {
        (self.inner, None)
    }

    #[inline]
    pub fn inner_mut(&mut self) -> (&mut EncoderBuffer<'a>, &Option<&'static [u8]>) {
        (&mut self.inner, &None)
    }

    #[inline]
    pub fn clear(&mut self) {
        self.inner.set_position(0);
    }

    #[inline(always)]
    fn flush(&mut self) {}
}

impl Encoder for Buffer<'_> {
    /// We have special handling for writes that include `Bytes` so signal that
    #[cfg(feature = "bytes")]
    const SPECIALIZES_BYTES: bool = true;

    #[inline]
    fn write_sized<F: FnOnce(&mut [u8])>(&mut self, len: usize, write: F) {
        // we need to flush the extra bytes if we have them
        self.flush();
        self.inner.write_sized(len, write)
    }

    #[inline]
    fn write_slice(&mut self, slice: &[u8]) {
        // we need to flush the extra bytes if we have them
        self.flush();
        self.inner.write_slice(slice);
    }

    #[inline]
    #[cfg(feature = "bytes")]
    fn write_bytes(&mut self, bytes: bytes::Bytes) {
        // we need to flush the extra bytes if we have them and replace them with the new one
        self.flush();
        // ensure the underlying buffer is big enough for the write
        self.inner.assert_capacity(bytes.len());
        // store the extra bytes and defer the copy
        self.extra = Some(bytes);
    }

    #[inline]
    fn write_zerocopy<
        T: zerocopy::AsBytes + zerocopy::FromBytes + zerocopy::Unaligned,
        F: FnOnce(&mut T),
    >(
        &mut self,
        write: F,
    ) {
        // we need to flush the extra bytes if we have them
        self.flush();
        self.inner.write_zerocopy(write);
    }

    #[inline]
    fn write_repeated(&mut self, count: usize, value: u8) {
        // we need to flush the extra bytes if we have them
        self.flush();
        self.inner.write_repeated(count, value)
    }

    #[inline]
    fn capacity(&self) -> usize {
        self.inner.capacity()
    }

    #[inline]
    #[cfg(feature = "bytes")]
    fn len(&self) -> usize {
        let mut len = self.inner.len();

        // make sure our len includes the extra bytes if we have them
        if let Some(extra) = self.extra.as_ref() {
            len += extra.len();
        }

        len
    }

    #[inline]
    #[cfg(not(feature = "bytes"))]
    fn len(&self) -> usize {
        self.inner.len()
    }
}

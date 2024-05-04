// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{storage::Infallible as _, Reader, Storage, VarInt};
use core::convert::Infallible;

pub struct Fallible<'a, R, E>
where
    R: ?Sized + Storage<Error = Infallible>,
    E: 'static + Clone,
{
    inner: &'a mut R,
    error: Option<E>,
}

impl<'a, R, E> Fallible<'a, R, E>
where
    R: ?Sized + Storage<Error = Infallible>,
    E: 'static + Clone,
{
    #[inline]
    pub fn new(inner: &'a mut R) -> Self {
        Self { inner, error: None }
    }

    #[inline]
    pub fn with_error(mut self, error: E) -> Self {
        self.error = Some(error);
        self
    }

    #[inline]
    pub fn set_error(&mut self, error: Option<E>) {
        self.error = error;
    }

    #[inline]
    fn check_error(&self) -> Result<(), E> {
        if let Some(error) = self.error.as_ref() {
            Err(error.clone())
        } else {
            Ok(())
        }
    }
}

impl<'a, R, E> Storage for Fallible<'a, R, E>
where
    R: ?Sized + Storage<Error = Infallible>,
    E: 'static + Clone,
{
    type Error = E;

    #[inline]
    fn buffered_len(&self) -> usize {
        self.inner.buffered_len()
    }

    #[inline]
    fn buffer_is_empty(&self) -> bool {
        self.inner.buffer_is_empty()
    }

    #[inline]
    fn read_chunk(&mut self, watermark: usize) -> Result<super::storage::Chunk<'_>, Self::Error> {
        self.check_error()?;
        let chunk = self.inner.infallible_read_chunk(watermark);
        Ok(chunk)
    }

    #[inline]
    fn partial_copy_into<Dest>(
        &mut self,
        dest: &mut Dest,
    ) -> Result<super::storage::Chunk<'_>, Self::Error>
    where
        Dest: crate::buffer::writer::Storage + ?Sized,
    {
        self.check_error()?;
        let chunk = self.inner.infallible_partial_copy_into(dest);
        Ok(chunk)
    }

    #[inline]
    fn copy_into<Dest>(&mut self, dest: &mut Dest) -> Result<(), Self::Error>
    where
        Dest: crate::buffer::writer::Storage + ?Sized,
    {
        self.check_error()?;
        self.inner.infallible_copy_into(dest);
        Ok(())
    }
}

impl<'a, R, E> Reader for Fallible<'a, R, E>
where
    R: ?Sized + Reader<Error = Infallible>,
    E: 'static + Clone,
{
    #[inline]
    fn current_offset(&self) -> VarInt {
        self.inner.current_offset()
    }

    #[inline]
    fn final_offset(&self) -> Option<VarInt> {
        self.inner.final_offset()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::reader::storage::Chunk;
    use s2n_codec::DecoderError;

    #[test]
    fn fallible_test() {
        let mut reader = Chunk::Slice(b"hello");
        let mut reader = Fallible::new(&mut reader).with_error(DecoderError::UnexpectedEof(1));

        assert!(reader.read_chunk(1).is_err());
    }
}

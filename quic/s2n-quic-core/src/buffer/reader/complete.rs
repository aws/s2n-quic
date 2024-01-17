// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    buffer::{
        reader::{storage::Chunk, Reader, Storage},
        writer, Error,
    },
    varint::VarInt,
};

/// Wraps a single [`Storage`] instance as a [`Reader`].
///
/// This can be used for scenarios where the entire stream is buffered and known up-front.
#[derive(Debug)]
pub struct Complete<'a, S> {
    storage: &'a mut S,
    current_offset: VarInt,
    final_offset: VarInt,
}

impl<'a, S> Complete<'a, S>
where
    S: Storage,
{
    #[inline]
    pub fn new(storage: &'a mut S) -> Result<Self, Error> {
        let final_offset = VarInt::try_from(storage.buffered_len())
            .ok()
            .ok_or(Error::OutOfRange)?;
        Ok(Self {
            storage,
            current_offset: VarInt::ZERO,
            final_offset,
        })
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.storage.buffer_is_empty()
    }
}

impl<'a, S> Storage for Complete<'a, S>
where
    S: Storage,
{
    type Error = S::Error;

    #[inline]
    fn buffered_len(&self) -> usize {
        self.storage.buffered_len()
    }

    #[inline]
    fn read_chunk(&mut self, watermark: usize) -> Result<Chunk, Self::Error> {
        let chunk = self.storage.read_chunk(watermark)?;
        self.current_offset += chunk.len();
        Ok(chunk)
    }

    #[inline]
    fn partial_copy_into<Dest>(&mut self, dest: &mut Dest) -> Result<Chunk, Self::Error>
    where
        Dest: writer::Storage + ?Sized,
    {
        let mut dest = dest.tracked();
        let chunk = self.storage.partial_copy_into(&mut dest)?;
        self.current_offset += chunk.len();
        self.current_offset += dest.written_len();
        Ok(chunk)
    }

    #[inline]
    fn copy_into<Dest>(&mut self, dest: &mut Dest) -> Result<(), Self::Error>
    where
        Dest: writer::Storage + ?Sized,
    {
        let mut dest = dest.tracked();
        self.storage.copy_into(&mut dest)?;
        self.current_offset += dest.written_len();
        Ok(())
    }
}

impl<'a, C> Reader for Complete<'a, C>
where
    C: Storage,
{
    #[inline]
    fn current_offset(&self) -> VarInt {
        self.current_offset
    }

    #[inline]
    fn final_offset(&self) -> Option<VarInt> {
        Some(self.final_offset)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complete_test() {
        let mut storage: &[u8] = &[1, 2, 3, 4];
        let mut reader = Complete::new(&mut storage).unwrap();

        assert_eq!(reader.current_offset(), VarInt::ZERO);
        assert_eq!(reader.final_offset(), Some(VarInt::from_u8(4)));

        let mut dest: &mut [u8] = &mut [0; 4];
        let chunk = reader.partial_copy_into(&mut dest).unwrap();
        assert_eq!(&*chunk, &[1, 2, 3, 4]);

        assert_eq!(reader.current_offset(), VarInt::from_u8(4));
        assert!(reader.buffer_is_empty());
    }
}

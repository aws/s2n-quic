// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    buffer::{
        reader::{storage::Chunk, Reader, Storage},
        writer, Error,
    },
    ensure,
    varint::VarInt,
};

/// Implements an incremental [`Reader`] that joins to temporary [`Storage`] as the stream data
///
/// This is useful for scenarios where the the stream isn't completely buffered in memory and
/// data come in gradually.
#[derive(Debug, Default)]
pub struct Incremental {
    current_offset: VarInt,
    final_offset: Option<VarInt>,
}

impl Incremental {
    #[inline]
    pub fn with_storage<'a, C: Storage>(
        &'a mut self,
        storage: &'a mut C,
        is_fin: bool,
    ) -> Result<WithStorage<'a, C>, Error> {
        let mut storage = WithStorage {
            incremental: self,
            storage,
        };

        if is_fin {
            storage.set_fin()?;
        } else {
            ensure!(
                storage.incremental.final_offset.is_none(),
                Err(Error::InvalidFin)
            );
        }

        Ok(storage)
    }

    #[inline]
    pub fn current_offset(&self) -> VarInt {
        self.current_offset
    }

    #[inline]
    pub fn final_offset(&self) -> Option<VarInt> {
        self.final_offset
    }
}

pub struct WithStorage<'a, C: Storage> {
    incremental: &'a mut Incremental,
    storage: &'a mut C,
}

impl<'a, C: Storage> WithStorage<'a, C> {
    #[inline]
    pub fn set_fin(&mut self) -> Result<&mut Self, Error> {
        let final_offset = self
            .incremental
            .current_offset
            .checked_add_usize(self.buffered_len())
            .ok_or(Error::OutOfRange)?;

        // make sure the final length doesn't change
        if let Some(current) = self.incremental.final_offset {
            ensure!(final_offset == current, Err(Error::InvalidFin));
        }

        self.incremental.final_offset = Some(final_offset);

        Ok(self)
    }
}

impl<'a, C: Storage> Storage for WithStorage<'a, C> {
    type Error = C::Error;

    #[inline]
    fn buffered_len(&self) -> usize {
        self.storage.buffered_len()
    }

    #[inline]
    fn read_chunk(&mut self, watermark: usize) -> Result<Chunk, Self::Error> {
        let chunk = self.storage.read_chunk(watermark)?;
        self.incremental.current_offset += chunk.len();
        Ok(chunk)
    }

    #[inline]
    fn partial_copy_into<Dest>(&mut self, dest: &mut Dest) -> Result<Chunk, Self::Error>
    where
        Dest: writer::Storage + ?Sized,
    {
        let mut dest = dest.tracked();
        let chunk = self.storage.partial_copy_into(&mut dest)?;
        self.incremental.current_offset += chunk.len();
        self.incremental.current_offset += dest.written_len();
        Ok(chunk)
    }

    #[inline]
    fn copy_into<Dest>(&mut self, dest: &mut Dest) -> Result<(), Self::Error>
    where
        Dest: writer::Storage + ?Sized,
    {
        let mut dest = dest.tracked();
        self.storage.copy_into(&mut dest)?;
        self.incremental.current_offset += dest.written_len();
        Ok(())
    }
}

impl<'a, C: Storage> Reader for WithStorage<'a, C> {
    #[inline]
    fn current_offset(&self) -> VarInt {
        self.incremental.current_offset()
    }

    #[inline]
    fn final_offset(&self) -> Option<VarInt> {
        self.incremental.final_offset()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn incremental_test() {
        let mut incremental = Incremental::default();

        assert_eq!(incremental.current_offset(), VarInt::ZERO);
        assert_eq!(incremental.final_offset(), None);

        {
            let mut chunk: &[u8] = &[1, 2, 3, 4];
            let mut reader = incremental.with_storage(&mut chunk, false).unwrap();
            let mut reader = reader.with_checks();

            assert_eq!(reader.buffered_len(), 4);

            let mut dest: &mut [u8] = &mut [0; 4];
            let trailing_chunk = reader.partial_copy_into(&mut dest).unwrap();
            assert_eq!(&*trailing_chunk, &[1, 2, 3, 4]);

            assert_eq!(reader.buffered_len(), 0);
        }

        assert_eq!(incremental.current_offset(), VarInt::from_u8(4));
    }
}

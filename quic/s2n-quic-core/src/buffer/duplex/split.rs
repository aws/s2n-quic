// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    buffer::{
        duplex,
        reader::{self, Reader, Storage as _},
        writer::{self, Writer},
        Error,
    },
    varint::VarInt,
};
use core::convert::Infallible;

/// A split duplex that tries to write as much as possible to `storage`, while falling back to
/// `duplex`.
pub struct Split<'a, S, D>
where
    S: writer::Storage + ?Sized,
    D: duplex::Skip<Error = Infallible> + ?Sized,
{
    storage: &'a mut S,
    duplex: &'a mut D,
}

impl<'a, S, D> Split<'a, S, D>
where
    S: writer::Storage + ?Sized,
    D: duplex::Skip<Error = Infallible> + ?Sized,
{
    #[inline]
    pub fn new(storage: &'a mut S, duplex: &'a mut D) -> Self {
        Self { storage, duplex }
    }
}

/// Delegates to the inner Duplex
impl<'a, S, D> reader::Storage for Split<'a, S, D>
where
    S: writer::Storage + ?Sized,
    D: duplex::Skip<Error = Infallible> + ?Sized,
{
    type Error = D::Error;

    #[inline]
    fn buffered_len(&self) -> usize {
        self.duplex.buffered_len()
    }

    #[inline]
    fn buffer_is_empty(&self) -> bool {
        self.duplex.buffer_is_empty()
    }

    #[inline]
    fn read_chunk(&mut self, watermark: usize) -> Result<reader::storage::Chunk<'_>, Self::Error> {
        self.duplex.read_chunk(watermark)
    }

    #[inline]
    fn partial_copy_into<Dest>(
        &mut self,
        dest: &mut Dest,
    ) -> Result<reader::storage::Chunk<'_>, Self::Error>
    where
        Dest: writer::Storage + ?Sized,
    {
        self.duplex.partial_copy_into(dest)
    }

    #[inline]
    fn copy_into<Dest>(&mut self, dest: &mut Dest) -> Result<(), Self::Error>
    where
        Dest: writer::Storage + ?Sized,
    {
        self.duplex.copy_into(dest)
    }
}

/// Delegates to the inner Duplex
impl<'a, C, D> Reader for Split<'a, C, D>
where
    C: writer::Storage + ?Sized,
    D: duplex::Skip<Error = Infallible> + ?Sized,
{
    #[inline]
    fn current_offset(&self) -> VarInt {
        self.duplex.current_offset()
    }

    #[inline]
    fn final_offset(&self) -> Option<VarInt> {
        self.duplex.final_offset()
    }

    #[inline]
    fn has_buffered_fin(&self) -> bool {
        self.duplex.has_buffered_fin()
    }

    #[inline]
    fn is_consumed(&self) -> bool {
        self.duplex.is_consumed()
    }
}

impl<'a, C, D> Writer for Split<'a, C, D>
where
    C: writer::Storage + ?Sized,
    D: duplex::Skip<Error = Infallible> + ?Sized,
{
    #[inline]
    fn read_from<R>(&mut self, reader: &mut R) -> Result<(), Error<R::Error>>
    where
        R: Reader + ?Sized,
    {
        let final_offset = reader.final_offset();

        {
            // if the storage specializes writing zero-copy Bytes/BytesMut, then just write to the
            // receive buffer, since that's what it stores
            let mut should_delegate = C::SPECIALIZES_BYTES || C::SPECIALIZES_BYTES_MUT;

            // if the storage is empty then write into the duplex
            should_delegate |= !self.storage.has_remaining_capacity();

            // if this packet is non-contiguous, then delegate to the wrapped writer
            should_delegate |= reader.current_offset() != self.duplex.current_offset();

            // if the storage has less than half of the payload, then delegate
            should_delegate |= self.storage.remaining_capacity() < (reader.buffered_len() / 2);

            if should_delegate {
                self.duplex.read_from(reader)?;

                // don't copy into `storage` here - let the caller do that later since it can be
                // more efficient to pull from `duplex` all in one go.

                return Ok(());
            }
        }

        debug_assert!(
            self.storage.has_remaining_capacity(),
            "this code should only be executed if the storage has capacity"
        );

        {
            // track the number of consumed bytes
            let mut reader = reader.track_read();

            reader.copy_into(self.storage)?;

            let write_len = reader.consumed_len();
            let write_len = VarInt::try_from(write_len).map_err(|_| Error::OutOfRange)?;

            // notify the duplex that we bypassed it and should skip
            self.duplex
                .skip(write_len, final_offset)
                .map_err(Error::mapped)?;
        }

        // if we still have some remaining bytes consume the rest in the duplex
        if !reader.buffer_is_empty() {
            self.duplex.read_from(reader)?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        buffer::{
            reader::Reader,
            writer::{Storage as _, Writer},
            Reassembler,
        },
        stream::testing::Data,
    };

    #[test]
    fn undersized_storage_test() {
        let mut duplex = Reassembler::default();
        let mut reader = Data::new(10);
        let mut checker = reader;

        let mut storage: Vec<u8> = vec![];
        {
            // limit the storage capacity so we force writing into the duplex
            let mut storage = storage.with_write_limit(1);

            let mut split = Split::new(&mut storage, &mut duplex);

            split.read_from(&mut reader).unwrap();
        }

        // the storage was too small so we delegated to duplex
        assert!(storage.is_empty());
        assert_eq!(duplex.buffered_len(), 10);

        // move the reassembled bytes into the checker
        checker.read_from(&mut duplex).unwrap();
        assert_eq!(duplex.current_offset().as_u64(), 10);
        assert!(duplex.is_consumed());
    }

    #[test]
    fn out_of_order_test() {
        let mut duplex = Reassembler::default();

        // first write 5 bytes at offset 5
        {
            let mut reader = Data::new(10);

            // advance the reader by 5 bytes
            let _ = reader.send_one(5);

            let mut storage: Vec<u8> = vec![];

            let mut split = Split::new(&mut storage, &mut duplex);

            split.read_from(&mut reader).unwrap();

            // make sure we consumed the reader
            assert_eq!(reader.current_offset().as_u64(), 10);

            assert_eq!(split.current_offset().as_u64(), 0);
            assert_eq!(split.buffered_len(), 0);

            // make sure we didn't write to the storage, even if we had capacity, since the
            // current_offset doesn't match
            assert!(storage.is_empty());
        }

        // then write 10 bytes at offset 0
        {
            let mut reader = Data::new(10);

            let mut storage: Vec<u8> = vec![];

            let mut split = Split::new(&mut storage, &mut duplex);

            split.read_from(&mut reader).unwrap();

            // make sure we consumed the reader
            assert_eq!(reader.current_offset().as_u64(), 10);

            assert_eq!(split.current_offset().as_u64(), 10);
            assert_eq!(split.buffered_len(), 0);

            // make sure we copied the entire reader
            assert_eq!(storage.len(), 10);
            assert!(duplex.is_consumed());
        }
    }

    #[test]
    fn skip_test() {
        let mut duplex = Reassembler::default();
        let mut reader = Data::new(10);
        let mut checker = reader;

        let mut storage: Vec<u8> = vec![];

        let mut split = Split::new(&mut storage, &mut duplex);

        split.read_from(&mut reader).unwrap();

        assert_eq!(storage.len(), 10);
        assert_eq!(duplex.current_offset().as_u64(), 10);

        checker.receive(&[&storage[..]]);
    }

    #[test]
    fn empty_storage_test() {
        let mut duplex = Reassembler::default();
        let mut reader = Data::new(10);
        let mut checker = reader;

        let mut storage = writer::storage::Empty;

        let mut split = Split::new(&mut storage, &mut duplex);

        split.read_from(&mut reader).unwrap();

        assert_eq!(split.current_offset().as_u64(), 0);
        assert_eq!(split.buffered_len(), 10);

        checker.read_from(&mut split).unwrap();

        assert_eq!(split.current_offset().as_u64(), 10);
        assert!(split.buffer_is_empty());
        assert_eq!(split.buffered_len(), 0);
        assert!(split.is_consumed());
    }

    #[test]
    fn partial_test() {
        let mut duplex = Reassembler::default();
        let mut reader = Data::new(10);
        let mut checker = reader;

        let mut storage: Vec<u8> = vec![];
        {
            let mut storage = storage.with_write_limit(9);

            let mut split = Split::new(&mut storage, &mut duplex);

            split.read_from(&mut reader).unwrap();
        }

        // the storage was at least half the reader
        assert_eq!(storage.len(), 9);
        assert_eq!(duplex.buffered_len(), 1);

        // move the reassembled bytes into the checker
        checker.receive(&[&storage]);
        checker.read_from(&mut duplex).unwrap();
        assert_eq!(duplex.current_offset().as_u64(), 10);
        assert!(duplex.is_consumed());
    }
}

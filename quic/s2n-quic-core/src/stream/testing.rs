// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! A model that ensures stream data is correctly sent and received between peers

use crate::buffer::{reader, writer};
use bytes::Bytes;

#[cfg(any(test, feature = "generator"))]
use bolero_generator::prelude::*;

static DATA: Bytes = {
    static INNER: [u8; DATA_LEN] = {
        let mut data = [0; DATA_LEN];
        let mut idx = 0;
        while idx < DATA_LEN {
            data[idx] = idx as u8;
            idx += 1;
        }
        data
    };

    Bytes::from_static(&INNER)
};

// when running with miri, set the values lower to make execution less expensive
const DATA_LEN: usize = (DEFAULT_STREAM_LEN as usize) * if cfg!(miri) { 1 } else { 128 };
const DEFAULT_STREAM_LEN: u64 = if cfg!(miri) { DATA_MOD as _ } else { 1024 };
const DATA_MOD: usize = 256; // Only the first 256 offsets of DATA are unique

#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Eq, Ord)]
pub struct Data {
    offset: u64,
    final_offset: Option<u64>,
    buffered_len: u64,
}

#[cfg(any(feature = "generator", test))]
impl TypeGenerator for Data {
    fn generate<D: bolero_generator::Driver>(driver: &mut D) -> Option<Self> {
        let offset = produce::<u64>().generate(driver)?;
        let final_offset = produce::<Option<u64>>()
            .with()
            .value(offset..)
            .generate(driver)?;

        let max_buffered_len = if let Some(end) = final_offset {
            let remaining = end - offset;
            remaining.min(DEFAULT_STREAM_LEN)
        } else {
            DEFAULT_STREAM_LEN
        };

        let buffered_len = (..max_buffered_len).generate(driver)?;

        Some(Data {
            offset,
            final_offset,
            buffered_len,
        })
    }
}

impl Default for Data {
    fn default() -> Self {
        Self::new(DEFAULT_STREAM_LEN)
    }
}

impl Data {
    /// The maximum length of a chunk of data for the stream
    pub const MAX_CHUNK_LEN: usize = DATA_LEN;

    pub const fn new(len: u64) -> Self {
        Self {
            buffered_len: len,
            final_offset: Some(len),
            offset: 0,
        }
    }

    /// Notifies the data that a set of chunks were received
    pub fn receive<Chunk: AsRef<[u8]>>(&mut self, chunks: &[Chunk]) {
        Self::receive_check(&mut self.offset, self.final_offset, chunks)
    }

    /// Notifies the data that a set of chunks were received
    pub fn receive_at<Chunk: AsRef<[u8]>>(&self, mut start: u64, chunks: &[Chunk]) {
        Self::receive_check(&mut start, self.final_offset, chunks)
    }

    fn receive_check<Chunk: AsRef<[u8]>>(
        start: &mut u64,
        final_offset: Option<u64>,
        chunks: &[Chunk],
    ) {
        for mut chunk in chunks.iter().map(AsRef::as_ref) {
            while !chunk.is_empty() {
                let offset = ((*start) % DATA_MOD as u64) as usize;
                let len = chunk.len().min(DATA.len() - offset);
                assert_eq!(
                    &chunk[..len],
                    &DATA[offset..offset + len],
                    "receive stream data at offset {} has been corrupted",
                    *start,
                );
                *start += len as u64;
                chunk = &chunk[len..];
            }
        }

        if let Some(final_offset) = final_offset {
            assert!(
                final_offset >= (*start),
                "receive stream data has exceeded beyond the expected amount by {}",
                (*start) - final_offset
            );
        }
    }

    /// Returns `true` if the stream is finished reading/writing
    pub fn is_finished(&self) -> bool {
        if let Some(fin) = self.final_offset {
            fin <= self.offset
        } else {
            false
        }
    }

    /// Asks the data what chunks should be sent next
    ///
    /// Returns the number of chunks that were filled
    pub fn send(&mut self, mut amount: usize, chunks: &mut [Bytes]) -> Option<usize> {
        if self.is_finished() {
            return None;
        }

        let mut count = 0;
        for chunk in chunks.iter_mut() {
            if let Some(data) = self.send_one(amount).filter(|data| !data.is_empty()) {
                amount -= data.len();
                count += 1;
                *chunk = data;
            } else {
                break;
            }
        }

        Some(count)
    }

    /// Sends a single chunk of data, up to the provided `amount`
    pub fn send_one(&mut self, amount: usize) -> Option<Bytes> {
        if self.is_finished() {
            return None;
        }

        let amount = (self.buffered_len as usize).min(amount);
        let chunk = Self::send_one_at(self.offset, amount);

        self.seek_forward(chunk.len() as u64);

        Some(chunk)
    }

    pub fn send_one_at(offset: u64, amount: usize) -> Bytes {
        let offset = (offset % DATA_MOD as u64) as usize;
        let to_send = amount.min(DATA.len() - offset);

        if to_send == 0 {
            return Bytes::new();
        }

        DATA.slice(offset..offset + to_send)
    }

    /// Returns the current offset being received or sent
    pub fn offset(&self) -> u64 {
        self.offset
    }

    /// Moves the current offset forward by the provided `len`
    pub fn seek_forward(&mut self, len: u64) {
        let len = self.buffered_len.min(len);
        self.buffered_len -= len;
        if let Some(new_offset) = self.offset.checked_add(len) {
            self.offset = new_offset;
        } else {
            self.final_offset = Some(u64::MAX);
            self.buffered_len = 0;
        }
    }
}

impl reader::Storage for Data {
    type Error = core::convert::Infallible;

    #[inline]
    fn buffered_len(&self) -> usize {
        self.buffered_len as usize
    }

    #[inline]
    fn read_chunk(&mut self, watermark: usize) -> Result<reader::storage::Chunk, Self::Error> {
        if let Some(chunk) = self.send_one(watermark) {
            return Ok(chunk.into());
        }

        Ok(Default::default())
    }

    #[inline]
    fn partial_copy_into<Dest: writer::Storage + ?Sized>(
        &mut self,
        dest: &mut Dest,
    ) -> Result<reader::storage::Chunk, Self::Error> {
        while let Some(chunk) = self.send_one(dest.remaining_capacity()) {
            // if the chunk matches the destination then return it instead of copying
            if chunk.len() == dest.remaining_capacity() || self.is_finished() {
                return Ok(chunk.into());
            }

            // otherwise copy the chunk into the destination
            dest.put_bytes(chunk);
        }

        Ok(Default::default())
    }
}

impl reader::Reader for Data {
    #[inline]
    fn current_offset(&self) -> crate::varint::VarInt {
        self.offset()
            .try_into()
            .unwrap_or(crate::varint::VarInt::MAX)
    }

    #[inline]
    fn final_offset(&self) -> Option<crate::varint::VarInt> {
        self.final_offset.and_then(|v| v.try_into().ok())
    }
}

impl writer::Storage for Data {
    #[inline]
    fn put_slice(&mut self, slice: &[u8]) {
        self.receive(&[slice]);
    }

    #[inline]
    fn remaining_capacity(&self) -> usize {
        // return max so readers don't know where the stream is expected to end and we can make an
        // assertion when they write
        usize::MAX
    }
}

impl writer::Writer for Data {
    #[inline]
    fn read_from<R>(&mut self, reader: &mut R) -> Result<(), crate::buffer::Error<R::Error>>
    where
        R: reader::Reader + ?Sized,
    {
        // no need to specialize on anything here
        reader.copy_into(self)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bolero::check;

    #[test]
    fn receive_test() {
        let mut receiver = Data::new(10);
        assert!(!receiver.is_finished());

        receiver.receive(&[&[0, 1, 2]]);
        assert!(!receiver.is_finished());

        receiver.receive(&[&[3, 4, 5]]);
        assert!(!receiver.is_finished());

        receiver.receive(&[&[6, 7, 8, 9]]);
        assert!(receiver.is_finished());
    }

    #[test]
    #[should_panic]
    fn receive_corruption_test() {
        let mut receiver = Data::new(10);
        receiver.receive(&[&[1, 2, 3]]);
    }

    #[test]
    #[should_panic]
    fn receive_overflow_test() {
        let mut receiver = Data::new(2);
        receiver.receive(&[&[0, 1, 2]]);
    }

    #[test]
    fn send_test() {
        let mut sender = Data::new(10);
        assert!(!sender.is_finished());

        let mut output = vec![Bytes::new(); 10];

        assert_eq!(sender.send(3, &mut output), Some(1));
        assert_eq!(output[0].as_ref(), &[0, 1, 2][..]);
        assert!(!sender.is_finished());

        assert_eq!(sender.send(4, &mut output), Some(1));
        assert_eq!(output[0].as_ref(), &[3, 4, 5, 6][..]);
        assert!(!sender.is_finished());

        assert_eq!(sender.send(3, &mut output), Some(1));
        assert_eq!(output[0].as_ref(), &[7, 8, 9][..]);
        assert!(sender.is_finished());

        assert_eq!(sender.send(1, &mut output), None);
    }

    #[test]
    #[cfg_attr(miri, ignore)] // This test breaks in CI but can't be reproduced locally - https://github.com/aws/s2n-quic/issues/867
    fn send_receive() {
        let g = (
            (1..(DEFAULT_STREAM_LEN * 16)),
            (1..(DEFAULT_STREAM_LEN * 2)),
        );

        check!()
            .with_generator(g)
            .cloned()
            .for_each(|(stream_len, send_amount)| {
                let mut sender = Data::new(stream_len);
                let mut receiver = Data::new(stream_len);

                let mut buf = vec![Bytes::new(); 5];

                while let Some(count) = sender.send(send_amount as usize, &mut buf[..]) {
                    assert_ne!(count, 0, "sender should return None if no chunks were sent");
                    receiver.receive(&buf[..count]);
                }

                assert!(sender.is_finished());
                assert!(receiver.is_finished());
            })
    }

    #[test]
    fn buffer_trait_test() {
        use writer::Writer as _;

        let mut reader = Data::new(10);
        let mut writer = reader;

        writer.read_from(&mut reader).unwrap();

        assert!(reader.is_finished());
        assert!(writer.is_finished());
    }
}

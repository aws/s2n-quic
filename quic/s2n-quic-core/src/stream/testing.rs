// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! A model that ensures stream data is correctly sent and received between peers

#[cfg(feature = "generator")]
use bolero_generator::*;

#[cfg(test)]
use bolero::generator::*;

use bytes::Bytes;

static DATA: Bytes = {
    const INNER: [u8; DATA_LEN] = {
        let mut data = [0; DATA_LEN];
        let mut idx = 0;
        while idx < data.len() {
            data[idx] = idx as u8;
            idx += 1;
        }
        data
    };

    Bytes::from_static(&INNER)
};

const DATA_LEN: usize = (DEFAULT_STREAM_LEN as usize) * 128;
const DEFAULT_STREAM_LEN: u64 = 1024;
const DATA_MOD: usize = 256; // Only the first 256 offsets of DATA are unique

#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Eq, Ord)]
#[cfg_attr(any(feature = "generator", test), derive(TypeGenerator))]
pub struct Data {
    #[cfg_attr(any(feature = "generator", test), generator(0..=DEFAULT_STREAM_LEN))]
    len: u64,
    #[cfg_attr(any(feature = "generator", test), generator(constant(0)))]
    offset: u64,
}

impl Default for Data {
    fn default() -> Self {
        Self::new(DEFAULT_STREAM_LEN as u64)
    }
}

impl Data {
    pub const fn new(len: u64) -> Self {
        Self { len, offset: 0 }
    }

    /// Notifies the data that a set of chunks were received
    pub fn receive<Chunk: AsRef<[u8]>>(&mut self, chunks: &[Chunk]) {
        Self::receive_check(&mut self.offset, self.len, chunks)
    }

    /// Notifies the data that a set of chunks were received
    pub fn receive_at<Chunk: AsRef<[u8]>>(&self, mut start: u64, chunks: &[Chunk]) {
        Self::receive_check(&mut start, self.len, chunks)
    }

    fn receive_check<Chunk: AsRef<[u8]>>(start: &mut u64, len: u64, chunks: &[Chunk]) {
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

        assert!(
            len >= (*start),
            "receive stream data has exceeded beyond the expected amount by {}",
            (*start) - len
        );
    }

    pub fn is_finished(&self) -> bool {
        self.len <= self.offset
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
            let offset = (self.offset % DATA_MOD as u64) as usize;
            let to_send = ((self.len - self.offset) as usize)
                .min(amount)
                .min(DATA.len() - offset);

            if to_send == 0 {
                break;
            }

            *chunk = DATA.slice(offset..offset + to_send);

            self.offset += to_send as u64;
            amount -= to_send;
            count += 1;
        }

        Some(count)
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
            })
    }
}

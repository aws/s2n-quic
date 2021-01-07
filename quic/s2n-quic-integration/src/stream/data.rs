//! A model that ensures stream data is correctly sent and received between peers

use bolero_generator::{constant, TypeGenerator};
use bytes::Bytes;
use lazy_static::lazy_static;

lazy_static! {
    static ref DATA: Bytes = unsafe {
        static mut INNER: [u8; DATA_LEN] = [0; DATA_LEN];
        for (idx, byte) in INNER.iter_mut().enumerate() {
            *byte = idx as u8;
        }
        Bytes::from_static(&INNER)
    };
}

const DATA_LEN: usize = DEFAULT_STREAM_LEN * 8;
const DEFAULT_STREAM_LEN: usize = 1024;
const DATA_MOD: usize = 256; // Only the first 256 offsets of DATA are unique

#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Eq, Ord, TypeGenerator)]
pub struct Data {
    #[generator(0..=DEFAULT_STREAM_LEN)]
    len: usize,
    #[generator(constant(0))]
    offset: usize,
}

impl Default for Data {
    fn default() -> Self {
        Self::new(DEFAULT_STREAM_LEN)
    }
}

impl Data {
    pub const fn new(len: usize) -> Self {
        Self { len, offset: 0 }
    }

    /// Notifies the data that a set of chunks were received
    pub fn receive<Chunk: AsRef<[u8]>>(&mut self, chunks: &[Chunk]) {
        for mut chunk in chunks.iter().map(AsRef::as_ref) {
            while !chunk.is_empty() {
                let offset = self.offset % DATA_MOD;
                let len = chunk.len().min(DATA.len() - offset);
                assert_eq!(
                    &chunk[..len],
                    &DATA[offset..offset + len],
                    "receive stream data at offset {} has been corrupted",
                    self.offset,
                );
                self.offset += len;
                chunk = &chunk[len..];
            }
        }

        assert!(
            self.len >= self.offset,
            "receive stream data has exceeded beyond the expected amount by {}",
            self.offset - self.len
        );
    }

    pub fn is_finished(&self) -> bool {
        self.len <= self.offset
    }

    /// Asks the data what chunks should be sent next
    pub fn send(&mut self, mut amount: usize, chunks: &mut [Bytes]) -> Option<usize> {
        if self.is_finished() {
            return None;
        }

        let mut count = 0;
        for chunk in chunks.iter_mut() {
            let offset = self.offset % DATA_MOD;
            let to_send = (self.len - self.offset)
                .min(amount)
                .min(DATA.len() - offset);

            if to_send == 0 {
                break;
            }

            *chunk = DATA.slice(offset..offset + to_send);

            self.offset += to_send;
            amount -= to_send;
            count += 1;
        }

        Some(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arr_macro::arr;
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

        let mut output = arr![Bytes::new(); 10];

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

                let mut buf = arr![
                    Bytes::new(); 5
                ];

                while let Some(count) = sender.send(send_amount, &mut buf[..]) {
                    assert_ne!(count, 0, "sender should return None if no chunks were sent");
                    receiver.receive(&buf[..count]);
                }
            })
    }
}

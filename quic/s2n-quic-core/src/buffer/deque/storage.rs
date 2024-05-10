// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::buffer::{
    reader::{self, storage::Chunk},
    writer,
};
use bytes::buf::UninitSlice;

impl writer::Storage for super::Deque {
    #[inline]
    fn put_slice(&mut self, bytes: &[u8]) {
        self.unfilled().put_slice(bytes);

        unsafe {
            // SAFETY: we write `len` bytes with `put_slice`
            self.fill(bytes.len()).unwrap();
        }
    }

    #[inline]
    fn put_uninit_slice<F, Error>(&mut self, payload_len: usize, f: F) -> Result<bool, Error>
    where
        F: FnOnce(&mut UninitSlice) -> Result<(), Error>,
    {
        let (mut head, _) = self.unfilled().into();

        let did_write = head.put_uninit_slice(payload_len, f)?;

        if did_write {
            unsafe {
                // SAFETY: the caller wrote into the bytes
                self.fill(payload_len).unwrap();
            }
        }

        Ok(did_write)
    }

    #[inline]
    fn remaining_capacity(&self) -> usize {
        (*self).remaining_capacity()
    }
}

impl reader::Storage for super::Deque {
    type Error = core::convert::Infallible;

    #[inline]
    fn buffered_len(&self) -> usize {
        self.len()
    }

    #[inline]
    fn read_chunk(&mut self, watermark: usize) -> Result<Chunk<'_>, Self::Error> {
        ensure!(watermark > 0 && !self.is_empty(), Ok(Chunk::default()));

        // compute how many bytes we need to consume
        let len = {
            let (head, _) = self.filled().into();
            debug_assert!(!head.is_empty());

            head.len().min(watermark)
        };

        let (head, _) = self.consume_filled(len).into();

        Ok(head[..].into())
    }

    #[inline]
    fn partial_copy_into<Dest>(&mut self, dest: &mut Dest) -> Result<Chunk<'_>, Self::Error>
    where
        Dest: writer::Storage + ?Sized,
    {
        ensure!(
            dest.has_remaining_capacity() && !self.is_empty(),
            Ok(Chunk::default())
        );

        let len = self.len().min(dest.remaining_capacity());

        let should_return_tail = {
            let (head, _tail) = self.filled().into();

            // if the head isn't enough to fill the watermark then we also need to return the tail
            head.len() < len
        };

        let (head, tail) = self.consume_filled(len).into();

        if should_return_tail {
            dest.put_slice(head);
            Ok(tail[..].into())
        } else {
            Ok(head[..].into())
        }
    }

    #[inline]
    fn copy_into<Dest>(&mut self, dest: &mut Dest) -> Result<(), Self::Error>
    where
        Dest: writer::Storage + ?Sized,
    {
        ensure!(dest.has_remaining_capacity() && !self.is_empty(), Ok(()));

        let len = self.len().min(dest.remaining_capacity());

        self.consume_filled(len).copy_into(dest)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        super::Deque,
        reader::{storage::Infallible as _, Reader},
        writer::Storage as _,
    };
    use crate::stream::testing::Data;
    use bolero::{check, TypeGenerator};

    #[test]
    fn storage_test() {
        let cap = 16;
        let mut buffer = Deque::new(cap);
        assert_eq!(buffer.remaining_capacity(), cap);

        buffer.put_slice(b"hello");
        buffer.put_slice(b" ");
        buffer.put_slice(b"world");

        let chunk = buffer.infallible_read_chunk(7);
        assert_eq!(&chunk[..], b"hello w");

        let chunk = buffer.infallible_read_chunk(3);
        assert_eq!(&chunk[..], b"orl");

        buffer.put_slice(&[42u8; 15]);

        let mut out: Vec<u8> = vec![];
        let chunk = buffer.infallible_partial_copy_into(&mut out);
        assert_eq!(&out[..], &[b'd', 42, 42, 42, 42, 42]);
        assert_eq!(&chunk[..], &[42u8; 10]);
    }

    #[derive(Clone, Copy, Debug, TypeGenerator)]
    enum Op {
        Put { len: u16 },
        ReadChunk { watermark: u16 },
        PartialCopy { watermark: u16 },
        FullCopy { watermark: u16 },
    }

    #[derive(Debug)]
    struct Model {
        buffer: Deque,
        send: Data,
        recv: Data,
    }

    impl Default for Model {
        fn default() -> Self {
            Self {
                buffer: Deque::new(u16::MAX as _),
                send: Data::new(usize::MAX as _),
                recv: Data::new(usize::MAX as _),
            }
        }
    }

    impl Model {
        fn apply_all(&mut self, ops: &[Op]) {
            for op in ops {
                self.apply(op);
            }
        }

        fn apply(&mut self, op: &Op) {
            match *op {
                Op::Put { len } => {
                    let mut stream = self.send.with_read_limit(len as _);
                    stream.infallible_copy_into(&mut self.buffer);
                }
                Op::ReadChunk { watermark } => {
                    let chunk = self.buffer.infallible_read_chunk(watermark as _);
                    self.recv.receive(&[chunk]);
                }
                Op::PartialCopy { watermark } => {
                    let mut recv = self.recv.with_write_limit(watermark as _);
                    let chunk = self.buffer.infallible_partial_copy_into(&mut recv);
                    self.recv.receive(&[chunk]);
                }
                Op::FullCopy { watermark } => {
                    let mut recv = self.recv.with_write_limit(watermark as _);
                    self.buffer.infallible_copy_into(&mut recv);
                }
            }
        }
    }

    #[test]
    fn model_test() {
        check!()
            .with_type::<Vec<Op>>()
            .for_each(|ops| Model::default().apply_all(ops))
    }
}

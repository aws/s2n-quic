// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use bolero::{check, TypeGenerator};
use std::io::{Read, Write};

macro_rules! assert_eq_dump {
    ($a:expr, $b:expr) => {
        assert_eq!($a, $b, "bytes mismatch");
    };
}

#[derive(Copy, Clone, Debug, TypeGenerator)]
enum Operation {
    Write { len: u16, zerocopy: bool },
    Read { len: u16, zerocopy: bool },
    Truncate { len: u16 },
    Advance { len: u16 },
    Clear,
    PushBack { len: u16 },
    SplitTo { len: u16, zerocopy: bool },
    PopFront,
    PopBack,
}

#[test]
fn model_test() {
    check!()
        .with_type::<Vec<Operation>>()
        .for_each(|operations| {
            let mut byte_source = (0u32..).flat_map(|v| v.to_be_bytes());
            let mut subject = ByteVec::new();
            let mut oracle: VecDeque<u8> = VecDeque::new();

            for operation in operations {
                match *operation {
                    Operation::Write { len, zerocopy } => {
                        let len = len as usize;

                        let mut chunk = BytesMut::with_capacity(len);
                        chunk.extend((&mut byte_source).take(len));
                        let chunk = chunk.freeze();

                        oracle.extend(chunk.iter());

                        if zerocopy {
                            subject.put_bytes(chunk);
                        } else {
                            subject.write_all(&chunk).unwrap();
                        }
                    }
                    Operation::Read { len, zerocopy } => {
                        let len = len as usize;

                        let read_len = if zerocopy {
                            let chunk = subject.infallible_read_chunk(len);
                            let read_len = chunk.buffered_len();

                            assert_eq!(oracle.make_contiguous()[..read_len], *chunk);
                            read_len
                        } else {
                            let mut buf = vec![0; len];
                            let read_len = subject.read(&mut buf).unwrap();
                            buf.truncate(read_len);

                            assert_eq!(oracle.make_contiguous()[..read_len], buf);
                            read_len
                        };

                        oracle.drain(..read_len);
                    }
                    Operation::Truncate { len } => {
                        let len = len as usize;

                        oracle.truncate(len);
                        subject.truncate(len);
                    }
                    Operation::Advance { len } => {
                        let len = len as usize;

                        let res = subject.advance(len);
                        assert_eq!(res.is_ok(), oracle.len() >= len);

                        if oracle.len() >= len {
                            oracle.drain(..len);
                        }
                    }
                    Operation::Clear => {
                        oracle.clear();
                        subject.clear();
                    }
                    Operation::PushBack { len } => {
                        let len = len as usize;
                        let mut chunk = BytesMut::with_capacity(len);
                        chunk.extend((&mut byte_source).take(len));
                        let chunk = chunk.freeze();
                        oracle.extend(chunk.iter());
                        subject.push_back(chunk);
                    }
                    Operation::SplitTo { len, zerocopy } => {
                        let len = len as usize;

                        if zerocopy {
                            let res = subject.split_to(len);
                            assert_eq!(res.is_err(), len > oracle.len());
                            if let Ok(chunk) = res {
                                assert_eq!(chunk, &oracle.make_contiguous()[..len]);
                                oracle.drain(..len);
                            }
                        } else {
                            let res = subject.split_to_copy(len);
                            assert_eq!(res.is_err(), len > oracle.len());
                            if let Ok(chunk) = res {
                                assert_eq!(chunk, &oracle.make_contiguous()[..len]);
                                oracle.drain(..len);
                            }
                        }
                    }
                    Operation::PopFront => {
                        let chunk = subject.pop_front();
                        assert_eq!(chunk.is_some(), !oracle.is_empty());
                        if let Some(chunk) = chunk {
                            assert_eq!(chunk, &oracle.make_contiguous()[..chunk.len()]);
                            oracle.drain(..chunk.len());
                        }
                    }
                    Operation::PopBack => {
                        let chunk = subject.pop_back();
                        assert_eq!(chunk.is_some(), !oracle.is_empty());
                        if let Some(chunk) = chunk {
                            let offset = oracle.len() - chunk.len();
                            assert_eq!(chunk, &oracle.make_contiguous()[offset..]);
                            oracle.drain(offset..);
                        }
                    }
                }

                assert_eq!(oracle.len(), subject.len());
                assert_eq!(oracle.is_empty(), subject.is_empty());
            }

            assert!(oracle.iter().eq(subject.chunks().flat_map(|v| v.iter())));
        });
}

#[test]
fn iter_test() {
    check!().with_type::<Vec<Vec<u8>>>().for_each(|chunks| {
        let mut subject = ByteVec::new();
        for chunk in chunks {
            subject.write_all(chunk).unwrap();
        }

        // the ByteVec does not store empty chunks so filter those out
        let expected = chunks.iter().filter(|chunk| !chunk.is_empty());

        for (actual, expected) in subject.chunks().zip(expected.clone()) {
            assert_eq!(actual, expected);
        }

        for (actual, expected) in subject.into_iter().zip(expected) {
            assert_eq!(actual, expected);
        }
    });
}

#[derive(Copy, Clone, Debug, TypeGenerator)]
enum BuilderOperation {
    Write { len: u16, is_bytes: bool },
    SplitTo { at: u16 },
    Append { len: u16 },
    WriteLenPrefix { len: u16, is_bytes: bool },
}

#[derive(Clone, Default)]
struct ByteSource {
    counter: u32,
    offset: u8,
}

impl Iterator for ByteSource {
    type Item = u8;

    fn next(&mut self) -> Option<Self::Item> {
        let byte = self.counter.to_be_bytes()[self.offset as usize];
        let next_offset = self.offset + 1;
        if next_offset == 4 {
            self.counter += 1;
            self.offset = 0;
        } else {
            self.offset = next_offset;
        }
        Some(byte)
    }
}

#[test]
fn byte_source_test() {
    for count in 0..16 {
        let actual: Vec<u8> = ByteSource::default().take(count).collect();
        let expected: Vec<u8> = (0u32..).flat_map(|v| v.to_be_bytes()).take(count).collect();
        assert_eq_dump!(actual, expected);
    }
}

#[derive(Default)]
struct BuilderModel {
    builder: Builder,
    oracle: Vec<u8>,
    byte_source: ByteSource,
}

impl BuilderModel {
    fn apply(&mut self, op: BuilderOperation) {
        let Self {
            builder,
            oracle,
            byte_source,
        } = self;

        use BuilderOperation::*;

        match op {
            Write { len, is_bytes } => {
                let len = len as usize;
                let mut chunk = BytesMut::with_capacity(len);
                chunk.extend(byte_source.take(len));
                let chunk_bytes = chunk.freeze();

                oracle.extend_from_slice(&chunk_bytes);

                if is_bytes {
                    builder.put_bytes(chunk_bytes);
                } else {
                    builder.put_slice(&chunk_bytes);
                }
            }
            SplitTo { at } => {
                let at = at as usize;
                if at <= oracle.len() {
                    let split = builder.split_to(at).unwrap();
                    assert_eq_dump!(split.copy_to_bytes(), &oracle[..at]);
                    oracle.drain(..at);
                } else {
                    assert!(matches!(
                        builder.split_to(at),
                        Err(ByteVecError::OutOfBounds(_))
                    ));
                }
            }
            Append { len } => {
                let len = len as usize;
                let mut chunk = BytesMut::with_capacity(len);
                chunk.extend(byte_source.take(len));
                let chunk_bytes = chunk.freeze();

                let mut bytes = ByteVec::new();
                bytes.push_back(chunk_bytes.clone());

                oracle.extend_from_slice(&chunk_bytes);
                builder.append(&mut bytes);
            }
            WriteLenPrefix { len, is_bytes } => {
                let len = len as usize;
                let mut chunk = BytesMut::with_capacity(len);
                chunk.extend(byte_source.take(len));
                let chunk_bytes = chunk.freeze();

                // Add length prefix to oracle
                oracle.extend_from_slice(&(chunk_bytes.len() as u64).to_be_bytes());
                // Add actual bytes
                oracle.extend_from_slice(&chunk_bytes);

                // Write to builder with length prefix
                builder.write_with_len_prefix(|b| {
                    if is_bytes {
                        b.put_bytes(chunk_bytes)
                    } else {
                        b.put_slice(&chunk_bytes)
                    }
                });
            }
        }

        assert_eq!(builder.len(), oracle.len());
        assert_eq!(builder.is_empty(), oracle.is_empty());
    }

    fn finish(self) {
        let final_bytes = self.builder.finish();
        assert_eq_dump!(final_bytes, self.oracle);
    }
}

#[test]
fn builder_model_test() {
    check!()
        .with_type::<Vec<BuilderOperation>>()
        .for_each(|operations| {
            let mut model = BuilderModel::default();

            for operation in operations {
                model.apply(*operation);
            }

            model.finish();
        });
}

#[test]
fn builder_capacity_test() {
    // Test large capacity behavior
    let mut large_builder = Builder::new(1 << 16);
    large_builder.put_slice(b"hello");
    large_builder.put_slice(b"world");
    let large_vec = large_builder.finish();
    assert_eq!(large_vec.len(), 10);
    assert_eq!(large_vec.chunks().len(), 1);

    // Test small capacity behavior
    let mut small_builder = Builder::new(1);
    small_builder.put_slice(b"hello");
    small_builder.put_slice(b"world");
    let small_vec = small_builder.finish();
    assert_eq!(small_vec.len(), 10);
    assert_eq!(small_vec.chunks().len(), 2);
}

/// Shows that if a written length spans an allocated chunk,
/// it should be atomically written so it can be inserted in the
/// correct location in the `ByteVec`.
#[test]
fn builder_write_len_prefix_torn_length() {
    use BuilderOperation::*;

    let ops = [
        Write {
            len: 65530,
            is_bytes: false,
        },
        WriteLenPrefix {
            len: 65535,
            is_bytes: false,
        },
    ];

    let mut model = BuilderModel::default();

    for op in ops {
        model.apply(op);
    }

    model.finish();
}

#[test]
fn builder_write_with_len_prefix_test() {
    // Test basic length-prefixed write
    let mut builder = Builder::new(16);
    builder.write_with_len_prefix(|b| b.put_slice(b"hello"));
    let result = builder.finish();
    let bytes = result.copy_to_bytes();
    assert_eq!(bytes.len(), 13); // 8 bytes for length + 5 bytes for "hello"
    let len = u64::from_be_bytes(bytes[..8].try_into().unwrap());
    assert_eq!(len as usize, 5);
    assert_eq!(&bytes[8..], b"hello");

    // Test multiple length-prefixed writes
    let mut builder = Builder::new(32);
    builder.write_with_len_prefix(|b| b.put_slice(b"hello"));
    builder.write_with_len_prefix(|b| b.put_slice(b"world"));
    let result = builder.finish();
    let bytes = result.copy_to_bytes();
    assert_eq!(bytes.len(), 26); // (8 + 5) + (8 + 5) bytes

    // First value
    let len1 = u64::from_be_bytes(bytes[..8].try_into().unwrap());
    assert_eq!(len1 as usize, 5);
    assert_eq!(&bytes[8..13], b"hello");

    // Second value
    let len2 = u64::from_be_bytes(bytes[13..21].try_into().unwrap());
    assert_eq!(len2 as usize, 5);
    assert_eq!(&bytes[21..], b"world");

    // Test empty value
    let mut builder = Builder::new(8);
    builder.write_with_len_prefix(|_| {});
    let result = builder.finish();
    let bytes = result.copy_to_bytes();
    assert_eq!(bytes.len(), 8); // Just the length prefix
    let len = u64::from_be_bytes(bytes[..8].try_into().unwrap());
    assert_eq!(len, 0);
}

#[derive(Copy, Clone, Debug, TypeGenerator)]
enum BuilderReaderOperation {
    WriteSlice { len: u8 },
    WriteBytes { len: u8 },
    Append { len: u8 },
    ReadPartialCopy { capacity: u8 },
    ReadCopyInto { capacity: u8 },
    ReadChunk { watermark: u8 },
}

#[test]
fn builder_reader_model_test() {
    check!()
        .with_type::<(u8, Vec<BuilderReaderOperation>)>()
        .for_each(|(builder_capacity, operations)| {
            // use a small capacity to ensure chunks and head are exercised
            let capacity = (*builder_capacity as usize % 64).max(1);
            let mut builder = Builder::new(capacity);
            let mut oracle: VecDeque<u8> = VecDeque::new();
            let mut byte_source = ByteSource::default();

            for operation in operations {
                match *operation {
                    BuilderReaderOperation::WriteSlice { len } => {
                        let len = len as usize;
                        let data: Vec<u8> = (&mut byte_source).take(len).collect();
                        oracle.extend(data.iter());
                        builder.put_slice(&data);
                    }
                    BuilderReaderOperation::WriteBytes { len } => {
                        let len = len as usize;
                        let data: Vec<u8> = (&mut byte_source).take(len).collect();
                        oracle.extend(data.iter());
                        builder.put_bytes(Bytes::from(data));
                    }
                    BuilderReaderOperation::Append { len } => {
                        let len = len as usize;
                        let data: Vec<u8> = (&mut byte_source).take(len).collect();
                        oracle.extend(data.iter());
                        let mut bv = ByteVec::new();
                        bv.push_back(Bytes::from(data));
                        builder.append(&mut bv);
                    }
                    BuilderReaderOperation::ReadPartialCopy { capacity: cap } => {
                        let cap = cap as usize;

                        // Read from builder using partial_copy_into with a limited dest
                        let mut dest = BytesMut::with_capacity(cap);
                        let mut limited = dest.with_write_limit(cap);
                        let chunk = builder.partial_copy_into(&mut limited).unwrap();
                        drop(limited);

                        // The dest should have been filled up to capacity (or all data)
                        let expected_written = cap.min(oracle.len());
                        let total_read = dest.len() + chunk.len();
                        assert!(
                            total_read <= expected_written,
                            "read more than expected: {total_read} > {expected_written}"
                        );

                        // Verify dest bytes match oracle
                        let oracle_slice = oracle.make_contiguous();
                        assert_eq_dump!(&dest[..], &oracle_slice[..dest.len()]);

                        // Verify trailing chunk matches oracle
                        let chunk_offset = dest.len();
                        assert_eq_dump!(
                            &chunk[..],
                            &oracle_slice[chunk_offset..chunk_offset + chunk.len()]
                        );

                        // Drain what we read from oracle
                        oracle.drain(..total_read);
                    }
                    BuilderReaderOperation::ReadCopyInto { capacity: cap } => {
                        let cap = cap as usize;

                        // Read from builder using copy_into with a limited dest
                        let mut dest = BytesMut::with_capacity(cap);
                        {
                            let mut limited = dest.with_write_limit(cap);
                            builder.copy_into(&mut limited).unwrap();
                        }

                        let expected_written = cap.min(oracle.len());
                        assert_eq!(
                            dest.len(),
                            expected_written,
                            "copy_into should fill dest up to capacity"
                        );

                        let oracle_slice = oracle.make_contiguous();
                        assert_eq_dump!(&dest[..], &oracle_slice[..dest.len()]);

                        oracle.drain(..dest.len());
                    }
                    BuilderReaderOperation::ReadChunk { watermark } => {
                        let watermark = watermark as usize;

                        let chunk = builder.read_chunk(watermark).unwrap();
                        let chunk_len = chunk.len();

                        if !oracle.is_empty() && watermark > 0 {
                            let oracle_slice = oracle.make_contiguous();
                            assert_eq_dump!(&chunk[..], &oracle_slice[..chunk_len]);
                        }

                        oracle.drain(..chunk_len);
                    }
                }

                assert_eq!(
                    builder.buffered_len(),
                    oracle.len(),
                    "length mismatch after operation"
                );
            }

            // Drain remaining and verify
            let mut dest = ByteVec::new();
            builder.copy_into(&mut dest).unwrap();
            let oracle_bytes: Vec<u8> = oracle.into();
            assert_eq_dump!(dest, &oracle_bytes[..]);
        });
}

#[test]
fn builder_storage_test() {
    let mut builder = Builder::new(4);

    // Test put_slice with capacity boundaries
    builder.put_slice(b"test");
    builder.put_slice(b"more");
    let result = builder.finish();
    assert_eq!(result, b"testmore");

    // Test put_bytes and put_bytes_mut
    let mut builder = Builder::default();
    builder.put_bytes(Bytes::from_static(b"hello"));
    builder.put_bytes_mut({
        let mut b = BytesMut::new();
        b.extend_from_slice(b"world");
        b
    });
    let result = builder.finish();
    assert_eq!(result, b"helloworld");

    // Test uninit_slice
    let mut builder = Builder::new(4);
    builder
        .put_uninit_slice::<_, std::io::Error>(4, |slice| {
            slice.copy_from_slice(b"test");
            Ok(())
        })
        .unwrap();
    let result = builder.finish();
    assert_eq!(result, b"test");
}

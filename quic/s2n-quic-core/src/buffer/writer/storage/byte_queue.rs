// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::buffer::writer::Storage;
use alloc::{collections::VecDeque, vec::Vec};
use bytes::{Bytes, BytesMut};

/// Implements a queue of bytes, capable of zero-copy transfer of data
macro_rules! impl_queue {
    ($ty:ident, $push:ident) => {
        impl Storage for $ty<Bytes> {
            const SPECIALIZES_BYTES: bool = true;
            const SPECIALIZES_BYTES_MUT: bool = true;

            #[inline]
            fn put_slice(&mut self, bytes: &[u8]) {
                self.put_bytes(Bytes::copy_from_slice(bytes));
            }

            #[inline]
            fn remaining_capacity(&self) -> usize {
                usize::MAX
            }

            #[inline]
            fn has_remaining_capacity(&self) -> bool {
                true
            }

            #[inline]
            fn put_bytes(&mut self, bytes: Bytes) {
                self.$push(bytes);
            }

            #[inline]
            fn put_bytes_mut(&mut self, bytes: BytesMut) {
                self.$push(bytes.freeze());
            }
        }

        impl Storage for $ty<BytesMut> {
            const SPECIALIZES_BYTES_MUT: bool = true;

            #[inline]
            fn put_slice(&mut self, bytes: &[u8]) {
                self.put_bytes_mut(BytesMut::from(bytes));
            }

            #[inline]
            fn remaining_capacity(&self) -> usize {
                usize::MAX
            }

            #[inline]
            fn has_remaining_capacity(&self) -> bool {
                true
            }

            #[inline]
            fn put_bytes(&mut self, bytes: Bytes) {
                // we can't convert Bytes into BytesMut so we'll need to copy it
                self.put_slice(&bytes);
            }

            #[inline]
            fn put_bytes_mut(&mut self, bytes: BytesMut) {
                self.$push(bytes);
            }
        }
    };
}

impl_queue!(Vec, push);
impl_queue!(VecDeque, push_back);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_queue_test() {
        let mut writer: Vec<BytesMut> = vec![];

        writer.put_slice(b"hello");
        writer.put_bytes(Bytes::from_static(b" "));
        writer.put_bytes_mut(BytesMut::from(&b"world"[..]));

        assert_eq!(
            writer,
            vec![
                BytesMut::from(&b"hello"[..]),
                BytesMut::from(&b" "[..]),
                BytesMut::from(&b"world"[..])
            ]
        );
    }
}

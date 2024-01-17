// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    buffer::{
        reader::{storage::Chunk, Storage},
        writer,
    },
    ensure,
};

macro_rules! impl_slice {
    ($ty:ty, $split:ident) => {
        impl Storage for $ty {
            type Error = core::convert::Infallible;

            #[inline]
            fn buffered_len(&self) -> usize {
                self.len()
            }

            #[inline]
            fn buffer_is_empty(&self) -> bool {
                self.is_empty()
            }

            #[inline]
            fn read_chunk(&mut self, watermark: usize) -> Result<Chunk, Self::Error> {
                ensure!(!self.is_empty(), Ok(Chunk::empty()));
                let len = self.len().min(watermark);
                // use `take` to work around borrowing rules
                let (chunk, remaining) = core::mem::take(self).$split(len);
                *self = remaining;
                Ok((&*chunk).into())
            }

            #[inline]
            fn partial_copy_into<Dest>(&mut self, dest: &mut Dest) -> Result<Chunk, Self::Error>
            where
                Dest: writer::Storage + ?Sized,
            {
                self.read_chunk(dest.remaining_capacity())
            }

            #[inline]
            fn copy_into<Dest>(&mut self, dest: &mut Dest) -> Result<(), Self::Error>
            where
                Dest: writer::Storage + ?Sized,
            {
                ensure!(!self.is_empty(), Ok(()));
                let len = self.len().min(dest.remaining_capacity());
                // use `take` to work around borrowing rules
                let (chunk, remaining) = core::mem::take(self).$split(len);
                *self = remaining;
                dest.put_slice(chunk);
                Ok(())
            }
        }
    };
}

impl_slice!(&[u8], split_at);
impl_slice!(&mut [u8], split_at_mut);

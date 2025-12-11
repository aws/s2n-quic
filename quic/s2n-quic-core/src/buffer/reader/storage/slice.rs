// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

// ignore these for macro consistency
#![allow(
    clippy::mem_replace_with_default,
    clippy::redundant_closure_call,
    unused_mut
)]

use crate::buffer::{
    reader::{storage::Chunk, Storage},
    writer,
};

macro_rules! impl_slice {
    ($ty:ty, $default:expr, $split:ident $(, $extend:expr, $new:expr)?) => {
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
            fn read_chunk(&mut self, watermark: usize) -> Result<Chunk<'_>, Self::Error> {
                ensure!(!self.is_empty(), Ok(Chunk::empty()));
                let len = self.len().min(watermark);
                // use `take` to work around borrowing rules
                let mut v = core::mem::replace(self, $default);
                let (chunk, remaining) = v.$split(len);
                $(
                    let chunk = ($extend)(chunk);
                    let remaining = ($extend)(remaining);
                )?
                $(
                    let remaining = ($new)(remaining);
                )?
                *self = remaining;
                Ok((&*chunk).into())
            }

            #[inline]
            fn partial_copy_into<Dest>(&mut self, dest: &mut Dest) -> Result<Chunk<'_>, Self::Error>
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
                let mut v = core::mem::replace(self, $default);
                let (chunk, remaining) = v.$split(len);
                dest.put_slice(chunk);
                $(
                    let remaining = ($extend)(remaining);
                    let remaining = ($new)(remaining);
                )?
                *self = remaining;
                Ok(())
            }
        }
    };
}

impl_slice!(&[u8], &[], split_at);
impl_slice!(&mut [u8], &mut [], split_at_mut);

#[cfg(feature = "std")]
impl_slice!(
    std::io::IoSlice<'_>,
    std::io::IoSlice::new(&[]),
    split_at,
    |chunk: &[u8]| unsafe {
        // SAFETY: we're using transmute to extend the lifetime of the chunk to `self`
        // Upstream tracking: https://github.com/rust-lang/rust/issues/124659
        core::mem::transmute::<&[u8], &[u8]>(chunk)
    },
    std::io::IoSlice::new
);
#[cfg(feature = "std")]
impl_slice!(
    std::io::IoSliceMut<'_>,
    std::io::IoSliceMut::new(&mut []),
    split_at_mut,
    |chunk: &mut [u8]| unsafe {
        // SAFETY: we're using transmute to extend the lifetime of the chunk to `self`
        // Upstream tracking: https://github.com/rust-lang/rust/issues/124659
        core::mem::transmute::<&mut [u8], &mut [u8]>(chunk)
    },
    std::io::IoSliceMut::new
);

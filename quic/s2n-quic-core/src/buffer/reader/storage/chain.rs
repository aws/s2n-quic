// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::buffer::{
    reader::{storage::Chunk, Storage},
    writer,
};

/// Chains two [`Storage`] implementations together, draining the first before the second.
pub struct Chain<A, B> {
    a: A,
    b: B,
}

impl<A, B> Chain<A, B>
where
    A: Storage<Error = core::convert::Infallible>,
    B: Storage<Error = core::convert::Infallible>,
{
    #[inline]
    pub fn new(a: A, b: B) -> Self {
        Self { a, b }
    }
}

impl<A, B> Storage for Chain<A, B>
where
    A: Storage<Error = core::convert::Infallible>,
    B: Storage<Error = core::convert::Infallible>,
{
    type Error = core::convert::Infallible;

    #[inline]
    fn buffered_len(&self) -> usize {
        self.a.buffered_len() + self.b.buffered_len()
    }

    #[inline]
    fn buffer_is_empty(&self) -> bool {
        self.a.buffer_is_empty() && self.b.buffer_is_empty()
    }

    #[inline]
    fn read_chunk(&mut self, watermark: usize) -> Result<Chunk<'_>, Self::Error> {
        if !self.a.buffer_is_empty() {
            return self.a.read_chunk(watermark);
        }
        self.b.read_chunk(watermark)
    }

    #[inline]
    fn partial_copy_into<Dest>(&mut self, dest: &mut Dest) -> Result<Chunk<'_>, Self::Error>
    where
        Dest: writer::Storage + ?Sized,
    {
        if !self.a.buffer_is_empty() {
            if self.a.buffered_len() > dest.remaining_capacity() {
                return self.a.partial_copy_into(dest);
            }
            self.a.copy_into(dest)?;
        }

        if !dest.has_remaining_capacity() {
            return Ok(Chunk::empty());
        }

        self.b.partial_copy_into(dest)
    }

    #[inline]
    fn copy_into<Dest>(&mut self, dest: &mut Dest) -> Result<(), Self::Error>
    where
        Dest: writer::Storage + ?Sized,
    {
        if !self.a.buffer_is_empty() {
            self.a.copy_into(dest)?;
        }

        if !dest.has_remaining_capacity() {
            return Ok(());
        }

        self.b.copy_into(dest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chain_read_chunk_drains_a_then_b() {
        let a: &[u8] = b"hello";
        let b: &[u8] = b" world";
        let mut chain = Chain::new(a, b);

        let chunk = chain.read_chunk(5).unwrap();
        assert_eq!(&*chunk, b"hello");

        let chunk = chain.read_chunk(6).unwrap();
        assert_eq!(&*chunk, b" world");

        assert!(chain.buffer_is_empty());
    }

    #[test]
    fn chain_partial_copy_fills_across_boundary() {
        let a: &[u8] = b"AB";
        let b: &[u8] = b"CDEF";
        let mut chain = Chain::new(a, b);

        let mut dest = [0u8; 5];
        let mut target = &mut dest[..];
        let chunk = chain.partial_copy_into(&mut target).unwrap();

        // a fully copied into dest, then b returns its trailing chunk (fits in remaining 3 bytes)
        assert_eq!(&dest[..2], b"AB");
        assert_eq!(&*chunk, b"CDE");
    }

    #[test]
    fn chain_copy_into_drains_both() {
        let a: &[u8] = b"pre";
        let b: &[u8] = b"fix";
        let mut chain = Chain::new(a, b);

        let mut dest: Vec<u8> = Vec::new();
        chain.copy_into(&mut dest).unwrap();

        assert_eq!(&dest, b"prefix");
        assert!(chain.buffer_is_empty());
    }

    #[test]
    fn chain_empty_a() {
        let a: &[u8] = b"";
        let b: &[u8] = b"only-b";
        let mut chain = Chain::new(a, b);

        assert_eq!(chain.buffered_len(), 6);

        let chunk = chain.read_chunk(10).unwrap();
        assert_eq!(&*chunk, b"only-b");
    }

    #[test]
    fn chain_empty_b() {
        let a: &[u8] = b"only-a";
        let b: &[u8] = b"";
        let mut chain = Chain::new(a, b);

        assert_eq!(chain.buffered_len(), 6);

        let chunk = chain.read_chunk(10).unwrap();
        assert_eq!(&*chunk, b"only-a");
    }

    #[test]
    fn chain_dest_smaller_than_a() {
        let a: &[u8] = b"ABCDEF";
        let b: &[u8] = b"GHI";
        let mut chain = Chain::new(a, b);

        let mut dest = [0u8; 3];
        let mut target = &mut dest[..];
        let chunk = chain.partial_copy_into(&mut target).unwrap();

        // a exceeds dest capacity, so a.partial_copy_into returns trailing chunk
        assert_eq!(&*chunk, b"ABC");
        // remaining: "DEF" in a + "GHI" in b
        assert_eq!(chain.buffered_len(), 6);
    }
}

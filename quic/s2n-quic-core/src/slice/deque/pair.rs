// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::{mem::MaybeUninit, ops};

pub struct Pair<Part> {
    parts: [Part; 2],
}

impl<Part> From<(Part, Part)> for Pair<Part> {
    #[inline]
    fn from((head, tail): (Part, Part)) -> Self {
        Self {
            parts: [head, tail],
        }
    }
}

impl<Part> From<Pair<Part>> for (Part, Part) {
    #[inline]
    fn from(pair: Pair<Part>) -> (Part, Part) {
        let [head, tail] = pair.parts;
        (head, tail)
    }
}

impl<T> Pair<T> {
    #[inline]
    pub fn map<F, U>(self, mut f: F) -> Pair<U>
    where
        F: FnMut(T) -> U,
    {
        let [head, tail] = self.parts;
        let head = f(head);
        let tail = f(tail);
        Pair {
            parts: [head, tail],
        }
    }
}

impl<Part, T> Pair<Part>
where
    Part: ops::Deref<Target = [T]>,
{
    #[inline]
    pub fn get(&self, mut index: usize) -> Option<&T> {
        for part in &self.parts {
            if let Some(v) = part.get(index) {
                return Some(v);
            };
            index -= part.len();
        }

        None
    }

    #[inline]
    pub fn iter<'a>(&'a self) -> impl Iterator<Item = &'a T>
    where
        T: 'a,
    {
        self.parts.iter().flat_map(|p| p.iter())
    }
}

impl<Part, T> Pair<Part>
where
    Part: ops::DerefMut<Target = [T]>,
{
    #[inline]
    pub fn get_mut(&mut self, mut index: usize) -> Option<&mut T> {
        for part in &mut self.parts {
            let part_len = part.len();
            if let Some(v) = part.get_mut(index) {
                return Some(v);
            };
            index -= part_len;
        }

        None
    }

    #[inline]
    pub fn iter_mut<'a>(&'a mut self) -> impl Iterator<Item = &'a mut T> + 'a
    where
        T: 'a,
    {
        self.parts.iter_mut().flat_map(|p| p.iter_mut())
    }
}

impl<'a, T> Pair<&'a [MaybeUninit<T>]> {
    /// # Safety
    ///
    /// Callers should guarantee the memory in the pair is initialized
    #[inline]
    pub unsafe fn assume_init_ref(self) -> Pair<&'a [T]> {
        self.map(|slice| {
            // SAFETY: similar justification for assume_init_mut
            &*(slice as *const [MaybeUninit<T>] as *const [T])
        })
    }
}

impl<'a, T> Pair<&'a mut [MaybeUninit<T>]> {
    /// # Safety
    ///
    /// Callers should guarantee the memory in the pair is initialized
    #[inline]
    pub unsafe fn assume_init_mut(self) -> Pair<&'a mut [T]> {
        self.map(|slice| {
            // SAFETY: casting `slice` to a `*mut [T]` is safe since the caller guarantees that
            // `slice` is initialized, and `MaybeUninit` is guaranteed to have the same layout as `T`.
            // The pointer obtained is valid since it refers to memory owned by `slice` which is a
            // reference and thus guaranteed to be valid for reads and writes.
            &mut *(slice as *mut [MaybeUninit<T>] as *mut [T])
        })
    }
}

#[cfg(feature = "std")]
mod std_ {
    use super::*;
    use std::io::{IoSlice, IoSliceMut};

    impl<'a> Pair<&'a [MaybeUninit<u8>]> {
        /// # Safety
        ///
        /// Callers should guarantee the memory in the pair is initialized
        #[inline]
        pub unsafe fn assume_init_io_slice(self) -> Pair<IoSlice<'a>> {
            self.assume_init_ref().map(IoSlice::new)
        }
    }

    #[cfg(feature = "std")]
    impl<'a> Pair<&'a mut [MaybeUninit<u8>]> {
        /// # Safety
        ///
        /// Callers should guarantee the memory in the pair is initialized
        #[inline]
        pub unsafe fn assume_init_io_slice_mut(self) -> Pair<IoSliceMut<'a>> {
            self.assume_init_mut().map(IoSliceMut::new)
        }
    }
}

#[cfg(feature = "alloc")]
mod alloc_ {
    use super::*;
    use crate::buffer::{reader, writer};
    use bytes::buf::UninitSlice;

    impl<S: reader::Storage> reader::Storage for Pair<S> {
        type Error = S::Error;

        #[inline]
        fn buffered_len(&self) -> usize {
            self.parts[0].buffered_len() + self.parts[1].buffered_len()
        }

        #[inline]
        fn read_chunk(
            &mut self,
            watermark: usize,
        ) -> Result<reader::storage::Chunk<'_>, Self::Error> {
            if !self.parts[0].buffer_is_empty() {
                self.parts[0].read_chunk(watermark)
            } else {
                self.parts[1].read_chunk(watermark)
            }
        }

        #[inline]
        fn partial_copy_into<Dest>(
            &mut self,
            dest: &mut Dest,
        ) -> Result<reader::storage::Chunk<'_>, Self::Error>
        where
            Dest: crate::buffer::writer::Storage + ?Sized,
        {
            if self.parts[0].buffered_len() >= dest.remaining_capacity() {
                self.parts[0].partial_copy_into(dest)
            } else {
                self.parts[0].copy_into(dest)?;
                self.parts[1].partial_copy_into(dest)
            }
        }

        #[inline]
        fn copy_into<Dest>(&mut self, dest: &mut Dest) -> Result<(), Self::Error>
        where
            Dest: writer::Storage + ?Sized,
        {
            self.parts[0].copy_into(dest)?;
            self.parts[1].copy_into(dest)
        }
    }

    impl<Part> Pair<Part>
    where
        Part: reader::Storage,
    {
        #[inline]
        pub fn reader_slice(&mut self) -> &[Part] {
            let [head, tail] = &self.parts;
            match (!head.buffer_is_empty(), !tail.buffer_is_empty()) {
                (true, true) => &self.parts,
                (true, false) => &self.parts[..1],
                (false, true) => &self.parts[1..],
                (false, false) => &[],
            }
        }

        #[inline]
        pub fn reader_slice_mut(&mut self) -> &mut [Part] {
            let [head, tail] = &self.parts;
            match (!head.buffer_is_empty(), !tail.buffer_is_empty()) {
                (true, true) => &mut self.parts,
                (true, false) => &mut self.parts[..1],
                (false, true) => &mut self.parts[1..],
                (false, false) => &mut [],
            }
        }
    }

    impl<S: writer::Storage> writer::Storage for Pair<S> {
        #[inline]
        fn put_slice(&mut self, mut bytes: &[u8]) {
            use reader::storage::Infallible;

            debug_assert!(bytes.len() <= self.remaining_capacity());

            bytes.infallible_copy_into(&mut self.parts[0]);
            bytes.infallible_copy_into(&mut self.parts[1]);
        }

        #[inline]
        fn put_uninit_slice<F, Error>(&mut self, payload_len: usize, f: F) -> Result<bool, Error>
        where
            F: FnOnce(&mut UninitSlice) -> Result<(), Error>,
        {
            if self.parts[0].has_remaining_capacity() {
                self.parts[0].put_uninit_slice(payload_len, f)
            } else {
                self.parts[1].put_uninit_slice(payload_len, f)
            }
        }

        #[inline]
        fn remaining_capacity(&self) -> usize {
            self.parts[0].remaining_capacity() + self.parts[1].remaining_capacity()
        }
    }

    impl<Part> Pair<Part>
    where
        Part: writer::Storage,
    {
        #[inline]
        pub fn writer_slice(&mut self) -> &[Part] {
            let [head, tail] = &self.parts;
            match (head.has_remaining_capacity(), tail.has_remaining_capacity()) {
                (true, true) => &self.parts,
                (true, false) => &self.parts[..1],
                (false, true) => &self.parts[1..],
                (false, false) => &[],
            }
        }

        #[inline]
        pub fn writer_slice_mut(&mut self) -> &mut [Part] {
            let [head, tail] = &self.parts;
            match (head.has_remaining_capacity(), tail.has_remaining_capacity()) {
                (true, true) => &mut self.parts,
                (true, false) => &mut self.parts[..1],
                (false, true) => &mut self.parts[1..],
                (false, false) => &mut [],
            }
        }
    }
}

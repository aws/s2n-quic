// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::{cell::UnsafeCell, mem::MaybeUninit, ops::Deref};

#[repr(transparent)]
pub struct Cell<T>(MaybeUninit<UnsafeCell<T>>);

impl<T> Cell<T> {
    #[inline]
    pub unsafe fn write(&self, value: T) {
        UnsafeCell::raw_get(self.0.as_ptr()).write(value);
    }

    #[inline]
    pub unsafe fn take(&self) -> T {
        self.0.assume_init_ref().get().read()
    }
}

#[derive(Debug)]
pub struct Slice<'a, T>(pub(super) &'a [T]);

impl<'a, T> Slice<'a, Cell<T>> {
    /// Assumes that the slice of [`Cell`]s is initialized and converts it to a slice of
    /// [`UnsafeCell`]s.
    ///
    /// See [`core::mem::MaybeUninit::assume_init`]
    #[inline]
    pub unsafe fn assume_init(self) -> Slice<'a, UnsafeCell<T>> {
        Slice(&*(self.0 as *const [Cell<T>] as *const [UnsafeCell<T>]))
    }

    /// Writes a value into a cell at the provided index
    ///
    /// # Safety
    ///
    /// The cell at `index` must be uninitialized and the caller must have synchronized access.
    #[inline]
    pub unsafe fn write(&self, index: usize, value: T) {
        self.0.get_unchecked(index).write(value)
    }

    /// Reads and takes the memory at a cell at the provided index
    ///
    /// # Safety
    ///
    /// The cell at `index` must be initialized and the caller must have synchronized access.
    #[inline]
    pub unsafe fn take(&self, index: usize) -> T {
        self.0.get_unchecked(index).take()
    }
}

impl<'a, T> Slice<'a, UnsafeCell<T>> {
    /// Converts the slice of [`UnsafeCell`]s into a mutable slice
    ///
    /// # Safety
    ///
    /// The slice must be exclusively owned, otherwise data races may occur.
    #[inline]
    pub unsafe fn into_mut(self) -> &'a mut [T] {
        let ptr = self.0.as_ptr() as *mut T;
        let len = self.0.len();
        core::slice::from_raw_parts_mut(ptr, len)
    }
}

impl<T> Deref for Slice<'_, T> {
    type Target = [T];

    #[inline]
    fn deref(&self) -> &[T] {
        self.0
    }
}

impl<T: PartialEq> PartialEq<[T]> for Slice<'_, UnsafeCell<T>> {
    #[inline]
    fn eq(&self, other: &[T]) -> bool {
        if self.len() != other.len() {
            return false;
        }

        for (a, b) in self.iter().zip(other) {
            if unsafe { &*a.get() } != b {
                return false;
            }
        }

        true
    }
}

impl<'a, T: PartialEq> PartialEq<Slice<'a, UnsafeCell<T>>> for [T] {
    #[inline]
    fn eq(&self, other: &Slice<'a, UnsafeCell<T>>) -> bool {
        other.eq(self)
    }
}

impl<'a, T: PartialEq> PartialEq<Slice<'a, UnsafeCell<T>>> for &[T] {
    #[inline]
    fn eq(&self, other: &Slice<'a, UnsafeCell<T>>) -> bool {
        other.eq(self)
    }
}

#[derive(Debug)]
pub struct Pair<S> {
    pub head: S,
    pub tail: S,
}

impl<'a, T> Pair<Slice<'a, Cell<T>>> {
    #[inline]
    pub unsafe fn assume_init(self) -> Pair<Slice<'a, UnsafeCell<T>>> {
        Pair {
            head: self.head.assume_init(),
            tail: self.tail.assume_init(),
        }
    }

    #[inline]
    pub unsafe fn write(&self, index: usize, value: T) {
        self.cell(index).write(value)
    }

    #[inline]
    pub unsafe fn take(&self, index: usize) -> T {
        self.cell(index).take()
    }

    unsafe fn cell(&self, index: usize) -> &Cell<T> {
        if let Some(cell) = self.head.0.get(index) {
            cell
        } else {
            assume!(
                index >= self.head.0.len(),
                "index must always be equal or greater than the `head` len"
            );
            let index = index - self.head.0.len();

            assume!(
                self.tail.get(index).is_some(),
                "index must be in-bounds for the `tail` slice: head={}, tail={}, index={}",
                self.head.0.len(),
                self.tail.0.len(),
                index
            );
            self.tail.get_unchecked(index)
        }
    }

    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = &Cell<T>> {
        self.head.0.iter().chain(self.tail.0)
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.head.len() + self.tail.len()
    }
}

impl<'a, T> Pair<Slice<'a, UnsafeCell<T>>> {
    #[inline]
    pub unsafe fn into_mut(self) -> (&'a mut [T], &'a mut [T]) {
        let head = self.head.into_mut();
        let tail = self.tail.into_mut();
        (head, tail)
    }
}

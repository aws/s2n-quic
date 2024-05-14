// Copyright The Rust Project Developers
// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Utilities for working with ring buffers
//!
//! Most of the logic is derived/copied from Rust's VecDeque implementation:
//!
//! [`VecDeque`](https://github.com/rust-lang/rust/blob/master/library/alloc/src/collections/vec_deque/mod.rs)

use core::ptr;

mod pair;

pub use pair::Pair;

/// Checks that the current state preserves the required invariants
#[inline]
pub fn invariants<T>(slice: &[T], head: usize, len: usize) -> bool {
    let cap = slice.len();

    ensure!(cap > 0, false);
    ensure!(cap > head, false);
    ensure!(cap >= len, false);

    true
}

#[inline]
pub fn wrap<T>(slice: &[T], head: usize, len: usize) -> usize {
    assert!(invariants(slice, head, len));

    let cursor = head + len;

    if slice.len().is_power_of_two() {
        cursor & (slice.len() - 1)
    } else {
        cursor % slice.len()
    }
}

/// Returns the `tail` position for the current state
#[inline]
fn tail<T>(slice: &[T], head: usize, len: usize) -> usize {
    assert!(invariants(slice, head, len));
    wrap(slice, head, len)
}

/// Returns the filled pair of slices for the current state
#[inline]
pub fn filled<T>(slice: &mut [T], head: usize, len: usize) -> Pair<&mut [T]> {
    assert!(invariants(slice, head, len));

    let tail = tail(slice, head, len);

    // if the slice is contiguous then return the single slice
    if is_contiguous(slice, head, len) {
        let head = &mut slice[head..tail];
        let tail: &mut [T] = &mut [];
        debug_assert_eq!(head.len(), len);

        return (head, tail).into();
    }

    let (bytes, head) = slice.split_at_mut(head);
    let (tail, _unfilled) = bytes.split_at_mut(tail);

    if head.is_empty() {
        debug_assert!(tail.is_empty());
    }

    debug_assert_eq!(head.len() + tail.len(), len);

    (head, tail).into()
}

/// Returns the unfilled pair of slices for the current state
#[inline]
pub fn unfilled<T>(slice: &mut [T], head: usize, len: usize) -> Pair<&mut [T]> {
    assert!(invariants(slice, head, len));

    let cap = slice.len();
    let tail = tail(slice, head, len);
    let remaining_capacity = cap - len;

    // if the slice is non-contiguous then return the unfilled space between
    if !is_contiguous(slice, head, len) {
        let head = &mut slice[tail..head];
        let tail: &mut [T] = &mut [];

        debug_assert_eq!(head.len(), remaining_capacity);

        return (head, tail).into();
    }

    let (slice, head_slice) = slice.split_at_mut(tail);
    let (tail_slice, _filled) = slice.split_at_mut(head);

    let head = head_slice;
    let tail = tail_slice;

    debug_assert!(!head.is_empty());

    debug_assert_eq!(head.len() + tail.len(), remaining_capacity);

    (head, tail).into()
}

/// Returns `true` if the currently occupied elements are contiguous
#[inline]
pub fn is_contiguous<T>(slice: &[T], head: usize, len: usize) -> bool {
    assert!(invariants(slice, head, len));

    head + len < slice.len()
}

/// Forces all of the currently occupied elements to be contiguous
#[inline]
pub fn make_contiguous<T>(slice: &mut [T], head_out: &mut usize, len: usize) {
    let head = *head_out;

    assert!(invariants(slice, head, len));

    // we only need to shuffle things if we're non-contiguous
    ensure!(!is_contiguous(slice, head, len));

    let cap = slice.len();

    debug_assert!(len <= cap);
    debug_assert!(head <= cap);

    let free = cap - len;
    let head_len = cap - head;
    let tail = len - head_len;
    let tail_len = tail;

    if free >= head_len {
        // there is enough free space to copy the head in one go,
        // this means that we first shift the tail backwards, and then
        // copy the head to the correct position.
        //
        // from: DEFGH....ABC
        // to:   ABCDEFGH....
        unsafe {
            copy(slice, 0, head_len, tail_len);
            // ...DEFGH.ABC
            copy_nonoverlapping(slice, head, 0, head_len);
            // ABCDEFGH....
        }

        *head_out = 0;
    } else if free >= tail_len {
        // there is enough free space to copy the tail in one go,
        // this means that we first shift the head forwards, and then
        // copy the tail to the correct position.
        //
        // from: FGH....ABCDE
        // to:   ...ABCDEFGH.
        unsafe {
            copy(slice, head, tail, head_len);
            // FGHABCDE....
            copy_nonoverlapping(slice, 0, tail + head_len, tail_len);
            // ...ABCDEFGH.
        }

        *head_out = tail;
    } else {
        // `free` is smaller than both `head_len` and `tail_len`.
        // the general algorithm for this first moves the slices
        // right next to each other and then uses `slice::rotate`
        // to rotate them into place:
        //
        // initially:   HIJK..ABCDEFG
        // step 1:      ..HIJKABCDEFG
        // step 2:      ..ABCDEFGHIJK
        //
        // or:
        //
        // initially:   FGHIJK..ABCDE
        // step 1:      FGHIJKABCDE..
        // step 2:      ABCDEFGHIJK..

        // pick the shorter of the 2 slices to reduce the amount
        // of memory that needs to be moved around.
        if head_len > tail_len {
            // tail is shorter, so:
            //  1. copy tail forwards
            //  2. rotate used part of the buffer
            //  3. update head to point to the new beginning (which is just `free`)

            unsafe {
                // if there is no free space in the buffer, then the slices are already
                // right next to each other and we don't need to move any memory.
                if free != 0 {
                    // because we only move the tail forward as much as there's free space
                    // behind it, we don't overwrite any elements of the head slice, and
                    // the slices end up right next to each other.
                    copy(slice, 0, free, tail_len);
                }

                // We just copied the tail right next to the head slice,
                // so all of the elements in the range are initialized
                let slice = &mut slice[free..cap];

                // because the deque wasn't contiguous, we know that `tail_len < self.len == slice.len()`,
                // so this will never panic.
                slice.rotate_left(tail_len);

                // the used part of the buffer now is `free..self.capacity()`, so set
                // `head` to the beginning of that range.
                *head_out = free;
            }
        } else {
            // head is shorter so:
            //  1. copy head backwards
            //  2. rotate used part of the buffer
            //  3. update head to point to the new beginning (which is the beginning of the buffer)

            unsafe {
                // if there is no free space in the buffer, then the slices are already
                // right next to each other and we don't need to move any memory.
                if free != 0 {
                    // copy the head slice to lie right behind the tail slice.
                    copy(slice, head, tail_len, head_len);
                }

                // because we copied the head slice so that both slices lie right
                // next to each other, all the elements in the range are initialized.
                let slice = &mut slice[..len];

                // because the deque wasn't contiguous, we know that `head_len < self.len == slice.len()`
                // so this will never panic.
                slice.rotate_right(head_len);

                // the used part of the buffer now is `0..self.len`, so set
                // `head` to the beginning of that range.
                *head_out = 0;
            }
        }
    }
}

/// Copies a contiguous block of memory len long from src to dst
#[inline]
unsafe fn copy<T>(slice: &mut [T], src: usize, dst: usize, len: usize) {
    debug_assert!(
        dst + len <= slice.len(),
        "cpy dst={} src={} len={} cap={}",
        dst,
        src,
        len,
        slice.len()
    );
    debug_assert!(
        src + len <= slice.len(),
        "cpy dst={} src={} len={} cap={}",
        dst,
        src,
        len,
        slice.len()
    );
    let ptr = slice.as_mut_ptr();
    unsafe {
        ptr::copy(ptr.add(src), ptr.add(dst), len);
    }
}

/// Copies a contiguous block of memory len long from src to dst
#[inline]
unsafe fn copy_nonoverlapping<T>(slice: &mut [T], src: usize, dst: usize, len: usize) {
    debug_assert!(
        dst + len <= slice.len(),
        "cno dst={} src={} len={} cap={}",
        dst,
        src,
        len,
        slice.len()
    );
    debug_assert!(
        src + len <= slice.len(),
        "cno dst={} src={} len={} cap={}",
        dst,
        src,
        len,
        slice.len()
    );
    let ptr = slice.as_mut_ptr();
    unsafe {
        ptr::copy_nonoverlapping(ptr.add(src), ptr.add(dst), len);
    }
}

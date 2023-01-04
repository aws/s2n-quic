// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{Cell, ClosedError, Result, Slice};
use alloc::alloc::Layout;
use atomic_waker::AtomicWaker;
use core::{
    marker::PhantomData,
    ops::Deref,
    panic::{RefUnwindSafe, UnwindSafe},
    ptr::NonNull,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};

type Pair<'a, T> = super::Pair<Slice<'a, Cell<T>>>;

#[derive(Clone, Copy, Debug)]
pub struct Cursor {
    head: usize,
    tail: usize,
    capacity: usize,
}

impl Cursor {
    #[inline]
    fn new(capacity: usize) -> Self {
        Self {
            head: 0,
            tail: 0,
            capacity,
        }
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.head == self.tail
    }

    #[inline]
    pub fn is_full(&self) -> bool {
        self.recv_len() == self.capacity
    }

    #[inline]
    pub fn recv_len(&self) -> usize {
        self.distance(self.head, self.tail)
    }

    #[inline]
    pub fn send_capacity(&self) -> usize {
        self.capacity - self.recv_len()
    }

    #[inline]
    pub fn increment_head(&mut self, len: usize) {
        self.head = self.increment(self.head, len);
    }

    #[inline]
    pub fn increment_tail(&mut self, len: usize) {
        self.tail = self.increment(self.tail, len);
    }

    #[inline]
    fn wrapped_head(&self) -> usize {
        self.collapse_position(self.head)
    }

    #[inline]
    fn wrapped_tail(&self) -> usize {
        self.collapse_position(self.tail)
    }

    /// Returns the distance between two positions.
    #[inline]
    fn distance(&self, a: usize, b: usize) -> usize {
        unsafe {
            unsafe_assert!(a == 0 || a < 2 * self.capacity);
            unsafe_assert!(b == 0 || b < 2 * self.capacity);
        }
        if a <= b {
            b - a
        } else {
            2 * self.capacity - a + b
        }
    }

    #[inline]
    fn increment(&self, pos: usize, n: usize) -> usize {
        unsafe {
            unsafe_assert!(pos == 0 || pos < 2 * self.capacity);
            unsafe_assert!(n <= self.capacity);
        }
        let threshold = 2 * self.capacity - n;
        if pos < threshold {
            pos + n
        } else {
            pos - threshold
        }
    }

    #[inline]
    fn collapse_position(&self, pos: usize) -> usize {
        unsafe {
            unsafe_assert!(pos == 0 || pos < 2 * self.capacity);
        }
        if pos < self.capacity {
            pos
        } else {
            pos - self.capacity
        }
    }
}

#[derive(Debug)]
pub struct State<T> {
    header: NonNull<Header<T>>,
    pub cursor: Cursor,
}

unsafe impl<T: Send> Send for State<T> {}
unsafe impl<T: Sync> Sync for State<T> {}
impl<T: RefUnwindSafe> UnwindSafe for State<T> {}

impl<T> Clone for State<T> {
    #[inline]
    fn clone(&self) -> Self {
        Self {
            header: self.header,
            cursor: self.cursor,
        }
    }
}

impl<T> Deref for State<T> {
    type Target = Header<T>;

    #[inline]
    fn deref(&self) -> &Self::Target {
        unsafe { self.header.as_ref() }
    }
}

impl<T> State<T> {
    #[inline]
    pub fn new(capacity: usize) -> Self {
        let header = Header::alloc(capacity).expect("could not allocate channel");
        let cursor = Cursor::new(capacity);
        Self { header, cursor }
    }

    #[inline]
    pub fn acquire_capacity(&mut self) -> Result<bool> {
        if !self.open.load(Ordering::Acquire) {
            return Err(ClosedError);
        }

        if !self.cursor.is_full() {
            return Ok(true);
        }

        // update the cached version
        self.cursor.head = self.head.load(Ordering::Acquire);

        let is_full = self.cursor.is_full();

        Ok(!is_full)
    }

    #[inline]
    pub fn acquire_filled(&mut self) -> Result<bool> {
        if !self.cursor.is_empty() {
            return Ok(true);
        }

        self.cursor.tail = self.tail.load(Ordering::Acquire);

        if !self.cursor.is_empty() {
            return Ok(true);
        }

        if !self.open.load(Ordering::Acquire) {
            return Err(ClosedError);
        }

        Ok(false)
    }

    #[inline]
    pub fn persist_head(&self, prev: Cursor) {
        // nothing changed
        if prev.head == self.cursor.head {
            return;
        }

        self.head.store(self.cursor.head, Ordering::Release);
    }

    #[inline]
    pub fn persist_tail(&self, prev: Cursor) {
        // nothing changed
        if prev.tail == self.cursor.tail {
            return;
        }

        self.tail.store(self.cursor.tail, Ordering::Release);
    }

    #[inline]
    fn data(&self) -> &[Cell<T>] {
        unsafe {
            let ptr = self.data_ptr();
            // we don't multiply the capacity here for simplicity
            let capacity = self.cursor.capacity;
            core::slice::from_raw_parts(ptr, capacity)
        }
    }

    #[inline]
    fn data_ptr(&self) -> *const Cell<T> {
        unsafe {
            let capacity = self.cursor.capacity;
            let (_, offset) = Header::<T>::layout_unchecked(capacity);

            let ptr = self.header.as_ptr() as *const u8;
            let ptr = ptr.add(offset);
            ptr as *const Cell<T>
        }
    }

    #[inline]
    pub fn try_close(&mut self) -> bool {
        let prev = self.open.swap(false, Ordering::Acquire);
        if !prev {
            unsafe {
                self.drop_contents();
            }
        }
        prev
    }

    #[inline]
    pub fn as_pairs(&self) -> (Pair<T>, Pair<T>) {
        let data = self.data();
        self.data_to_pairs(data)
    }

    #[inline]
    fn data_to_pairs<'a>(&self, data: &'a [Cell<T>]) -> (Pair<'a, T>, Pair<'a, T>) {
        if self.cursor.is_full() {
            let head = self.cursor.wrapped_head();
            let (filled_tail, filled_head) = data.split_at(head);
            let filled = Pair {
                head: Slice(filled_head),
                tail: Slice(filled_tail),
            };
            let unfilled = Pair {
                head: Slice(&[]),
                tail: Slice(&[]),
            };
            return (filled, unfilled);
        }

        let head = self.cursor.wrapped_head();
        let tail = self.cursor.wrapped_tail();

        let is_contiguous = tail >= head;

        if is_contiguous {
            let (data, unfilled_head) = data.split_at(tail);
            let (unfilled_tail, filled_head) = data.split_at(head);

            let filled = Pair {
                head: Slice(filled_head),
                tail: Slice(&[]),
            };
            let unfilled = Pair {
                head: Slice(unfilled_head),
                tail: Slice(unfilled_tail),
            };
            (filled, unfilled)
        } else {
            let (data, filled_head) = data.split_at(head);
            let (filled_tail, unfilled_head) = data.split_at(tail);

            let filled = Pair {
                head: Slice(filled_head),
                tail: Slice(filled_tail),
            };
            let unfilled = Pair {
                head: Slice(unfilled_head),
                tail: Slice(&[]),
            };
            (filled, unfilled)
        }
    }

    #[inline]
    unsafe fn drop_contents(&mut self) {
        // refresh the cursor from the shared state
        self.cursor.head = self.head.load(Ordering::Acquire);
        self.cursor.tail = self.tail.load(Ordering::Acquire);

        // release all of the filled data
        let (filled, _unfilled) = self.as_pairs();
        for cell in filled.iter() {
            drop(cell.take());
        }

        // free the header
        let ptr = self.header.as_ptr() as *mut u8;
        let capacity = self.cursor.capacity;
        let (layout, _offset) = Header::<T>::layout_unchecked(capacity);
        alloc::alloc::dealloc(ptr, layout)
    }
}

#[derive(Debug)]
pub struct Header<T> {
    head: AtomicUsize,
    pub receiver: AtomicWaker,
    tail: AtomicUsize,
    pub sender: AtomicWaker,
    open: AtomicBool,
    data: PhantomData<T>,
}

impl<T> Header<T> {
    fn alloc(capacity: usize) -> Option<NonNull<Self>> {
        unsafe {
            let (layout, _offset) = Self::layout(capacity).ok()?;
            let state = alloc::alloc::alloc(layout);
            let state = state as *mut Self;
            let state = NonNull::new(state)?;

            state.as_ptr().write(Self::new());

            Some(state)
        }
    }

    #[inline]
    const fn new() -> Self {
        Self {
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
            sender: AtomicWaker::new(),
            receiver: AtomicWaker::new(),
            open: AtomicBool::new(true),
            data: PhantomData,
        }
    }

    #[inline]
    fn layout(capacity: usize) -> Result<(Layout, usize), alloc::alloc::LayoutError> {
        let header_layout = Layout::new::<Self>();
        let data_layout = Layout::array::<Cell<T>>(capacity)?;
        let (layout, offset) = header_layout.extend(data_layout)?;
        Ok((layout, offset))
    }

    #[inline]
    unsafe fn layout_unchecked(capacity: usize) -> (Layout, usize) {
        if let Ok(v) = Self::layout(capacity) {
            v
        } else {
            core::hint::unreachable_unchecked()
        }
    }
}

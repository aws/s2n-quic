// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{Cell, ClosedError, Result, Slice};
use crate::sync::primitive::{AtomicBool, AtomicUsize, AtomicWaker, IsZst, Ordering};
use alloc::alloc::Layout;
use cache_padded::CachePadded;
use core::{
    fmt,
    marker::PhantomData,
    ops::Deref,
    panic::{RefUnwindSafe, UnwindSafe},
    ptr::NonNull,
};

type Pair<'a, T> = super::Pair<Slice<'a, Cell<T>>>;

const MINIMUM_CAPACITY: usize = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Sender,
    Receiver,
}

#[derive(Clone, Copy)]
pub struct Cursor {
    head: usize,
    tail: usize,
    capacity: usize,
}

impl fmt::Debug for Cursor {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Cursor")
            .field("head", &self.head)
            .field("tail", &self.tail)
            .field("len", &self.len())
            .field("capacity", &self.capacity())
            .field("is_empty", &self.is_empty())
            .field("is_full", &self.is_full())
            .field("is_contiguous", &self.is_contiguous())
            .finish()
    }
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
    fn invariants(&self) {
        unsafe {
            assume!(
                self.capacity >= MINIMUM_CAPACITY,
                "the capacity must be at least the MINIMUM_CAPACITY value"
            );
            assume!(
                self.head < self.capacity,
                "the `head` pointer should be strictly less than the capacity"
            );
            assume!(
                self.tail < self.capacity,
                "the `tail` pointer should be strictly less than the capacity"
            );
            let len = count(self.head, self.tail, self.capacity);
            assume!(
                len < self.capacity,
                "the computed `len` should be strictly less than the capacity"
            );
        }
    }

    #[inline]
    pub fn capacity(&self) -> usize {
        self.invariants();
        // To make cursor management easier, we never allow the callers to hit the total capacity.
        // We also account for this when allocating the state by adding 1 to the request capacity.
        self.capacity - 1
    }

    #[inline]
    fn cap(&self) -> usize {
        self.invariants();
        self.capacity
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.invariants();
        count(self.head, self.tail, self.cap())
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.invariants();
        self.tail == self.head
    }

    #[inline]
    pub fn is_full(&self) -> bool {
        self.invariants();
        count(self.tail, self.head, self.cap()) == 1
    }

    #[inline]
    pub fn recv_len(&self) -> usize {
        self.invariants();
        self.len()
    }

    #[inline]
    pub fn send_capacity(&self) -> usize {
        self.invariants();
        self.capacity() - self.recv_len()
    }

    #[inline]
    pub fn increment_head(&mut self, n: usize) {
        self.invariants();
        unsafe {
            assume!(
                n <= self.capacity(),
                "n should never exceed the total capacity"
            );
        }
        self.head = self.wrap_add(self.head, n);
        self.invariants();
    }

    #[inline]
    pub fn increment_tail(&mut self, n: usize) {
        self.invariants();
        unsafe {
            assume!(
                n <= self.capacity(),
                "n should never exceed the total capacity"
            );
        }
        self.tail = self.wrap_add(self.tail, n);
        self.invariants();
    }

    #[inline]
    fn wrap_add(&self, idx: usize, addend: usize) -> usize {
        wrap_index(idx.wrapping_add(addend), self.cap())
    }

    #[inline]
    fn is_contiguous(&self) -> bool {
        self.tail >= self.head
    }
}

/// Returns the index in the underlying buffer for a given logical element index.
#[inline]
fn wrap_index(index: usize, size: usize) -> usize {
    // size is always a power of 2
    unsafe {
        assume!(
            size.is_power_of_two(),
            "The calculations in the lengths rely on the capacity being a power of 2"
        );
        assume!(
            size >= MINIMUM_CAPACITY,
            "The calculations in the lengths rely on the capacity being at least {}",
            MINIMUM_CAPACITY
        );
    }
    index & (size - 1)
}

/// Calculate the number of elements left to be read in the buffer
#[inline]
fn count(head: usize, tail: usize, size: usize) -> usize {
    // size is always a power of 2
    unsafe {
        assume!(
            size.is_power_of_two(),
            "The calculations in the lengths rely on the capacity being a power of 2"
        );
        assume!(
            size >= MINIMUM_CAPACITY,
            "The calculations in the lengths rely on the capacity being at least {}",
            MINIMUM_CAPACITY
        );
    }
    (tail.wrapping_sub(head)) & (size - 1)
}

/// The synchronized state between two peers
///
/// The internal design of the cursor management is based on [`alloc::collections::VecDeque`].
pub struct State<T> {
    header: NonNull<Header<T>>,
    pub cursor: Cursor,
}

impl<T> fmt::Debug for State<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("State")
            .field("header", self.deref())
            .field("cursor", &self.cursor)
            .finish()
    }
}

/// Safety: synchronization of state is managed through atomic values
unsafe impl<T: Send> Send for State<T> {}

/// Safety: synchronization of state is managed through atomic values
unsafe impl<T: Sync> Sync for State<T> {}

/// The data behind the header pointer itself is unwind safe
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
        // If we're sending a zero-sized type, set the capacity to the maximum value, since we're
        // not sending any data and just coordinating cursors at this point
        let capacity = if T::IS_ZST {
            // The total capacity must be a power of two
            usize::MAX / 2 + 1
        } else {
            // Add 1 to the requested capacity so it's easier to manage cursor wrapping
            core::cmp::max(capacity + 1, MINIMUM_CAPACITY).next_power_of_two()
        };
        let header = Header::alloc(capacity).expect("could not allocate channel");
        let cursor = Cursor::new(capacity);
        Self { header, cursor }
    }

    /// Tries to acquire more unfilled slots on the channel
    ///
    /// If the channel is closed, an error is returned. If the channel has at least one slot of
    /// capacity, `true` is returned. Otherwise `false` is returned.
    #[inline]
    pub fn acquire_capacity(&mut self) -> Result<bool> {
        if !self.open.load(Ordering::Acquire) {
            return Err(ClosedError);
        }

        // update the cached version
        self.cursor.head = self.head.load(Ordering::Acquire);

        let is_full = self.cursor.is_full();

        Ok(!is_full)
    }

    /// Tries to acquire more filled slots on the channel
    ///
    /// If the channel is closed, an error is returned. If the channel has at least one slot of
    /// capacity, `true` is returned. Otherwise `false` is returned.
    #[inline]
    pub fn acquire_filled(&mut self) -> Result<bool> {
        self.cursor.tail = self.tail.load(Ordering::Acquire);

        if !self.cursor.is_empty() {
            return Ok(true);
        }

        if !self.open.load(Ordering::Acquire) {
            // make one more effort to load the remaining items
            self.cursor.tail = self.tail.load(Ordering::Acquire);

            if !self.cursor.is_empty() {
                return Ok(true);
            }

            return Err(ClosedError);
        }

        Ok(false)
    }

    /// Notifies the peer of `head` updates for the given cursor
    #[inline]
    pub fn persist_head(&self, prev: Cursor) {
        // nothing changed
        if prev.head == self.cursor.head {
            return;
        }

        self.head.store(self.cursor.head, Ordering::Release);

        self.sender.wake();
    }

    /// Notifies the peer of `tail` updates for the given cursor
    #[inline]
    pub fn persist_tail(&self, prev: Cursor) {
        // nothing changed
        if prev.tail == self.cursor.tail {
            return;
        }

        self.tail.store(self.cursor.tail, Ordering::Release);

        self.receiver.wake();
    }

    #[inline]
    fn data(&self) -> &[Cell<T>] {
        unsafe {
            // Safety: the state must still be allocated and the cursor inbounds
            let ptr = self.data_ptr();
            let capacity = self.cursor.capacity;
            core::slice::from_raw_parts(ptr, capacity)
        }
    }

    #[inline]
    fn data_ptr(&self) -> *const Cell<T> {
        unsafe {
            // If the type is zero-sized, no need to calculate offsets
            if T::IS_ZST {
                return NonNull::<Cell<T>>::dangling().as_ptr();
            }

            // Safety: the state must still be allocated and the cursor inbounds
            let capacity = self.cursor.capacity;
            let (_, offset) = Header::<T>::layout_unchecked(capacity);

            let ptr = self.header.as_ptr() as *const u8;
            let ptr = ptr.add(offset);
            ptr as *const Cell<T>
        }
    }

    /// Closes one side of the channel and notifies the peer of the event
    #[inline]
    pub fn close(&mut self, side: Side) {
        // notify the other side that we've closed the channel
        match side {
            Side::Sender => self.receiver.wake(),
            Side::Receiver => self.sender.wake(),
        }

        let was_open = self.open.swap(false, Ordering::SeqCst);

        // make sure the peer is notified before fully dropping the contents
        match side {
            Side::Sender => self.receiver.wake(),
            Side::Receiver => self.sender.wake(),
        }

        if !was_open {
            unsafe {
                // Safety: we synchronization closing between the two peers through atomic
                // variables. At this point both sides have agreed on its final state.
                self.drop_contents();
            }
        }
    }

    /// Returns the channel slots as two pairs of filled and unfilled slices
    #[inline]
    pub fn as_pairs(&self) -> (Pair<T>, Pair<T>) {
        let data = self.data();
        self.data_to_pairs(data)
    }

    #[inline]
    fn data_to_pairs<'a>(&self, data: &'a [Cell<T>]) -> (Pair<'a, T>, Pair<'a, T>) {
        self.cursor.invariants();

        let head = self.cursor.head;
        let tail = self.cursor.tail;

        let (filled, unfilled) = if self.cursor.is_contiguous() {
            unsafe {
                assume!(data.len() >= tail, "data must span the tail length");
            }
            let (data, unfilled_head) = data.split_at(tail);

            unsafe {
                assume!(data.len() >= head, "data must span the head length");
            }
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
            unsafe {
                assume!(data.len() >= head, "data must span the head length");
            }
            let (data, filled_head) = data.split_at(head);

            unsafe {
                assume!(data.len() >= tail, "data must span the tail length");
            }
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
        };

        unsafe {
            assume!(
                filled.len() == self.cursor.recv_len(),
                "filled len should agree with the cursor len {} == {}\n{:?}",
                filled.len(),
                self.cursor.recv_len(),
                self.cursor
            );
        }

        (filled, unfilled)
    }

    /// Frees the contents of the channel
    ///
    /// # Safety
    ///
    /// Each side must have synchronized and agreed on the final state before calling this
    #[inline]
    unsafe fn drop_contents(&mut self) {
        // refresh the cursor from the shared state
        self.cursor.head = self.head.load(Ordering::Acquire);
        self.cursor.tail = self.tail.load(Ordering::Acquire);

        // release all of the filled data
        let (filled, _unfilled) = self.as_pairs();
        if !T::IS_ZST {
            for cell in filled.iter() {
                drop(cell.take());
            }
        }

        // make sure we free any stored wakers
        let header = self.header.as_mut();
        drop(header.receiver.take());
        drop(header.sender.take());

        // free the header
        let ptr = self.header.as_ptr() as *mut u8;
        let capacity = self.cursor.capacity;
        let (layout, _offset) = Header::<T>::layout_unchecked(capacity);
        alloc::alloc::dealloc(ptr, layout)
    }
}

pub struct Header<T> {
    head: CachePadded<AtomicUsize>,
    tail: CachePadded<AtomicUsize>,
    open: CachePadded<AtomicBool>,
    pub receiver: AtomicWaker,
    pub sender: AtomicWaker,
    data: PhantomData<T>,
}

impl<T> fmt::Debug for Header<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Header")
            .field("head", &self.head.load(Ordering::Relaxed))
            .field("tail", &self.tail.load(Ordering::Relaxed))
            .field("open", &self.open.load(Ordering::Relaxed))
            .field("receiver", &self.receiver)
            .field("sender", &self.sender)
            .finish()
    }
}

impl<T> Header<T> {
    /// Allocates a header and data slice for the given capacity
    fn alloc(capacity: usize) -> Option<NonNull<Self>> {
        unsafe {
            // Safety: we assume that `alloc` gives us a valid pointer to write to
            let (layout, _offset) = Self::layout(capacity).ok()?;
            let state = alloc::alloc::alloc(layout);
            let state = state as *mut Self;
            let state = NonNull::new(state)?;

            state.as_ptr().write(Self::new());

            Some(state)
        }
    }

    #[inline]
    fn new() -> Self {
        Self {
            head: CachePadded::new(AtomicUsize::new(0)),
            tail: CachePadded::new(AtomicUsize::new(0)),
            sender: AtomicWaker::new(),
            receiver: AtomicWaker::new(),
            open: CachePadded::new(AtomicBool::new(true)),
            data: PhantomData,
        }
    }

    /// Computes the checked layout for the header
    #[inline]
    fn layout(capacity: usize) -> Result<(Layout, usize), alloc::alloc::LayoutError> {
        let header_layout = Layout::new::<Self>();
        // A slice of cells is allocated in the same region as the header
        let data_layout = Layout::array::<Cell<T>>(capacity)?;
        let (layout, offset) = header_layout.extend(data_layout)?;
        Ok((layout, offset))
    }

    /// Computes the memory layout of the header without checking of its validatity
    ///
    /// # Safety
    ///
    /// The layout must have been previously checked before calling this.
    #[inline]
    unsafe fn layout_unchecked(capacity: usize) -> (Layout, usize) {
        if let Ok(v) = Self::layout(capacity) {
            v
        } else {
            core::hint::unreachable_unchecked()
        }
    }
}

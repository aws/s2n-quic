// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::fmt;
use std::{
    ops::{Deref, DerefMut},
    ptr::NonNull,
};

/// An entry in the intrusive queue
///
/// Contains the value and links to the previous and next entries.
pub struct Entry<T>(Box<Inner<T>>);

type Link<T> = NonNull<Inner<T>>;

struct Inner<T> {
    value: T,
    prev: Option<Link<T>>,
    next: Option<Link<T>>,
}

unsafe impl<T: Send> Send for Entry<T> {}
unsafe impl<T: Sync> Sync for Entry<T> {}

impl<T> Inner<T> {
    #[inline(always)]
    fn assert_unlinked(&self) {
        if cfg!(debug_assertions) {
            debug_assert!(self.prev.is_none());
            debug_assert!(self.next.is_none());
        }
    }
}

impl<T: fmt::Debug> fmt::Debug for Entry<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.value.fmt(f)
    }
}

impl<T: Clone> Clone for Entry<T> {
    fn clone(&self) -> Self {
        self.0.assert_unlinked();
        Self::new(self.0.value.clone())
    }
}

impl<T: Default> Default for Entry<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

impl<T> From<T> for Entry<T> {
    fn from(value: T) -> Self {
        Self::new(value)
    }
}

impl<T> Entry<T> {
    /// Create a new entry with the given value
    pub fn new(value: T) -> Self {
        let inner = Inner {
            value,
            prev: None,
            next: None,
        };
        Self(Box::new(inner))
    }

    /// Consume the entry and return the value
    pub fn into_inner(self) -> T {
        let inner = self.0;
        inner.assert_unlinked();
        inner.value
    }

    #[inline(always)]
    fn assert_unlinked(&self) {
        if cfg!(debug_assertions) {
            self.0.assert_unlinked();
        }
    }
}

impl<T> Deref for Entry<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0.value
    }
}

impl<T> DerefMut for Entry<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0.value
    }
}

impl<T: crate::socket::channel::ByteCost> crate::socket::channel::ByteCost for Entry<T> {
    fn byte_cost(&self) -> u64 {
        (**self).byte_cost()
    }
}

impl<T: crate::socket::channel::Sendable> crate::socket::channel::Sendable for Entry<T> {
    fn send<S: crate::socket::send::Socket>(&mut self, socket: &S) -> std::io::Result<()> {
        (**self).send(socket)
    }
}

/// An intrusive FIFO queue
///
/// This is a doubly-linked list where elements are pushed to the back
/// and popped from the front. The queue owns all entries through Box pointers.
pub struct Queue<T> {
    head: Option<Link<T>>,
    tail: Option<Link<T>>,
    len: usize,
}

unsafe impl<T: Send> Send for Queue<T> {}
unsafe impl<T: Sync> Sync for Queue<T> {}

impl<T: fmt::Debug> fmt::Debug for Queue<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

impl<T> Queue<T> {
    /// Create a new empty queue
    pub const fn new() -> Self {
        Self {
            head: None,
            tail: None,
            len: 0,
        }
    }

    /// Returns true if the queue is empty
    pub fn is_empty(&self) -> bool {
        self.head.is_none()
    }

    /// Returns the number of entries in the queue
    pub fn len(&self) -> usize {
        self.len
    }

    /// Peek at the first entry without removing it
    pub fn front(&self) -> Option<&T> {
        self.head.map(|head| unsafe { &(*head.as_ptr()).value })
    }

    /// Peek at the last entry without removing it
    pub fn back(&self) -> Option<&T> {
        self.tail.map(|tail| unsafe { &(*tail.as_ptr()).value })
    }

    /// Push an entry to the back of the queue
    pub fn push_back(&mut self, entry: Entry<T>) {
        entry.assert_unlinked();

        // Leak the box to get a raw pointer we can store in the queue
        let new_tail = NonNull::from(Box::leak(entry.0));

        unsafe {
            // Set the new entry's prev pointer to the current tail
            (*new_tail.as_ptr()).prev = self.tail;

            // If there's a tail, link it to the new entry
            if let Some(tail) = self.tail {
                (*tail.as_ptr()).next = Some(new_tail);
            } else {
                // Queue was empty, this is the new head
                self.head = Some(new_tail);
            }
        }

        self.tail = Some(new_tail);
        self.len += 1;
    }

    /// Push an entry to the front of the queue
    pub fn push_front(&mut self, entry: Entry<T>) {
        entry.assert_unlinked();

        let new_head = NonNull::from(Box::leak(entry.0));

        unsafe {
            (*new_head.as_ptr()).next = self.head;

            if let Some(head) = self.head {
                (*head.as_ptr()).prev = Some(new_head);
            } else {
                self.tail = Some(new_head);
            }
        }

        self.head = Some(new_head);
        self.len += 1;
    }

    /// Pop an entry from the front of the queue
    ///
    /// Returns None if the queue is empty.
    pub fn pop_front(&mut self) -> Option<Entry<T>> {
        let head_ptr = self.head.take()?;

        unsafe {
            // Get the next pointer from the head
            let next = (*head_ptr.as_ptr()).next;
            self.head = next;

            // Update the new head's prev pointer
            if let Some(new_head) = self.head {
                (*new_head.as_ptr()).prev = None;
            } else {
                // Queue is now empty, clear tail
                self.tail = None;
            }

            // Clear the popped entry's pointers
            (*head_ptr.as_ptr()).prev = None;
            (*head_ptr.as_ptr()).next = None;

            self.len -= 1;

            // Reconstruct the Entry from the leaked box
            Some(Entry(Box::from_raw(head_ptr.as_ptr())))
        }
    }

    /// Pop an entry from the back of the queue
    ///
    /// Returns None if the queue is empty.
    pub fn pop_back(&mut self) -> Option<Entry<T>> {
        let tail_ptr = self.tail.take()?;

        unsafe {
            let prev = (*tail_ptr.as_ptr()).prev;
            self.tail = prev;

            if let Some(new_tail) = self.tail {
                (*new_tail.as_ptr()).next = None;
            } else {
                self.head = None;
            }

            (*tail_ptr.as_ptr()).prev = None;
            (*tail_ptr.as_ptr()).next = None;

            self.len -= 1;

            Some(Entry(Box::from_raw(tail_ptr.as_ptr())))
        }
    }

    /// Peek at the front entry without removing it
    pub fn peek_front(&self) -> Option<&T> {
        self.head.map(|head| unsafe { &(*head.as_ptr()).value })
    }

    /// Peek at the front entry mutably without removing it
    pub fn peek_front_mut(&mut self) -> Option<&mut T> {
        self.head.map(|head| unsafe { &mut (*head.as_ptr()).value })
    }

    /// Peek at the back entry without removing it
    pub fn peek_back(&self) -> Option<&T> {
        self.tail.map(|tail| unsafe { &(*tail.as_ptr()).value })
    }

    /// Peek at the back entry mutably without removing it
    pub fn peek_back_mut(&mut self) -> Option<&mut T> {
        self.tail.map(|tail| unsafe { &mut (*tail.as_ptr()).value })
    }

    /// Prepend another queue to the front of this queue.
    ///
    /// This is O(1) — just a pointer splice. The `other` queue is left empty.
    /// After this operation, entries from `other` appear before entries from `self`.
    pub fn prepend(&mut self, other: &mut Queue<T>) {
        other.append(self);
        core::mem::swap(self, other);
    }

    /// Append another queue to the back of this queue.
    ///
    /// This is O(1) — just a pointer splice. The `other` queue is left empty.
    pub fn append(&mut self, other: &mut Queue<T>) {
        let Some(other_head) = other.head.take() else {
            // other is empty
            return;
        };
        let other_tail = other.tail.take().unwrap();
        let other_len = other.len;
        other.len = 0;

        if let Some(tail) = self.tail {
            unsafe {
                (*tail.as_ptr()).next = Some(other_head);
                (*other_head.as_ptr()).prev = Some(tail);
            }
            self.tail = Some(other_tail);
        } else {
            // self is empty
            self.head = Some(other_head);
            self.tail = Some(other_tail);
        }

        self.len += other_len;
    }

    pub fn iter(&self) -> Iter<'_, T> {
        Iter {
            next: self.head,
            len: self.len,
            _phantom: std::marker::PhantomData,
        }
    }

    pub fn iter_mut(&mut self) -> IterMut<'_, T> {
        IterMut {
            next: self.head,
            len: self.len,
            _phantom: std::marker::PhantomData,
        }
    }

    pub fn drain(&mut self) -> IntoIter<T> {
        IntoIter {
            queue: core::mem::replace(self, Queue::new()),
        }
    }
}

impl<T> Default for Queue<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Drop for Queue<T> {
    fn drop(&mut self) {
        // Pop all entries to ensure proper cleanup
        while self.pop_front().is_some() {}
    }
}

pub struct Iter<'a, T> {
    next: Option<Link<T>>,
    len: usize,
    _phantom: std::marker::PhantomData<&'a T>,
}

impl<'a, T> Iterator for Iter<'a, T> {
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        let current = self.next.take()?;
        unsafe {
            let inner = &*current.as_ptr();
            self.next = inner.next;
            self.len -= 1;
            Some(&inner.value)
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.len;
        (len, Some(len))
    }
}

pub struct IterMut<'a, T> {
    next: Option<Link<T>>,
    len: usize,
    _phantom: std::marker::PhantomData<&'a mut T>,
}

impl<'a, T> Iterator for IterMut<'a, T> {
    type Item = &'a mut T;

    fn next(&mut self) -> Option<Self::Item> {
        let current = self.next.take()?;
        unsafe {
            let inner = &mut *current.as_ptr();
            self.next = inner.next;
            self.len -= 1;
            Some(&mut inner.value)
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.len;
        (len, Some(len))
    }
}

impl<T> IntoIterator for Queue<T> {
    type Item = Entry<T>;
    type IntoIter = IntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        IntoIter { queue: self }
    }
}

pub struct IntoIter<T> {
    queue: Queue<T>,
}

impl<T> Iterator for IntoIter<T> {
    type Item = Entry<T>;

    fn next(&mut self) -> Option<Self::Item> {
        self.queue.pop_front()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.queue.len();
        (len, Some(len))
    }
}

impl<T> FromIterator<Entry<T>> for Queue<T> {
    fn from_iter<I: IntoIterator<Item = Entry<T>>>(iter: I) -> Self {
        let mut queue = Queue::new();
        for item in iter {
            queue.push_back(item);
        }
        queue
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bolero::{check, TypeGenerator};
    use std::collections::VecDeque;

    #[test]
    fn test_push_pop() {
        let mut queue = Queue::new();

        assert!(queue.is_empty());
        assert!(queue.pop_front().is_none());

        queue.push_back(Entry::new(1));
        queue.push_back(Entry::new(2));
        queue.push_back(Entry::new(3));

        assert!(!queue.is_empty());

        let entry1 = queue.pop_front().unwrap();
        assert_eq!(*entry1, 1);

        let entry2 = queue.pop_front().unwrap();
        assert_eq!(*entry2, 2);

        let entry3 = queue.pop_front().unwrap();
        assert_eq!(*entry3, 3);

        assert!(queue.is_empty());
        assert!(queue.pop_front().is_none());
    }

    #[test]
    fn test_peek() {
        let mut queue = Queue::new();

        assert!(queue.peek_front().is_none());
        assert!(queue.peek_back().is_none());

        queue.push_back(Entry::new(1));
        assert_eq!(*queue.peek_front().unwrap(), 1);
        assert_eq!(*queue.peek_back().unwrap(), 1);

        queue.push_back(Entry::new(2));
        assert_eq!(*queue.peek_front().unwrap(), 1);
        assert_eq!(*queue.peek_back().unwrap(), 2);

        queue.push_back(Entry::new(3));
        assert_eq!(*queue.peek_front().unwrap(), 1);
        assert_eq!(*queue.peek_back().unwrap(), 3);
    }

    #[test]
    fn test_peek_mut() {
        let mut queue = Queue::new();

        queue.push_back(Entry::new(1));
        queue.push_back(Entry::new(2));

        *queue.peek_front_mut().unwrap() = 10;
        *queue.peek_back_mut().unwrap() = 20;

        assert_eq!(*queue.pop_front().unwrap(), 10);
        assert_eq!(*queue.pop_front().unwrap(), 20);
    }

    #[test]

    fn test_into_value() {
        let mut queue = Queue::new();

        queue.push_back(Entry::new(42));
        let entry = queue.pop_front().unwrap();
        let value = entry.into_inner();

        assert_eq!(value, 42);
    }

    #[test]
    fn test_single_element() {
        let mut queue = Queue::new();

        queue.push_back(Entry::new(100));
        assert!(!queue.is_empty());

        let entry = queue.pop_front().unwrap();
        assert_eq!(*entry, 100);

        assert!(queue.is_empty());
    }

    #[test]
    fn test_push_pop_interleaved() {
        let mut queue = Queue::new();

        queue.push_back(Entry::new(1));
        queue.push_back(Entry::new(2));

        assert_eq!(*queue.pop_front().unwrap(), 1);

        queue.push_back(Entry::new(3));
        queue.push_back(Entry::new(4));

        assert_eq!(*queue.pop_front().unwrap(), 2);
        assert_eq!(*queue.pop_front().unwrap(), 3);

        queue.push_back(Entry::new(5));

        assert_eq!(*queue.pop_front().unwrap(), 4);
        assert_eq!(*queue.pop_front().unwrap(), 5);

        assert!(queue.is_empty());
    }

    #[test]
    fn test_append_both_non_empty() {
        let mut a = Queue::new();
        a.push_back(Entry::new(1));
        a.push_back(Entry::new(2));

        let mut b = Queue::new();
        b.push_back(Entry::new(3));
        b.push_back(Entry::new(4));

        a.append(&mut b);

        assert!(b.is_empty());
        assert_eq!(*a.pop_front().unwrap(), 1);
        assert_eq!(*a.pop_front().unwrap(), 2);
        assert_eq!(*a.pop_front().unwrap(), 3);
        assert_eq!(*a.pop_front().unwrap(), 4);
        assert!(a.is_empty());
    }

    #[test]
    fn test_append_to_empty() {
        let mut a = Queue::new();
        let mut b = Queue::new();
        b.push_back(Entry::new(10));
        b.push_back(Entry::new(20));

        a.append(&mut b);

        assert!(b.is_empty());
        assert_eq!(*a.pop_front().unwrap(), 10);
        assert_eq!(*a.pop_front().unwrap(), 20);
        assert!(a.is_empty());
    }

    #[test]
    fn test_append_empty_other() {
        let mut a = Queue::new();
        a.push_back(Entry::new(1));

        let mut b = Queue::new();
        a.append(&mut b);

        assert_eq!(*a.pop_front().unwrap(), 1);
        assert!(a.is_empty());
    }

    #[test]
    fn test_append_both_empty() {
        let mut a: Queue<u64> = Queue::new();
        let mut b: Queue<u64> = Queue::new();
        a.append(&mut b);
        assert!(a.is_empty());
        assert!(b.is_empty());
    }

    #[test]
    fn test_append_peek() {
        let mut a = Queue::new();
        a.push_back(Entry::new(1));

        let mut b = Queue::new();
        b.push_back(Entry::new(2));

        a.append(&mut b);

        assert_eq!(*a.peek_front().unwrap(), 1);
        assert_eq!(*a.peek_back().unwrap(), 2);
    }

    #[test]
    fn test_push_front_empty() {
        let mut queue = Queue::new();
        queue.push_front(Entry::new(1));
        assert_eq!(queue.len(), 1);
        assert_eq!(*queue.peek_front().unwrap(), 1);
        assert_eq!(*queue.peek_back().unwrap(), 1);
        assert_eq!(*queue.pop_front().unwrap(), 1);
        assert!(queue.is_empty());
    }

    #[test]
    fn test_push_front_ordering() {
        let mut queue = Queue::new();
        queue.push_front(Entry::new(3));
        queue.push_front(Entry::new(2));
        queue.push_front(Entry::new(1));

        assert_eq!(*queue.pop_front().unwrap(), 1);
        assert_eq!(*queue.pop_front().unwrap(), 2);
        assert_eq!(*queue.pop_front().unwrap(), 3);
        assert!(queue.is_empty());
    }

    #[test]
    fn test_push_front_and_push_back_mixed() {
        let mut queue = Queue::new();
        queue.push_back(Entry::new(2));
        queue.push_front(Entry::new(1));
        queue.push_back(Entry::new(3));
        queue.push_front(Entry::new(0));

        assert_eq!(*queue.pop_front().unwrap(), 0);
        assert_eq!(*queue.pop_front().unwrap(), 1);
        assert_eq!(*queue.pop_front().unwrap(), 2);
        assert_eq!(*queue.pop_front().unwrap(), 3);
        assert!(queue.is_empty());
    }

    #[test]
    fn test_pop_back_empty() {
        let mut queue: Queue<u64> = Queue::new();
        assert!(queue.pop_back().is_none());
    }

    #[test]
    fn test_pop_back_single() {
        let mut queue = Queue::new();
        queue.push_back(Entry::new(42));
        assert_eq!(*queue.pop_back().unwrap(), 42);
        assert!(queue.is_empty());
        assert!(queue.pop_back().is_none());
        assert!(queue.pop_front().is_none());
    }

    #[test]
    fn test_pop_back_ordering() {
        let mut queue = Queue::new();
        queue.push_back(Entry::new(1));
        queue.push_back(Entry::new(2));
        queue.push_back(Entry::new(3));

        assert_eq!(*queue.pop_back().unwrap(), 3);
        assert_eq!(*queue.pop_back().unwrap(), 2);
        assert_eq!(*queue.pop_back().unwrap(), 1);
        assert!(queue.is_empty());
    }

    #[test]
    fn test_pop_front_and_pop_back_interleaved() {
        let mut queue = Queue::new();
        queue.push_back(Entry::new(1));
        queue.push_back(Entry::new(2));
        queue.push_back(Entry::new(3));
        queue.push_back(Entry::new(4));

        assert_eq!(*queue.pop_front().unwrap(), 1);
        assert_eq!(*queue.pop_back().unwrap(), 4);
        assert_eq!(*queue.pop_front().unwrap(), 2);
        assert_eq!(*queue.pop_back().unwrap(), 3);
        assert!(queue.is_empty());
    }

    #[test]
    fn test_prepend_both_non_empty() {
        let mut a = Queue::new();
        a.push_back(Entry::new(3));
        a.push_back(Entry::new(4));

        let mut b = Queue::new();
        b.push_back(Entry::new(1));
        b.push_back(Entry::new(2));

        a.prepend(&mut b);

        assert!(b.is_empty());
        assert_eq!(a.len(), 4);
        assert_eq!(*a.pop_front().unwrap(), 1);
        assert_eq!(*a.pop_front().unwrap(), 2);
        assert_eq!(*a.pop_front().unwrap(), 3);
        assert_eq!(*a.pop_front().unwrap(), 4);
        assert!(a.is_empty());
    }

    #[test]
    fn test_prepend_to_empty() {
        let mut a: Queue<u64> = Queue::new();
        let mut b = Queue::new();
        b.push_back(Entry::new(1));
        b.push_back(Entry::new(2));

        a.prepend(&mut b);

        assert!(b.is_empty());
        assert_eq!(*a.pop_front().unwrap(), 1);
        assert_eq!(*a.pop_front().unwrap(), 2);
    }

    #[test]
    fn test_prepend_empty_other() {
        let mut a = Queue::new();
        a.push_back(Entry::new(1));

        let mut b: Queue<u64> = Queue::new();
        a.prepend(&mut b);

        assert_eq!(a.len(), 1);
        assert_eq!(*a.pop_front().unwrap(), 1);
    }

    #[test]
    fn test_prepend_both_empty() {
        let mut a: Queue<u64> = Queue::new();
        let mut b: Queue<u64> = Queue::new();
        a.prepend(&mut b);
        assert!(a.is_empty());
        assert!(b.is_empty());
    }

    #[test]
    fn test_prepend_peek() {
        let mut a = Queue::new();
        a.push_back(Entry::new(2));

        let mut b = Queue::new();
        b.push_back(Entry::new(1));

        a.prepend(&mut b);

        assert_eq!(*a.peek_front().unwrap(), 1);
        assert_eq!(*a.peek_back().unwrap(), 2);
    }

    #[test]
    fn test_reverse_iteration_pattern() {
        // This is the exact pattern used by the wheel's cascade fix:
        // pop_back from source, push_front into destination
        let mut source = Queue::new();
        source.push_back(Entry::new(1));
        source.push_back(Entry::new(2));
        source.push_back(Entry::new(3));

        let mut dest = Queue::new();
        dest.push_back(Entry::new(4));
        dest.push_back(Entry::new(5));

        // Reverse-iterate source into front of dest
        while let Some(entry) = source.pop_back() {
            dest.push_front(entry);
        }

        assert!(source.is_empty());
        assert_eq!(*dest.pop_front().unwrap(), 1);
        assert_eq!(*dest.pop_front().unwrap(), 2);
        assert_eq!(*dest.pop_front().unwrap(), 3);
        assert_eq!(*dest.pop_front().unwrap(), 4);
        assert_eq!(*dest.pop_front().unwrap(), 5);
        assert!(dest.is_empty());
    }

    #[derive(Clone, Copy, Debug, TypeGenerator)]
    enum Operation {
        PushBack,
        PushFront,
        PopFront,
        PopBack,
        Append,
        Prepend,
    }

    #[test]
    fn differential_test() {
        check!().with_type::<Vec<Operation>>().for_each(|ops| {
            let mut values = 0u64..;
            let mut oracle = VecDeque::new();
            let mut subject = Queue::new();

            // secondary queue for append/prepend operations
            let mut oracle_other = VecDeque::new();
            let mut subject_other = Queue::new();

            for op in ops {
                match op {
                    Operation::PushBack => {
                        let value = values.next().unwrap();
                        oracle.push_back(value);
                        subject.push_back(Entry::new(value));
                        assert_eq!(oracle.len(), subject.len());

                        // also push to secondary so appends/prepends have something to work with
                        let value2 = values.next().unwrap();
                        oracle_other.push_back(value2);
                        subject_other.push_back(Entry::new(value2));
                    }
                    Operation::PushFront => {
                        let value = values.next().unwrap();
                        oracle.push_front(value);
                        subject.push_front(Entry::new(value));
                        assert_eq!(oracle.len(), subject.len());

                        let value2 = values.next().unwrap();
                        oracle_other.push_front(value2);
                        subject_other.push_front(Entry::new(value2));
                    }
                    Operation::PopFront => {
                        assert_eq!(oracle.pop_front(), subject.pop_front().map(|entry| *entry));
                        assert_eq!(oracle.len(), subject.len());
                    }
                    Operation::PopBack => {
                        assert_eq!(oracle.pop_back(), subject.pop_back().map(|entry| *entry));
                        assert_eq!(oracle.len(), subject.len());
                    }
                    Operation::Append => {
                        oracle.extend(oracle_other.drain(..));
                        subject.append(&mut subject_other);
                        assert!(subject_other.is_empty());
                        assert_eq!(oracle.len(), subject.len());
                    }
                    Operation::Prepend => {
                        let mut temp = oracle_other.drain(..).collect::<VecDeque<_>>();
                        temp.extend(oracle.drain(..));
                        oracle = temp;
                        subject.prepend(&mut subject_other);
                        assert!(subject_other.is_empty());
                        assert_eq!(oracle.len(), subject.len());
                    }
                }

                // Invariant: peek_front/peek_back match oracle
                assert_eq!(oracle.front().copied(), subject.peek_front().copied());
                assert_eq!(oracle.back().copied(), subject.peek_back().copied());
            }

            // Drain and verify final contents match
            while let Some(expected) = oracle.pop_front() {
                let actual = *subject.pop_front().unwrap();
                assert_eq!(expected, actual);
            }
            assert!(subject.pop_front().is_none());
        })
    }
}

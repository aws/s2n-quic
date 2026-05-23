// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::{cell::Cell, fmt};
use std::{
    ops::{self, Deref, DerefMut},
    ptr::NonNull,
};

// ── Adapter Trait ──────────────────────────────────────────────────────────

/// Links embedded in a value for intrusive list membership.
///
/// Store as raw type-erased pointers to keep size minimal (2 * usize).
/// Use Cell for interior mutability since the queue needs to update links.
pub struct Links {
    prev: Cell<Option<NonNull<()>>>,
    next: Cell<Option<NonNull<()>>>,
}

impl Links {
    pub const fn new() -> Self {
        Self {
            prev: Cell::new(None),
            next: Cell::new(None),
        }
    }

    /// Returns true if this entry is currently linked in a list
    #[inline(always)]
    pub fn is_linked(&self) -> bool {
        self.prev.get().is_some() || self.next.get().is_some()
    }

    #[inline(always)]
    fn assert_unlinked(&self) {
        if cfg!(debug_assertions) {
            debug_assert!(self.prev.get().is_none());
            debug_assert!(self.next.get().is_none());
        }
    }
}

impl Default for Links {
    fn default() -> Self {
        Self::new()
    }
}

/// Adapter trait that tells a Queue how to manipulate links within a value.
///
/// Allows intrusive queues to work with different pointer types (Box, Rc, Arc)
/// and enables multiple list memberships per value (via different link fields).
pub trait Adapter {
    /// The link container type that holds both links and the target value (e.g., Inner<T>)
    type Value;

    /// The user-facing value type for iteration (e.g., T for Entry<T>)
    type Target: ?Sized;

    /// The pointer type that owns the value (e.g., Box<T>, Rc<T>, Arc<T>)
    type Pointer;

    /// Get pointer to Links field from pointer to Value.
    ///
    /// For multiple list memberships, different adapters return different Links fields.
    ///
    /// # Safety
    /// The pointer must be valid and point to an initialized Value.
    unsafe fn links(value: *mut Self::Value) -> *mut Links;

    /// Get pointer to Target from pointer to Value.
    ///
    /// # Safety
    /// The pointer must be valid and point to an initialized Value.
    unsafe fn target(value: *mut Self::Value) -> *mut Self::Target;

    /// Convert Pointer to raw pointer (borrow, doesn't consume).
    fn as_ptr(ptr: &Self::Pointer) -> *const Self::Value;

    /// Leak Pointer into raw pointer (for push - transfers ownership to queue).
    fn into_raw(ptr: Self::Pointer) -> *mut Self::Value;

    /// Reconstruct Pointer from raw pointer (for pop - takes ownership from queue).
    ///
    /// # Safety
    /// The pointer must have been created by `into_raw` and not yet reconstructed.
    unsafe fn from_raw(ptr: *mut Self::Value) -> Self::Pointer;
}

// ── Entry (Box-based intrusive node) ───────────────────────────────────────

/// An entry in the intrusive queue
///
/// Contains the value and links to the previous and next entries.
pub struct Entry<T>(Box<Inner<T>>);

pub struct Inner<T> {
    value: T,
    links: Links,
}

impl<T> ops::Deref for Inner<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

impl<T> ops::DerefMut for Inner<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.value
    }
}

unsafe impl<T: Send> Send for Entry<T> {}
unsafe impl<T: Sync> Sync for Entry<T> {}

impl<T: fmt::Debug> fmt::Debug for Entry<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.value.fmt(f)
    }
}

impl<T: Clone> Clone for Entry<T> {
    fn clone(&self) -> Self {
        self.0.links.assert_unlinked();
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
            links: Links::new(),
        };
        Self(Box::new(inner))
    }

    /// Consume the entry and return the value
    pub fn into_inner(self) -> T {
        let inner = self.0;
        inner.links.assert_unlinked();
        inner.value
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

/// Default adapter for Entry<T> using the embedded links field
pub struct EntryAdapter<T>(core::marker::PhantomData<T>);

impl<T> Adapter for EntryAdapter<T> {
    type Value = Inner<T>;
    type Target = T;
    type Pointer = Entry<T>;

    unsafe fn links(value: *mut Self::Value) -> *mut Links {
        core::ptr::addr_of_mut!((*value).links)
    }

    unsafe fn target(value: *mut Self::Value) -> *mut Self::Target {
        core::ptr::addr_of_mut!((*value).value)
    }

    fn as_ptr(ptr: &Self::Pointer) -> *const Self::Value {
        &*ptr.0
    }

    fn into_raw(ptr: Self::Pointer) -> *mut Self::Value {
        Box::into_raw(ptr.0)
    }

    unsafe fn from_raw(ptr: *mut Self::Value) -> Self::Pointer {
        Entry(Box::from_raw(ptr))
    }
}

/// Macro to define an Rc adapter for a type with an embedded Links field
///
/// Usage:
/// ```ignore
/// struct MyType {
///     pto_links: Links,
///     // other fields...
/// }
///
/// // For RefCell wrapper:
/// rc_adapter!(struct MyAdapter { pto_links: RefCell<MyType> });
///
/// // For direct access:
/// rc_adapter!(struct MyAdapter { pto_links: MyType });
/// ```
#[macro_export]
macro_rules! rc_adapter {
    // Pattern for RefCell<T>
    ($vis:vis struct $adapter:ident { $field:ident : RefCell < $inner:ty > $(,)? }) => {
        #[derive(Clone, Copy, Debug)]
        $vis struct $adapter;

        impl $crate::intrusive::Adapter for $adapter {
            type Value = std::cell::RefCell<$inner>;
            type Target = std::cell::RefCell<$inner>;
            type Pointer = std::rc::Rc<std::cell::RefCell<$inner>>;

            unsafe fn links(value: *mut Self::Value) -> *mut $crate::intrusive::Links {
                core::ptr::addr_of_mut!((*(*value).as_ptr()).$field)
            }

            unsafe fn target(value: *mut Self::Value) -> *mut Self::Target {
                value
            }

            fn as_ptr(ptr: &Self::Pointer) -> *const Self::Value {
                std::rc::Rc::as_ptr(ptr)
            }

            fn into_raw(ptr: Self::Pointer) -> *mut Self::Value {
                std::rc::Rc::into_raw(ptr) as *mut Self::Value
            }

            unsafe fn from_raw(ptr: *mut Self::Value) -> Self::Pointer {
                std::rc::Rc::from_raw(ptr)
            }
        }
    };
    // Pattern for plain types
    ($vis:vis struct $adapter:ident { $field:ident : $inner:ty $(,)? }) => {
        #[derive(Clone, Copy, Debug)]
        $vis struct $adapter;

        impl $crate::intrusive::Adapter for $adapter {
            type Value = $inner;
            type Target = $inner;
            type Pointer = std::rc::Rc<$inner>;

            unsafe fn links(value: *mut Self::Value) -> *mut $crate::intrusive::Links {
                core::ptr::addr_of_mut!((*value).$field)
            }

            unsafe fn target(value: *mut Self::Value) -> *mut Self::Target {
                value
            }

            fn as_ptr(ptr: &Self::Pointer) -> *const Self::Value {
                std::rc::Rc::as_ptr(ptr)
            }

            fn into_raw(ptr: Self::Pointer) -> *mut Self::Value {
                std::rc::Rc::into_raw(ptr) as *mut Self::Value
            }

            unsafe fn from_raw(ptr: *mut Self::Value) -> Self::Pointer {
                std::rc::Rc::from_raw(ptr)
            }
        }
    };
}

// ── Generic Adapter-Based List ────────────────────────────────────────────

/// Generic intrusive FIFO list parameterized by an adapter.
///
/// This is a doubly-linked list where elements are pushed to the back
/// and popped from the front. The list works with any pointer type through
/// the adapter trait.
pub struct List<A: Adapter> {
    head: Option<NonNull<A::Value>>,
    tail: Option<NonNull<A::Value>>,
    len: usize,
    _phantom: core::marker::PhantomData<A>,
}

unsafe impl<A: Adapter> Send for List<A> where A::Pointer: Send {}
unsafe impl<A: Adapter> Sync for List<A> where A::Pointer: Sync {}

impl<A: Adapter> List<A> {
    /// Create a new empty list
    pub const fn new() -> Self {
        Self {
            head: None,
            tail: None,
            len: 0,
            _phantom: core::marker::PhantomData,
        }
    }

    /// Returns true if the list is empty
    pub fn is_empty(&self) -> bool {
        self.head.is_none()
    }

    /// Returns the number of entries in the list
    pub fn len(&self) -> usize {
        self.len
    }

    /// Push an entry to the back of the list
    pub fn push_back(&mut self, ptr: A::Pointer) {
        let raw = A::into_raw(ptr);
        let new_tail = unsafe { NonNull::new_unchecked(raw as *mut A::Value) };
        let self_ref = unsafe { NonNull::new_unchecked(new_tail.as_ptr() as *mut ()) };

        unsafe {
            let links = A::links(new_tail.as_ptr());
            (*links).assert_unlinked();

            if let Some(tail) = self.tail {
                // Non-empty list: link after current tail
                (*links)
                    .prev
                    .set(Some(NonNull::new_unchecked(tail.as_ptr() as *mut ())));
                (*links).next.set(Some(self_ref));

                let tail_links = A::links(tail.as_ptr());
                (*tail_links)
                    .next
                    .set(Some(NonNull::new_unchecked(new_tail.as_ptr() as *mut ())));
            } else {
                // Empty list: singleton gets self-references
                (*links).prev.set(Some(self_ref));
                (*links).next.set(Some(self_ref));
                self.head = Some(new_tail);
            }
        }

        self.tail = Some(new_tail);
        self.len += 1;
    }

    /// Push an entry to the front of the list
    pub fn push_front(&mut self, ptr: A::Pointer) {
        let raw = A::into_raw(ptr);
        let new_head = unsafe { NonNull::new_unchecked(raw as *mut A::Value) };
        let self_ref = unsafe { NonNull::new_unchecked(new_head.as_ptr() as *mut ()) };

        unsafe {
            let links = A::links(new_head.as_ptr());
            (*links).assert_unlinked();

            if let Some(head) = self.head {
                // Non-empty list: link before current head
                (*links).prev.set(Some(self_ref));
                (*links)
                    .next
                    .set(Some(NonNull::new_unchecked(head.as_ptr() as *mut ())));

                let head_links = A::links(head.as_ptr());
                (*head_links)
                    .prev
                    .set(Some(NonNull::new_unchecked(new_head.as_ptr() as *mut ())));
            } else {
                // Empty list: singleton gets self-references
                (*links).prev.set(Some(self_ref));
                (*links).next.set(Some(self_ref));
                self.tail = Some(new_head);
            }
        }

        self.head = Some(new_head);
        self.len += 1;
    }

    /// Pop an entry from the front of the list
    pub fn pop_front(&mut self) -> Option<A::Pointer> {
        let head = self.head.take()?;

        unsafe {
            let links = A::links(head.as_ptr());
            let next = (*links)
                .next
                .get()
                .map(|p| NonNull::new_unchecked(p.as_ptr() as *mut A::Value))
                .filter(|&p| p != head);

            self.head = next;

            if let Some(new_head) = self.head {
                let new_head_links = A::links(new_head.as_ptr());
                (*new_head_links)
                    .prev
                    .set(Some(NonNull::new_unchecked(new_head.as_ptr() as *mut ())));
            } else {
                self.tail = None;
            }

            (*links).prev.set(None);
            (*links).next.set(None);

            self.len -= 1;

            Some(A::from_raw(head.as_ptr()))
        }
    }

    /// Pop an entry from the back of the list
    pub fn pop_back(&mut self) -> Option<A::Pointer> {
        let tail = self.tail.take()?;

        unsafe {
            let links = A::links(tail.as_ptr());
            let prev = (*links)
                .prev
                .get()
                .map(|p| NonNull::new_unchecked(p.as_ptr() as *mut A::Value))
                .filter(|&p| p != tail);

            self.tail = prev;

            if let Some(new_tail) = self.tail {
                let new_tail_links = A::links(new_tail.as_ptr());
                (*new_tail_links)
                    .next
                    .set(Some(NonNull::new_unchecked(new_tail.as_ptr() as *mut ())));
            } else {
                self.head = None;
            }

            (*links).prev.set(None);
            (*links).next.set(None);

            self.len -= 1;

            Some(A::from_raw(tail.as_ptr()))
        }
    }

    /// Append another list to the back of this list
    pub fn append(&mut self, other: &mut List<A>) {
        let Some(other_head) = other.head.take() else {
            return;
        };
        let other_tail = other.tail.take().unwrap();
        let other_len = other.len;
        other.len = 0;

        unsafe {
            if let Some(tail) = self.tail {
                // self.tail.next: was self-ref → now points to other_head
                let tail_links = A::links(tail.as_ptr());
                (*tail_links)
                    .next
                    .set(Some(NonNull::new_unchecked(other_head.as_ptr() as *mut ())));

                // other_head.prev: was self-ref → now points to self.tail
                let other_head_links = A::links(other_head.as_ptr());
                (*other_head_links)
                    .prev
                    .set(Some(NonNull::new_unchecked(tail.as_ptr() as *mut ())));

                // other_tail.next remains self-ref (still the tail) ✓
                // self.head.prev remains self-ref (still the head) ✓
                self.tail = Some(other_tail);
            } else {
                // self was empty, just adopt other's structure as-is
                self.head = Some(other_head);
                self.tail = Some(other_tail);
            }
        }

        self.len += other_len;
    }

    /// Prepend another list to the front of this list
    pub fn prepend(&mut self, other: &mut List<A>) {
        other.append(self);
        core::mem::swap(self, other);
    }

    /// Peek at the front entry without removing it
    /// Peek at the first entry without removing it
    pub fn front(&self) -> Option<&A::Target> {
        self.head.map(|head| unsafe { &*A::target(head.as_ptr()) })
    }

    /// Peek at the last entry without removing it
    pub fn back(&self) -> Option<&A::Target> {
        self.tail.map(|tail| unsafe { &*A::target(tail.as_ptr()) })
    }

    /// Peek at the front entry without removing it (alias)
    pub fn peek_front(&self) -> Option<&A::Target> {
        self.front()
    }

    /// Peek at the back entry without removing it (alias)
    pub fn peek_back(&self) -> Option<&A::Target> {
        self.back()
    }

    /// Peek at the front entry mutably without removing it
    pub fn peek_front_mut(&mut self) -> Option<&mut A::Target> {
        self.head
            .map(|head| unsafe { &mut *A::target(head.as_ptr()) })
    }

    /// Peek at the back entry mutably without removing it
    pub fn peek_back_mut(&mut self) -> Option<&mut A::Target> {
        self.tail
            .map(|tail| unsafe { &mut *A::target(tail.as_ptr()) })
    }

    /// Peek at the front entry mutably without removing it (alias)
    pub fn front_mut(&mut self) -> Option<&mut A::Target> {
        self.peek_front_mut()
    }

    /// Peek at the back entry mutably without removing it (alias)
    pub fn back_mut(&mut self) -> Option<&mut A::Target> {
        self.peek_back_mut()
    }

    /// Iterate over references to values in the list
    pub fn iter(&self) -> Iter<'_, A> {
        Iter {
            next: self.head,
            len: self.len,
            _phantom: core::marker::PhantomData,
        }
    }

    /// Iterate over mutable references to values in the list
    pub fn iter_mut(&mut self) -> IterMut<'_, A> {
        IterMut {
            next: self.head,
            len: self.len,
            _phantom: core::marker::PhantomData,
        }
    }

    /// Drain all entries from the list
    pub fn drain(&mut self) -> IntoIter<A> {
        IntoIter {
            list: std::mem::take(self),
        }
    }
}

impl<A: Adapter> Default for List<A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<A: Adapter> fmt::Debug for List<A>
where
    A::Target: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

impl<A: Adapter> Drop for List<A> {
    fn drop(&mut self) {
        while self.pop_front().is_some() {}
    }
}

impl<A: Adapter> IntoIterator for List<A> {
    type Item = A::Pointer;
    type IntoIter = IntoIter<A>;

    fn into_iter(self) -> Self::IntoIter {
        IntoIter { list: self }
    }
}

pub struct Iter<'a, A: Adapter> {
    next: Option<NonNull<A::Value>>,
    len: usize,
    _phantom: core::marker::PhantomData<&'a A>,
}

impl<'a, A: Adapter> Iterator for Iter<'a, A> {
    type Item = &'a A::Target;

    fn next(&mut self) -> Option<Self::Item> {
        let current = self.next.take()?;
        unsafe {
            let value = &*A::target(current.as_ptr());
            let links = A::links(current.as_ptr());
            self.next = (*links)
                .next
                .get()
                .map(|p| NonNull::new_unchecked(p.as_ptr() as *mut A::Value))
                .filter(|&p| p != current);
            self.len -= 1;
            Some(value)
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.len;
        (len, Some(len))
    }
}

pub struct IterMut<'a, A: Adapter> {
    next: Option<NonNull<A::Value>>,
    len: usize,
    _phantom: core::marker::PhantomData<&'a mut A>,
}

impl<'a, A: Adapter> Iterator for IterMut<'a, A> {
    type Item = &'a mut A::Target;

    fn next(&mut self) -> Option<Self::Item> {
        let current = self.next.take()?;
        unsafe {
            let value = &mut *A::target(current.as_ptr());
            let links = A::links(current.as_ptr());
            self.next = (*links)
                .next
                .get()
                .map(|p| NonNull::new_unchecked(p.as_ptr() as *mut A::Value))
                .filter(|&p| p != current);
            self.len -= 1;
            Some(value)
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.len;
        (len, Some(len))
    }
}

pub struct IntoIter<A: Adapter> {
    list: List<A>,
}

impl<A: Adapter> Iterator for IntoIter<A> {
    type Item = A::Pointer;

    fn next(&mut self) -> Option<Self::Item> {
        self.list.pop_front()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.list.len();
        (len, Some(len))
    }
}

pub type Queue<T> = List<EntryAdapter<T>>;

impl<T> crate::socket::channel::UnboundedSender<Entry<T>> for Queue<T> {
    fn send(&mut self, value: Entry<T>) -> Result<(), Entry<T>> {
        self.push_back(value);
        Ok(())
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
                        let mut temp = std::mem::take(&mut oracle_other);
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

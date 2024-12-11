// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

/// List which manages the status of a slice of entries
///
/// This implementation avoids allocation or shuffling by storing list links
/// inline with the entries.
///
/// # Time complexity
///
/// | [push]  | [pop]   | [remove] |
/// |---------|---------|----------|
/// | *O*(1)  | *O*(1)  | *O*(1)   |
#[derive(Debug)]
pub struct List {
    head: usize,
    tail: usize,
    len: usize,
    /// Tracks if a node is linked or not but only when debug assertions are enabled
    #[cfg(debug_assertions)]
    linked: Vec<bool>,
}

impl Default for List {
    #[inline]
    fn default() -> Self {
        Self {
            head: usize::MAX,
            tail: usize::MAX,
            len: 0,
            #[cfg(debug_assertions)]
            linked: vec![],
        }
    }
}

impl List {
    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    #[inline]
    pub fn pop<L>(&mut self, entries: &mut [L]) -> Option<usize>
    where
        L: AsMut<Link>,
    {
        if self.len == 0 {
            return None;
        }

        let idx = self.head;
        let link = entries[idx].as_mut();
        self.head = link.next;
        link.reset();

        if self.head == usize::MAX {
            self.tail = usize::MAX;
        } else {
            entries[self.head].as_mut().prev = usize::MAX;
        }

        self.set_linked_status(idx, false);

        Some(idx)
    }

    #[inline]
    pub fn front(&self) -> Option<usize> {
        if self.head == usize::MAX {
            None
        } else {
            Some(self.head)
        }
    }

    #[inline]
    pub fn push<L>(&mut self, entries: &mut [L], idx: usize)
    where
        L: AsMut<Link>,
    {
        debug_assert!(idx < usize::MAX);

        let tail = self.tail;
        if tail != usize::MAX {
            entries[tail].as_mut().next = idx;
        } else {
            debug_assert!(self.is_empty());
            self.head = idx;
        }
        self.tail = idx;

        let link = entries[idx].as_mut();
        link.prev = tail;
        link.next = usize::MAX;

        self.set_linked_status(idx, true);
    }

    #[inline]
    pub fn remove<L>(&mut self, entries: &mut [L], idx: usize)
    where
        L: AsMut<Link>,
    {
        debug_assert!(!self.is_empty());
        debug_assert!(idx < usize::MAX);

        let link = entries[idx].as_mut();
        let next = link.next;
        let prev = link.prev;
        link.reset();

        if prev != usize::MAX {
            entries[prev].as_mut().next = next;
        } else {
            debug_assert!(self.head == idx);
            self.head = next;
        }

        if next != usize::MAX {
            entries[next].as_mut().prev = prev;
        } else {
            debug_assert!(self.tail == idx);
            self.tail = prev;
        }

        self.set_linked_status(idx, false);
    }

    #[inline]
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    pub fn iter<'a, L>(&'a self, entries: &'a [L]) -> impl Iterator<Item = usize> + 'a
    where
        L: AsRef<Link>,
    {
        let mut idx = self.head;
        core::iter::from_fn(move || {
            if idx == usize::MAX {
                return None;
            }
            let res = idx;
            idx = entries[idx].as_ref().next;
            Some(res)
        })
    }

    #[inline(always)]
    fn set_linked_status(&mut self, idx: usize, linked: bool) {
        if linked {
            self.len += 1;
        } else {
            self.len -= 1;
        }

        #[cfg(debug_assertions)]
        {
            if self.linked.len() <= idx {
                self.linked.resize(idx + 1, false);
            }
            assert_eq!(self.linked[idx], !linked, "{self:?}");
            self.linked[idx] = linked;
            let expected_len = self.linked.iter().filter(|&v| *v).count();
            assert_eq!(expected_len, self.len, "{self:?}");
        }

        let _ = idx;

        debug_assert_eq!(self.head == usize::MAX, self.is_empty(), "{self:?}");
        debug_assert_eq!(self.tail == usize::MAX, self.is_empty(), "{self:?}");
        debug_assert_eq!(self.head == usize::MAX, self.tail == usize::MAX, "{self:?}");
    }
}

#[derive(Debug)]
pub struct Link {
    next: usize,
    prev: usize,
}

impl Default for Link {
    #[inline]
    fn default() -> Self {
        Self {
            next: usize::MAX,
            prev: usize::MAX,
        }
    }
}

impl Link {
    #[inline]
    fn reset(&mut self) {
        self.next = usize::MAX;
        self.prev = usize::MAX;
    }
}

impl AsRef<Link> for Link {
    #[inline]
    fn as_ref(&self) -> &Link {
        self
    }
}

impl AsMut<Link> for Link {
    #[inline]
    fn as_mut(&mut self) -> &mut Link {
        self
    }
}

#[cfg(test)]
mod tests {
    use bolero::{check, TypeGenerator};

    use super::*;
    use std::collections::VecDeque;

    const LEN: usize = 4;

    enum Location {
        A,
        B,
    }

    #[derive(Default)]
    struct CheckedList {
        list: List,
        oracle: VecDeque<usize>,
    }

    impl CheckedList {
        #[inline]
        fn pop(&mut self, entries: &mut [Link]) -> Option<usize> {
            let v = self.list.pop(entries);
            assert_eq!(v, self.oracle.pop_front());
            self.invariants(entries);
            v
        }

        #[inline]
        fn push(&mut self, entries: &mut [Link], v: usize) {
            self.list.push(entries, v);
            self.oracle.push_back(v);
            self.invariants(entries);
        }

        #[inline]
        fn remove(&mut self, entries: &mut [Link], v: usize) {
            self.list.remove(entries, v);
            let idx = self.oracle.iter().position(|&x| x == v).unwrap();
            self.oracle.remove(idx);
            self.invariants(entries);
        }

        #[inline]
        fn invariants(&self, entries: &[Link]) {
            let actual = self.list.iter(entries);
            assert!(actual.eq(self.oracle.iter().copied()));
        }
    }

    struct Harness {
        a: CheckedList,
        b: CheckedList,
        locations: Vec<Location>,
        entries: Vec<Link>,
    }

    impl Default for Harness {
        fn default() -> Self {
            let mut a = CheckedList::default();
            let mut entries: Vec<Link> = (0..LEN).map(|_| Link::default()).collect();
            let locations = (0..LEN).map(|_| Location::A).collect();

            for idx in 0..LEN {
                a.push(&mut entries, idx);
            }

            Self {
                a,
                b: Default::default(),
                locations,
                entries,
            }
        }
    }

    impl Harness {
        #[inline]
        fn transfer(&mut self, idx: usize) {
            let location = &mut self.locations[idx];
            match location {
                Location::A => {
                    self.a.remove(&mut self.entries, idx);
                    self.b.push(&mut self.entries, idx);
                    *location = Location::B;
                }
                Location::B => {
                    self.b.remove(&mut self.entries, idx);
                    self.a.push(&mut self.entries, idx);
                    *location = Location::A;
                }
            }
        }

        #[inline]
        fn pop_a(&mut self) {
            if let Some(v) = self.a.pop(&mut self.entries) {
                self.b.push(&mut self.entries, v);
                self.locations[v] = Location::B;
            }
        }

        #[inline]
        fn pop_b(&mut self) {
            if let Some(v) = self.b.pop(&mut self.entries) {
                self.a.push(&mut self.entries, v);
                self.locations[v] = Location::A;
            }
        }
    }

    #[derive(Clone, Copy, Debug, TypeGenerator)]
    enum Op {
        Transfer(#[generator(0..LEN)] usize),
        PopA,
        PopB,
    }

    #[test]
    fn invariants_test() {
        check!().with_type::<Vec<Op>>().for_each(|ops| {
            let mut harness = Harness::default();
            for op in ops {
                match op {
                    Op::Transfer(idx) => harness.transfer(*idx),
                    Op::PopA => harness.pop_a(),
                    Op::PopB => harness.pop_b(),
                }
            }
        })
    }
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::fmt;

const SLOT_SIZE: usize = core::mem::size_of::<usize>();

#[derive(Clone, Default)]
pub struct BitSet {
    values: Vec<usize>,
}

impl fmt::Debug for BitSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_set().entries(self.iter()).finish()
    }
}

impl BitSet {
    #[inline]
    #[allow(dead_code)]
    pub fn insert(&mut self, id: usize) {
        self.resize_for_id(id);
        unsafe { self.insert_unchecked(id) }
    }

    #[inline]
    #[allow(dead_code)]
    pub unsafe fn insert_unchecked(&mut self, id: usize) {
        let (index, mask) = Self::index_mask(id);
        s2n_quic_core::assume!(index < self.values.len(), "Index out of bounds");
        let value = &mut self.values[index];
        *value |= mask;
    }

    #[inline]
    #[allow(dead_code)]
    pub fn remove(&mut self, id: usize) -> bool {
        let (index, mask) = Self::index_mask(id);
        if let Some(value) = self.values.get_mut(index) {
            let was_set = (*value & mask) > 0;
            *value &= !mask;
            was_set
        } else {
            false
        }
    }

    #[inline]
    pub fn resize_for_id(&mut self, id: usize) {
        let (index, _mask) = Self::index_mask(id);
        if index >= self.values.len() {
            self.values.resize(index + 1, 0);
        }
    }

    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = usize> + '_ {
        Iter {
            slots: &self.values[..],
            index: 0,
            shift: 0,
        }
    }

    #[inline]
    pub fn drain(&mut self) -> impl Iterator<Item = usize> + '_ {
        Iter {
            slots: &mut self.values[..],
            index: 0,
            shift: 0,
        }
    }

    #[inline(always)]
    fn index_mask(id: usize) -> (usize, usize) {
        let index = id / SLOT_SIZE;
        let mask = 1 << (id % SLOT_SIZE);
        (index, mask)
    }
}

struct Iter<S: Slots> {
    slots: S,
    index: usize,
    shift: usize,
}

impl<S: Slots> Iterator for Iter<S> {
    type Item = usize;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let slot = self.slots.at_index(self.index)?;

            // if the slot is empty then keep going
            if slot == 0 {
                self.index += 1;
                continue;
            }

            while self.shift < SLOT_SIZE {
                let shift = self.shift;
                let id = self.index * SLOT_SIZE + shift;
                let mask = 1 << shift;
                self.shift += 1;
                if slot & mask != 0 {
                    return Some(id);
                }
            }

            self.slots.on_next(self.index);
            self.index += 1;
            self.shift = 0;
        }
    }
}

impl<S: Slots> Drop for Iter<S> {
    #[inline]
    fn drop(&mut self) {
        self.slots.finish(self.index);
    }
}

trait Slots {
    fn at_index(&self, index: usize) -> Option<usize>;
    fn on_next(&mut self, index: usize);
    fn finish(&mut self, index: usize);
}

impl Slots for &[usize] {
    #[inline]
    fn at_index(&self, index: usize) -> Option<usize> {
        self.get(index).cloned()
    }

    #[inline]
    fn on_next(&mut self, _index: usize) {}

    #[inline]
    fn finish(&mut self, _index: usize) {}
}

impl Slots for &mut [usize] {
    #[inline]
    fn at_index(&self, index: usize) -> Option<usize> {
        self.get(index).cloned()
    }

    #[inline]
    fn on_next(&mut self, index: usize) {
        self[index] = 0;
    }

    #[inline]
    fn finish(&mut self, index: usize) {
        // clear out any remaining slots in `Drain`
        unsafe { self.get_unchecked_mut(index..).fill(0) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bolero::TypeGenerator;
    use std::collections::BTreeSet;

    #[derive(Clone, Copy, Debug, TypeGenerator)]
    enum Op {
        Insert(u8),
        Remove(u8),
    }

    #[test]
    fn bit_set_test() {
        bolero::check!().with_type::<Vec<Op>>().for_each(|ops| {
            let mut subject = BitSet::default();
            let mut oracle = BTreeSet::default();

            for op in ops {
                match *op {
                    Op::Insert(id) => {
                        subject.insert(id as usize);
                        oracle.insert(id as usize);
                    }
                    Op::Remove(id) => {
                        let a = subject.remove(id as usize);
                        let b = oracle.remove(&(id as usize));
                        assert_eq!(a, b);
                    }
                }

                assert!(
                    subject.iter().eq(oracle.iter().cloned()),
                    "oracle: {oracle:?}\nsubject: {subject:?}"
                );
            }
        });
    }
}

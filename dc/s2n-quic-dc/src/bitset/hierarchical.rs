// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::fmt;
use std::ops;

use super::BitSet64;

/// A hierarchical bitset with 4 layers for O(4) search operations.
///
/// Structure:
/// - Layer 3 (top): 1 x BitSet64 = 64 summary bits
/// - Layer 2: 64 x BitSet64 = 4,096 summary bits
/// - Layer 1: 4,096 x BitSet64 = 262,144 summary bits
/// - Layer 0 (data): 262,144 x BitSet64 = 16,777,216 data bits
pub struct HierarchicalBitSet {
    layer_3: BitSet64,
    layer_2: Layer,
    layer_1: Layer,
    layer_0: Layer,
    capacity: u32,
    len: u32,
}

impl fmt::Debug for HierarchicalBitSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HierarchicalBitSet")
            .field("capacity", &self.capacity)
            .field("len", &self.len)
            .finish_non_exhaustive()
    }
}

impl HierarchicalBitSet {
    pub const MAX_CAPACITY: u32 = 16_777_216;

    pub fn new(capacity: u32) -> Self {
        let mut bitset = Self {
            capacity,
            len: 0,
            layer_3: BitSet64::new(),
            layer_2: Default::default(),
            layer_1: Default::default(),
            layer_0: Default::default(),
        };
        bitset.init();
        bitset
    }

    fn init(&mut self) {
        let capacity = self.capacity.max(1);
        assert!(capacity <= Self::MAX_CAPACITY);
        self.capacity = capacity;

        let path = Self::compute_indices(capacity - 1);
        self.layer_0.initialize(path.layer_0, false);
        self.layer_1.initialize(path.layer_1, false);
        self.layer_2.initialize(path.layer_2, false);
    }

    #[inline(always)]
    fn compute_indices(index: u32) -> LayerPath {
        LayerPath::new(index)
    }

    pub fn is_empty(&self) -> bool {
        self.layer_3.is_empty()
    }

    pub fn capacity(&self) -> u32 {
        self.capacity
    }

    pub fn len(&self) -> u32 {
        self.len
    }

    pub fn contains(&self, index: u32) -> bool {
        if index >= self.capacity {
            return false;
        }
        let path = Self::compute_indices(index);
        self.layer_0[path.layer_0].get(path.layer_0.bit)
    }

    pub fn insert(&mut self, index: u32) -> bool {
        debug_assert!(index < self.capacity);
        if index >= self.capacity {
            return false;
        }

        let path = Self::compute_indices(index);

        let newly_inserted = self.layer_0[path.layer_0].insert(path.layer_0.bit);
        self.layer_1[path.layer_1].insert(path.layer_1.bit);
        self.layer_2[path.layer_2].insert(path.layer_2.bit);
        self.layer_3.insert(path.layer_3());

        if newly_inserted {
            self.len += 1;
        }
        newly_inserted
    }

    /// Insert a contiguous range of indices [start, end] inclusive.
    ///
    /// Currently O(range) — each element is inserted individually, which
    /// updates all four hierarchy layers.  A future optimisation could set
    /// whole `BitSet64` blocks in layer_0 at once (O(range/64)), but the
    /// simpler loop is easier to audit for correctness.
    pub fn insert_range(&mut self, start: u32, end: u32) {
        if start > end || start >= self.capacity {
            return;
        }
        let end = end.min(self.capacity - 1);
        // For small ranges, just loop (avoids complexity overhead)
        if end - start < 64 {
            for i in start..=end {
                self.insert(i);
            }
            return;
        }
        // For larger ranges, insert per-element (could optimize full-block sets later)
        for i in start..=end {
            self.insert(i);
        }
    }

    pub fn remove(&mut self, index: u32) -> bool {
        if index >= self.capacity {
            return false;
        }

        let path = Self::compute_indices(index);

        let (was_present, cascade) = self.layer_0.remove(path.layer_0);

        if cascade
            && self.layer_1.remove_cascade(path.layer_1)
            && self.layer_2.remove_cascade(path.layer_2)
        {
            self.layer_3.remove(path.layer_3());
        }

        if was_present {
            self.len -= 1;
        }
        was_present
    }

    pub fn clear(&mut self) {
        self.len = 0;
        self.layer_0.clear();
        self.layer_1.clear();
        self.layer_2.clear();
        self.layer_3.clear();
    }

    pub fn first(&self) -> Option<u32> {
        let l3_bit = self.layer_3.first()?;
        let l3_index = LayerIndex {
            block: 0,
            bit: l3_bit,
        };

        let l2_index = self.layer_2.first(l3_index);
        let l1_index = self.layer_1.first(l2_index);
        let l0_index = self.layer_0.first(l1_index);
        Some(l0_index.expand() as u32)
    }

    pub fn pop_first(&mut self) -> Option<u32> {
        let l3_bit = self.layer_3.first()?;
        let l3_index = LayerIndex {
            block: 0,
            bit: l3_bit,
        };

        let l2_index = self.layer_2.first(l3_index);
        let l1_index = self.layer_1.first(l2_index);
        let l0_index = self.layer_0.first(l1_index);
        let value = l0_index.expand() as u32;

        if self.layer_0.remove_cascade(l0_index)
            && self.layer_1.remove_cascade(l1_index)
            && self.layer_2.remove_cascade(l2_index)
        {
            self.layer_3.remove(l3_index.bit);
        }

        self.len -= 1;
        Some(value)
    }

    pub fn drain(&mut self) -> Drain<'_> {
        Drain { bitset: self }
    }

    /// Grow capacity to accommodate at least `new_capacity` indices.
    pub fn grow(&mut self, new_capacity: u32) {
        if new_capacity <= self.capacity {
            return;
        }
        assert!(new_capacity <= Self::MAX_CAPACITY);

        let path = Self::compute_indices(new_capacity - 1);
        self.layer_0.grow(path.layer_0);
        self.layer_1.grow(path.layer_1);
        self.layer_2.grow(path.layer_2);
        self.capacity = new_capacity;
    }
}

pub struct Drain<'a> {
    bitset: &'a mut HierarchicalBitSet,
}

impl Iterator for Drain<'_> {
    type Item = u32;

    fn next(&mut self) -> Option<Self::Item> {
        self.bitset.pop_first()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.bitset.len() as usize;
        (len, Some(len))
    }
}

#[derive(Clone, Copy, Debug)]
struct LayerPath {
    layer_0: LayerIndex,
    layer_1: LayerIndex,
    layer_2: LayerIndex,
}

impl LayerPath {
    #[inline(always)]
    pub fn new(index: u32) -> LayerPath {
        let layer_0 = LayerIndex::new(index, 0);
        let layer_1 = LayerIndex::new(index, 1);
        let layer_2 = LayerIndex::new(index, 2);
        LayerPath {
            layer_0,
            layer_1,
            layer_2,
        }
    }

    #[inline(always)]
    fn layer_3(&self) -> u8 {
        self.layer_2.block as u8
    }
}

#[derive(Clone, Copy, Debug)]
struct LayerIndex {
    block: usize,
    bit: u8,
}

impl LayerIndex {
    #[inline(always)]
    pub const fn new(index: u32, layer: u32) -> Self {
        let shift = layer * 6 + 6;
        let block = (index >> shift) as usize;
        let bit = ((index >> (shift - 6)) % 64) as u8;
        Self { block, bit }
    }

    fn expand(&self) -> usize {
        self.block * 64 + self.bit as usize
    }
}

#[derive(Clone, Default)]
struct Layer {
    blocks: Vec<BitSet64>,
}

impl Layer {
    pub fn initialize(&mut self, index: LayerIndex, enabled: bool) {
        let complete_blocks = index.block + 1;
        let value = if enabled {
            BitSet64::all()
        } else {
            BitSet64::new()
        };

        self.blocks.reserve_exact(complete_blocks);
        self.blocks.resize(complete_blocks, value);
    }

    pub fn grow(&mut self, index: LayerIndex) {
        let needed = index.block + 1;
        if self.blocks.len() < needed {
            self.blocks.resize(needed, BitSet64::new());
        }
    }

    pub fn clear(&mut self) {
        for block in self.blocks.iter_mut() {
            block.clear();
        }
    }

    pub fn remove(&mut self, index: LayerIndex) -> (bool, bool) {
        let block = &mut self.blocks[index.block];
        let was_present = block.remove(index.bit);
        (was_present, was_present && block.is_empty())
    }

    pub fn remove_cascade(&mut self, index: LayerIndex) -> bool {
        let block = &mut self.blocks[index.block];
        block.remove(index.bit);
        block.is_empty()
    }

    #[inline]
    pub fn first(&self, parent: LayerIndex) -> LayerIndex {
        let block_index = parent.expand();
        let block = &self.blocks[block_index];
        let bit = block
            .first()
            .expect("empty block but higher layer indicated otherwise");
        LayerIndex {
            block: block_index,
            bit,
        }
    }
}

impl ops::Index<LayerIndex> for Layer {
    type Output = BitSet64;

    #[inline]
    fn index(&self, index: LayerIndex) -> &Self::Output {
        &self.blocks[index.block]
    }
}

impl ops::IndexMut<LayerIndex> for Layer {
    #[inline]
    fn index_mut(&mut self, index: LayerIndex) -> &mut Self::Output {
        &mut self.blocks[index.block]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_operations() {
        let mut bitset = HierarchicalBitSet::new(1000);
        assert!(bitset.is_empty());
        assert_eq!(bitset.len(), 0);

        assert!(bitset.insert(0));
        assert!(bitset.insert(63));
        assert!(bitset.insert(64));
        assert!(bitset.insert(999));

        assert!(!bitset.is_empty());
        assert_eq!(bitset.len(), 4);
        assert!(bitset.contains(0));
        assert!(bitset.contains(63));
        assert!(bitset.contains(64));
        assert!(bitset.contains(999));
        assert!(!bitset.contains(1));

        assert!(!bitset.insert(0));
        assert_eq!(bitset.len(), 4);

        assert!(bitset.remove(0));
        assert!(!bitset.remove(0));
        assert_eq!(bitset.len(), 3);

        bitset.clear();
        assert!(bitset.is_empty());
    }

    #[test]
    fn pop_first() {
        let mut bitset = HierarchicalBitSet::new(1_000_000);
        assert_eq!(bitset.pop_first(), None);

        bitset.insert(999_999);
        bitset.insert(1000);
        bitset.insert(50);
        bitset.insert(0);

        assert_eq!(bitset.pop_first(), Some(0));
        assert_eq!(bitset.pop_first(), Some(50));
        assert_eq!(bitset.pop_first(), Some(1000));
        assert_eq!(bitset.pop_first(), Some(999_999));
        assert_eq!(bitset.pop_first(), None);
        assert!(bitset.is_empty());
    }

    #[test]
    fn drain() {
        let mut bitset = HierarchicalBitSet::new(10_000);
        let indices = vec![5000, 10, 9999, 0, 500, 64, 63, 4096];
        for &idx in &indices {
            bitset.insert(idx);
        }

        let drained: Vec<u32> = bitset.drain().collect();
        let mut sorted = indices.clone();
        sorted.sort();
        assert_eq!(drained, sorted);
        assert!(bitset.is_empty());
    }

    #[test]
    fn grow() {
        let mut bitset = HierarchicalBitSet::new(100);
        bitset.insert(50);
        bitset.grow(1000);
        assert!(bitset.contains(50));
        assert!(bitset.insert(500));
        assert!(bitset.contains(500));
        assert_eq!(bitset.len(), 2);
    }

    #[test]
    fn layer_boundaries() {
        let mut bitset = HierarchicalBitSet::new(1_000_000);
        let indices = vec![0, 63, 64, 4095, 4096, 262_143, 262_144, 999_999];
        for &idx in &indices {
            assert!(bitset.insert(idx));
            assert!(bitset.contains(idx));
        }
        assert_eq!(bitset.len(), indices.len() as u32);

        for &idx in &indices {
            assert!(bitset.remove(idx));
        }
        assert!(bitset.is_empty());
    }
}

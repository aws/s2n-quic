// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Bitset data structures.
//!
//! `BitSet64` is a compact 64-bit word bitset.
//! `HierarchicalBitSet` layers four levels of `BitSet64` for O(4) insert/remove/pop
//! over up to ~16 million entries.

mod bitset64;
mod hierarchical;

pub use bitset64::BitSet64;
pub use hierarchical::HierarchicalBitSet;

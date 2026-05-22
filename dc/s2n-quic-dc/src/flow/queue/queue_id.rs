// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use s2n_quic_core::varint::VarInt;

/// Queue ID bit layout (62-bit QUIC varint payload).
///
/// We interleave index and generation fields so queue IDs keep a stable slot lookup
/// while preserving generation for stale-handle detection:
///
/// ```text
/// [ index_high | generation_high | generation_low | index_low ]
/// ```
///
/// where:
/// - `index_low` uses `INDEX_LOW_BITS` (=20) LSBs,
/// - `index_high` uses `INDEX_HIGH_BITS` (=5) MSBs,
/// - `generation_low` uses `GENERATION_LOW_BITS` (=10) bits between index halves,
/// - `generation_high` uses `GENERATION_BITS - GENERATION_LOW_BITS` (=27) bits in
///   the remaining middle region.
///
/// Decoding is the inverse composition performed by [`index`] and [`generation`].
pub const INDEX_BITS: u32 = 25;
pub const GENERATION_BITS: u32 = 62 - INDEX_BITS;
pub const GENERATION_MASK: u64 = (1u64 << GENERATION_BITS) - 1;
pub const MAX_SLOTS: usize = 1usize << INDEX_BITS;
const INDEX_LOW_BITS: u32 = 20;
const INDEX_HIGH_BITS: u32 = INDEX_BITS - INDEX_LOW_BITS;
const GENERATION_LOW_BITS: u32 = 10;
const INDEX_LOW_MASK: u64 = (1u64 << INDEX_LOW_BITS) - 1;
const INDEX_HIGH_MASK: u64 = (1u64 << INDEX_HIGH_BITS) - 1;
const GENERATION_LOW_MASK: u64 = (1u64 << GENERATION_LOW_BITS) - 1;
const GENERATION_HIGH_MASK: u64 = (1u64 << (GENERATION_BITS - GENERATION_LOW_BITS)) - 1;
const GENERATION_LOW_SHIFT: u32 = INDEX_LOW_BITS;
const GENERATION_HIGH_SHIFT: u32 = INDEX_LOW_BITS + GENERATION_LOW_BITS;
const INDEX_HIGH_SHIFT: u32 = GENERATION_HIGH_SHIFT + (GENERATION_BITS - GENERATION_LOW_BITS);

#[inline]
pub fn encode(index: usize, generation: u64) -> VarInt {
    debug_assert!(index < MAX_SLOTS);
    let index = index as u64;
    let generation = generation & GENERATION_MASK;
    let index_low = index & INDEX_LOW_MASK;
    let index_high = (index >> INDEX_LOW_BITS) & INDEX_HIGH_MASK;
    let generation_low = generation & GENERATION_LOW_MASK;
    let generation_high = (generation >> GENERATION_LOW_BITS) & GENERATION_HIGH_MASK;
    let value = index_low
        | (generation_low << GENERATION_LOW_SHIFT)
        | (generation_high << GENERATION_HIGH_SHIFT)
        | (index_high << INDEX_HIGH_SHIFT);
    // SAFETY: all packed values are bounded to 62 bits by construction.
    unsafe { VarInt::new_unchecked(value) }
}

#[inline]
pub fn index(queue_id: VarInt) -> usize {
    let value = queue_id.as_u64();
    let index_low = value & INDEX_LOW_MASK;
    let index_high = (value >> INDEX_HIGH_SHIFT) & INDEX_HIGH_MASK;
    ((index_high << INDEX_LOW_BITS) | index_low) as usize
}

#[cfg(test)]
#[inline]
pub fn generation(queue_id: VarInt) -> u64 {
    let value = queue_id.as_u64();
    let generation_low = (value >> GENERATION_LOW_SHIFT) & GENERATION_LOW_MASK;
    let generation_high = (value >> GENERATION_HIGH_SHIFT) & GENERATION_HIGH_MASK;
    (generation_high << GENERATION_LOW_BITS) | generation_low
}

#[cfg(test)]
mod tests {
    use super::*;
    use bolero::check;

    #[test]
    fn bolero_round_trip_index_and_generation() {
        check!()
            .with_type::<(u32, u64)>()
            .for_each(|(raw_index, generation)| {
                let slot = (*raw_index as usize) % MAX_SLOTS;
                let generation = *generation & GENERATION_MASK;
                let queue_id = encode(slot, generation);
                assert_eq!(index(queue_id), slot);
                assert_eq!(super::generation(queue_id), generation);
            });
    }
}

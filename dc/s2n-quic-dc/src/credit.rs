// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

mod config;
mod counters;
mod pool;
mod slot;
mod waker;

pub use config::Config;
pub use counters::Counters;
pub use pool::{Distributor, Pool, Priority};
pub use slot::{AbandonResult, DeadSlot, DeadSlotQueue, GrantResult, Slot};

/// Static-asserts that the embedded `slot: Slot` field lives at offset 0 of `$outer`.
///
/// Required for every `#[repr(C)]` allocation whose pointer is handed to the credit pool: the pool
/// stores only `NonNull<Slot>`, and the per-allocation `drop_fn` recovers the outer type by casting
/// `NonNull<Slot>` back. That cast is sound only when `Slot` is the prefix of the outer struct.
/// A future field reorder that moves `slot` away from offset 0 would silently corrupt the cast and
/// produce undefined behavior at deallocation time; this macro turns that into a compile error.
///
/// Convention: the embedded field MUST be named `slot` so this macro can locate it via
/// `core::mem::offset_of!($outer, slot)`.
///
/// # Example
///
/// ```ignore
/// #[repr(C)]
/// struct WriterAlloc {
///     slot: $crate::credit::Slot,
///     // ... other fields ...
/// }
/// $crate::assert_slot_at_offset_zero!(WriterAlloc);
/// ```
#[macro_export]
macro_rules! assert_slot_at_offset_zero {
    ($outer:ty) => {
        const _: () = assert!(
            ::core::mem::offset_of!($outer, slot) == 0,
            "credit::Slot must be at offset 0 of the outer allocation — \
             the credit pool casts NonNull<Slot> back to the outer type via drop_fn"
        );
    };
}

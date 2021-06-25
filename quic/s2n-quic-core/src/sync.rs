// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

pub mod atomic_waker;

#[cfg(feature = "alloc")]
pub mod ring_lock;

#[cfg(all(feature = "alloc", not(loom)))]
pub use alloc::sync::Arc;
#[cfg(loom)]
pub use loom::sync::Arc;

#[cfg(not(loom))]
pub use core::sync::atomic;
#[cfg(loom)]
pub use loom::sync::atomic;

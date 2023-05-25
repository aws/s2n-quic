// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#[cfg(feature = "alloc")]
mod primitive;

#[cfg(feature = "crossbeam-utils")]
pub use crossbeam_utils::CachePadded;
#[cfg(feature = "atomic-waker")]
pub mod atomic_waker;
#[cfg(target_has_atomic = "32")]
pub mod cursor;
#[cfg(feature = "alloc")]
pub mod spsc;
#[cfg(feature = "alloc")]
pub mod worker;

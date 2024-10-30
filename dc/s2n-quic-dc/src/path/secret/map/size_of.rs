// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::status::IsRetired;
use s2n_quic_core::dc;
use std::{net::SocketAddr, sync::atomic::AtomicU64, time::Instant};

/// Provide an approximation of the size of Self, including any heap indirection (e.g., a vec
/// backed by a megabyte is a megabyte in `size`, not 24 bytes).
///
/// Approximation because we don't currently attempt to account for (as an example) padding. It's
/// too annoying to do that.
#[cfg_attr(not(test), allow(unused))]
pub(crate) trait SizeOf: Sized {
    fn size(&self) -> usize {
        // If we don't need drop, it's very likely that this type is fully contained in size_of
        // Self. This simplifies implementing this trait for e.g. std types.
        assert!(
            !std::mem::needs_drop::<Self>(),
            "{:?} requires custom SizeOf impl",
            std::any::type_name::<Self>()
        );
        std::mem::size_of::<Self>()
    }
}

impl SizeOf for Instant {}
impl SizeOf for u32 {}
impl SizeOf for SocketAddr {}
impl SizeOf for AtomicU64 {}

impl SizeOf for IsRetired {}
impl SizeOf for dc::ApplicationParams {}

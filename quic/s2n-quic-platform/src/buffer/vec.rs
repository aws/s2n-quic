// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::buffer::Buffer;
use core::{
    fmt,
    ops::{Deref, DerefMut},
};
use lazy_static::lazy_static;
use s2n_quic_core::path::DEFAULT_MAX_MTU;

// TODO decide on better defaults
lazy_static! {
    static ref DEFAULT_MESSAGE_COUNT: usize = {
        std::env::var("S2N_UNSTABLE_DEFAULT_MESSAGE_COUNT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1024 * 2)
    };
}

pub struct VecBuffer {
    region: alloc::vec::Vec<u8>,
    mtu: usize,
}

impl VecBuffer {
    /// Create a contiguous buffer with the specified number of messages
    pub fn new(message_count: usize, mtu: usize) -> Self {
        let len = message_count * mtu;
        let region = alloc::vec![0; len];

        Self { region, mtu }
    }
}

impl Default for VecBuffer {
    fn default() -> Self {
        // when testing this crate, make buffers smaller to avoid
        // repeated large allocations
        if cfg!(test) {
            Self::new(64, 1200)
        } else {
            Self::new(*DEFAULT_MESSAGE_COUNT, DEFAULT_MAX_MTU.into())
        }
    }
}

impl fmt::Debug for VecBuffer {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("VecBuffer")
            .field("mtu", &self.mtu())
            .field("message_count", &self.len())
            .finish()
    }
}

impl Buffer for VecBuffer {
    fn len(&self) -> usize {
        self.region.len()
    }

    fn mtu(&self) -> usize {
        self.mtu
    }
}

impl Deref for VecBuffer {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        self.region.as_ref()
    }
}

impl DerefMut for VecBuffer {
    fn deref_mut(&mut self) -> &mut [u8] {
        self.region.as_mut()
    }
}

impl Drop for VecBuffer {
    fn drop(&mut self) {
        // The buffer could contain sensitive data so release it before freeing the memory
        zeroize::Zeroize::zeroize(&mut self.region[..]);
    }
}

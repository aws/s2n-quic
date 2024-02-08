// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::buffer::writer::Storage;

/// Immediately discards any write operations
///
/// This implementation can be used for benchmarking operations outside of copies.
#[derive(Clone, Copy, Debug, Default)]
pub struct Discard;

impl Storage for Discard {
    #[inline]
    fn put_slice(&mut self, bytes: &[u8]) {
        let _ = bytes;
    }

    #[inline]
    fn remaining_capacity(&self) -> usize {
        usize::MAX
    }
}

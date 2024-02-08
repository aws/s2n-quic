// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::Storage;

/// A writer incapable of being written to
#[derive(Clone, Copy, Debug, Default)]
pub struct Empty;

impl Storage for Empty {
    #[inline]
    fn put_slice(&mut self, slice: &[u8]) {
        debug_assert!(
            slice.is_empty(),
            "cannot put a non-empty slice in empty writer storage"
        );
    }

    #[inline]
    fn remaining_capacity(&self) -> usize {
        0
    }

    #[inline]
    fn has_remaining_capacity(&self) -> bool {
        false
    }
}

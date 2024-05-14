// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use alloc::collections::VecDeque;

impl super::Storage for VecDeque<u8> {
    #[inline]
    fn put_slice(&mut self, bytes: &[u8]) {
        self.extend(bytes);
    }

    #[inline]
    fn remaining_capacity(&self) -> usize {
        (isize::MAX as usize) - self.len()
    }
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::encoder::Encoder;

/// Estimates the `encoding_size` of an `EncoderValue`
pub struct EncoderLenEstimator {
    capacity: usize,
    len: usize,
}

impl EncoderLenEstimator {
    /// Create a new estimator with a given buffer `capacity`
    pub const fn new(capacity: usize) -> Self {
        Self { capacity, len: 0 }
    }

    /// Returns true when the estimated len is greater than
    /// the allocated buffer capacity.
    pub const fn overflowed(&self) -> bool {
        self.len > self.capacity
    }
}

impl Encoder for EncoderLenEstimator {
    fn write_sized<F: FnOnce(&mut [u8])>(&mut self, len: usize, _write: F) {
        self.len += len;
    }

    fn write_slice(&mut self, slice: &[u8]) {
        self.len += slice.len();
    }

    fn write_repeated(&mut self, count: usize, _value: u8) {
        self.len += count;
    }

    fn capacity(&self) -> usize {
        self.capacity
    }

    fn len(&self) -> usize {
        self.len
    }
}

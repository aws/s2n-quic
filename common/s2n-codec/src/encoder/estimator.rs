// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::encoder::Encoder;

/// Estimates the `encoding_size` of an `EncoderValue`
#[cfg_attr(test, derive(Clone, Debug, bolero::TypeGenerator))]
pub struct EncoderLenEstimator {
    capacity: usize,
    len: usize,
}

impl EncoderLenEstimator {
    /// Create a new estimator with a given buffer `capacity`
    #[inline]
    pub const fn new(capacity: usize) -> Self {
        Self { capacity, len: 0 }
    }

    /// Returns true when the estimated len is greater than
    /// the allocated buffer capacity.
    #[inline]
    pub const fn overflowed(&self) -> bool {
        self.len > self.capacity
    }
}

impl Encoder for EncoderLenEstimator {
    #[inline]
    fn write_sized<F: FnOnce(&mut [u8])>(&mut self, len: usize, _write: F) {
        self.len += len;
    }

    #[inline]
    fn write_slice(&mut self, slice: &[u8]) {
        self.len += slice.len();
    }

    #[inline]
    fn write_repeated(&mut self, count: usize, _value: u8) {
        self.len += count;
    }

    #[inline]
    fn write_zerocopy<T: zerocopy::FromBytes + zerocopy::Unaligned, F: FnOnce(&mut T)>(
        &mut self,
        _write: F,
    ) {
        self.len += core::mem::size_of::<T>();
    }

    #[inline]
    fn capacity(&self) -> usize {
        self.capacity
    }

    #[inline]
    fn len(&self) -> usize {
        self.len
    }
}

#[cfg(test)]
mod bolero_harnesses {

    use super::*;

    #[test]
    #[cfg_attr(kani, kani::proof)]
    fn bolero_test_27_new() {
        bolero::check!()
            .with_type()
            .cloned()
            .for_each(|capacity: usize| Some(EncoderLenEstimator::new(capacity)));
    }

    #[test]
    #[cfg_attr(kani, kani::proof)]
    fn bolero_test_28_overflowed() {
        bolero::check!()
            .with_type()
            .for_each(|callee: &EncoderLenEstimator| Some(callee.overflowed()));
    }
    
    #[test]
    #[cfg_attr(kani, kani::proof)]
    fn bolero_test_29_write_repeated() {
        bolero::check!().with_type().cloned().for_each(
            |(mut callee, count, value): (EncoderLenEstimator, usize, u8)| {
                Some(callee.write_repeated(count, value))
            },
        );
    }

    #[test]
    #[cfg_attr(kani, kani::proof)]
    fn bolero_test_30_capacity() {
        bolero::check!()
            .with_type()
            .for_each(|callee: &EncoderLenEstimator| Some(callee.capacity()));
    }
    
    #[test]
    #[cfg_attr(kani, kani::proof)]
    fn bolero_test_31_len() {
        bolero::check!()
            .with_type()
            .for_each(|callee: &EncoderLenEstimator| Some(callee.len()));
    }
}

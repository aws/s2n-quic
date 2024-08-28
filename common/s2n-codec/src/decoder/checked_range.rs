// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::fmt;

#[derive(Copy, Clone)]
#[cfg_attr(test, derive(bolero::TypeGenerator))]
pub struct CheckedRange {
    start: usize,
    end: usize,

    #[cfg(all(debug_assertions, feature = "checked_range_unsafe"))]
    original_ptr: *const u8,
}

impl CheckedRange {
    #[inline]
    pub(crate) fn new(start: usize, end: usize, original_ptr: *const u8) -> Self {
        #[cfg(not(all(debug_assertions, feature = "checked_range_unsafe")))]
        let _ = original_ptr;

        Self {
            start,
            end,
            #[cfg(all(debug_assertions, feature = "checked_range_unsafe"))]
            original_ptr,
        }
    }

    #[cfg(feature = "checked_range_unsafe")]
    #[inline]
    pub fn get<'a>(&self, slice: &'a [u8]) -> &'a [u8] {
        unsafe {
            #[cfg(debug_assertions)]
            debug_assert_eq!(slice.as_ptr().add(self.start), self.original_ptr);

            slice.get_unchecked(self.start..self.end)
        }
    }

    #[cfg(not(feature = "checked_range_unsafe"))]
    #[inline]
    pub fn get<'a>(&self, slice: &'a [u8]) -> &'a [u8] {
        &slice[self.start..self.end]
    }

    #[cfg(feature = "checked_range_unsafe")]
    #[inline]
    pub fn get_mut<'a>(&self, slice: &'a mut [u8]) -> &'a mut [u8] {
        unsafe {
            #[cfg(debug_assertions)]
            debug_assert_eq!(slice.as_ptr().add(self.start), self.original_ptr);

            slice.get_unchecked_mut(self.start..self.end)
        }
    }

    #[cfg(not(feature = "checked_range_unsafe"))]
    #[inline]
    pub fn get_mut<'a>(&self, slice: &'a mut [u8]) -> &'a mut [u8] {
        &mut slice[self.start..self.end]
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.end - self.start
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }
}

impl fmt::Debug for CheckedRange {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}..{}", self.start, self.end)
    }
}

#[cfg(test)]
mod bolero_harnesses {
    use super::*;
    #[test]
    #[cfg_attr(kani, kani::proof)]
    fn bolero_test_25_len() {
        bolero::check!()
            .with_type()
            .cloned()
            .for_each(|callee: CheckedRange| Some(callee.len()));
    }
    #[test]
    #[cfg_attr(kani, kani::proof)]
    fn bolero_test_26_is_empty() {
        bolero::check!()
            .with_type()
            .cloned()
            .for_each(|callee: CheckedRange| Some(callee.is_empty()));
    }
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::{
    convert::{TryFrom, TryInto},
    fmt,
    fmt::{Display, Formatter},
    num::NonZeroUsize,
};

#[derive(Clone, Copy, Debug)]
pub struct MaxSegments(NonZeroUsize);

impl Default for MaxSegments {
    fn default() -> Self {
        MaxSegments::MAX
    }
}

impl TryFrom<usize> for MaxSegments {
    type Error = MaxSegmentsError;

    fn try_from(value: usize) -> Result<Self, Self::Error> {
        if !(1..=Self::MAX.into()).contains(&value) {
            return Err(MaxSegmentsError);
        }

        Ok(MaxSegments(value.try_into().expect(
            "Value must be greater than zero according to the check above",
        )))
    }
}

#[derive(Debug)]
pub struct MaxSegmentsError;

impl Display for MaxSegmentsError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "MaxSegments must be a non-zero value less than or equal to {}",
            MaxSegments::MAX.0.get()
        )
    }
}

impl From<MaxSegments> for usize {
    #[inline]
    fn from(value: MaxSegments) -> Self {
        value.0.get()
    }
}

#[cfg(s2n_quic_platform_gso)]
impl MaxSegments {
    // The packet pacer enforces a burst limit of 10 packets, so there is no benefit to having
    // GSO MaxSegments higher than 10.
    //= https://www.rfc-editor.org/rfc/rfc9002.txt#7.7
    //# Senders SHOULD limit bursts to the initial congestion window
    const MAX: Self = MaxSegments(unsafe {
        NonZeroUsize::new_unchecked(s2n_quic_core::recovery::MAX_BURST_PACKETS as usize)
    });
}

#[cfg(not(s2n_quic_platform_gso))]
impl MaxSegments {
    const MAX: Self = MaxSegments(unsafe { NonZeroUsize::new_unchecked(1) });
}

#[derive(Debug)]
pub struct Gso {
    #[allow(dead_code)] // ignore this field on unsupported platforms
    max_segments: MaxSegments,
}

impl Default for Gso {
    fn default() -> Self {
        Self {
            max_segments: MaxSegments::MAX,
        }
    }
}

impl Gso {
    #[inline]
    pub fn max_segments(&self) -> usize {
        self.max_segments.into()
    }

    #[inline]
    pub fn default_max_segments(&self) -> usize {
        self.max_segments.0.min(MaxSegments::default().0).into()
    }
}

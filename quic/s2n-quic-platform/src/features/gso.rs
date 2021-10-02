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
        MaxSegments::DEFAULT
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
    // https://github.com/torvalds/linux/blob/e9f1cbc0c4114880090c7a578117d3b9cf184ad4/tools/testing/selftests/net/udpgso.c#L37
    // ```
    // #define UDP_MAX_SEGMENTS	(1 << 6UL)
    // ```
    const MAX: Self = MaxSegments(unsafe { NonZeroUsize::new_unchecked(1 << 6) });

    // TODO profile a good default
    // We need to strike a good balance of how deep the message buffers go.
    // If they're too deep then we'll waste a lot of space and be swapping pages
    // frequently. 16 seems like a good place to start as that was about the number
    // of packets being sent at a time on a 1GbE test.
    const DEFAULT: MaxSegments = MaxSegments(unsafe { NonZeroUsize::new_unchecked(16) });
}

#[cfg(not(s2n_quic_platform_gso))]
impl MaxSegments {
    const MAX: Self = MaxSegments(unsafe { NonZeroUsize::new_unchecked(1) });
    const DEFAULT: MaxSegments = MaxSegments(unsafe { NonZeroUsize::new_unchecked(1) });
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

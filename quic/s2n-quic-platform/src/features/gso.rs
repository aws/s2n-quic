// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::num::NonZeroUsize;

#[derive(Debug)]
pub struct Gso {
    max_segments: NonZeroUsize,
}

impl Default for Gso {
    fn default() -> Self {
        let max_segments = if cfg!(target_os = "linux") {
            // https://github.com/torvalds/linux/blob/e9f1cbc0c4114880090c7a578117d3b9cf184ad4/tools/testing/selftests/net/udpgso.c#L37
            // ```
            // #define UDP_MAX_SEGMENTS	(1 << 6UL)
            // ```
            1 << 6
        } else {
            1
        };

        let max_segments = NonZeroUsize::new(max_segments).unwrap();

        Self { max_segments }
    }
}

#[cfg(target_os = "linux")]
impl Gso {
    #[inline]
    pub fn max_segments(&self) -> usize {
        self.max_segments.get()
    }

    #[inline]
    pub fn default_max_segments(&self) -> usize {
        // TODO profile a good default
        // We need to strike a good balance of how deep the message buffers go.
        // If they're too deep then we'll waste a lot of space and be swapping pages
        // frequently. 16 seems like a good place to start as that was about the number
        // of packets being sent at a time on a 1GbE test.
        const DEFAULT_MAX_SEGMENTS: usize = 16;

        self.max_segments().min(DEFAULT_MAX_SEGMENTS)
    }
}

#[cfg(not(target_os = "linux"))]
impl Gso {
    #[inline]
    pub fn max_segments(&self) -> usize {
        1
    }

    #[inline]
    pub fn default_max_segments(&self) -> usize {
        1
    }
}

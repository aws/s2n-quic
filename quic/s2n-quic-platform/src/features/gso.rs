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
    pub fn set(cmsg: &mut [u8], segment_size: usize) -> usize {
        use core::mem::size_of;
        type SegmentType = u16;

        unsafe {
            let len = libc::CMSG_SPACE(size_of::<SegmentType>() as _) as usize;
            debug_assert_ne!(len, 0);
            assert!(
                cmsg.len() >= len,
                "out of space in cmsg: needed {}, got {}",
                len,
                cmsg.len()
            );

            // interpret the start of cmsg as a cmsghdr
            // Safety: the cmsg slice should already be zero-initialized and aligned
            debug_assert!(cmsg.iter().all(|b| *b == 0));
            let cmsg = &mut *(&mut cmsg[0] as *mut u8 as *mut libc::cmsghdr);

            // Indicate the type of cmsg
            cmsg.cmsg_level = libc::SOL_UDP;
            cmsg.cmsg_type = libc::UDP_SEGMENT;

            // tell the kernel how large our value is
            cmsg.cmsg_len = libc::CMSG_LEN(size_of::<SegmentType>() as _) as _;

            // Write the actual value in the data space of the cmsg
            // Safety: we asserted we had enough space in the cmsg buffer above
            core::ptr::write(
                libc::CMSG_DATA(cmsg) as *const _ as *mut _,
                segment_size as SegmentType,
            );

            len
        }
    }

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
    pub fn set(_cmsg: &mut [u8], _segment_size: usize) -> usize {
        panic!("cannot use GSO on the current platform")
    }

    #[inline]
    pub fn max_segments(&self) -> usize {
        1
    }

    #[inline]
    pub fn default_max_segments(&self) -> usize {
        1
    }
}

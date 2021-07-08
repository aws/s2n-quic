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

        let len = unsafe { libc::CMSG_SPACE(size_of::<SegmentType>() as _) } as usize;
        debug_assert_ne!(len, 0);
        assert!(
            cmsg.len() >= len,
            "out of space in cmsg: needed {}, got {}",
            len,
            cmsg.len()
        );

        let cmsg = unsafe { &mut *(&mut cmsg[0] as *mut u8 as *mut libc::cmsghdr) };
        cmsg.cmsg_level = libc::SOL_UDP;
        cmsg.cmsg_type = libc::UDP_SEGMENT;
        cmsg.cmsg_len = unsafe { libc::CMSG_LEN(size_of::<SegmentType>() as _) } as _;
        unsafe {
            core::ptr::write(
                libc::CMSG_DATA(cmsg) as *const _ as *mut _,
                segment_size as SegmentType,
            );
        }

        len
    }

    pub fn max_segments(&self) -> usize {
        self.max_segments.get()
    }

    pub fn default_max_segments(&self) -> usize {
        // TODO profile a good default
        const DEFAULT_MAX_SEGMENTS: usize = 16;

        self.max_segments().min(DEFAULT_MAX_SEGMENTS)
    }
}

#[cfg(not(target_os = "linux"))]
impl Gso {
    pub fn set(_cmsg: &mut [u8], _segment_size: usize) -> usize {
        panic!("cannot use GSO on the current platform")
    }

    pub fn max_segments(&self) -> usize {
        1
    }

    pub fn default_max_segments(&self) -> usize {
        1
    }
}

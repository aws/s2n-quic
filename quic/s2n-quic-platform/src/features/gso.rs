// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::c_int;
use core::{
    convert::{TryFrom, TryInto},
    fmt,
    fmt::{Display, Formatter},
    num::NonZeroUsize,
};

#[derive(Clone, Copy, Debug)]
pub struct MaxSegments(NonZeroUsize);

impl MaxSegments {
    pub const MAX: Self = gso_impl::MAX_SEGMENTS;
    pub const DEFAULT: Self = gso_impl::DEFAULT_SEGMENTS;
}

impl Default for MaxSegments {
    #[inline]
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
mod gso_enabled {
    use super::*;
    use libc::{SOL_UDP, UDP_SEGMENT};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    pub const LEVEL: Option<c_int> = Some(SOL_UDP as _);
    pub const TYPE: Option<c_int> = Some(UDP_SEGMENT as _);
    pub const CMSG_SPACE: usize = crate::message::cmsg::size_of_cmsg::<super::Cmsg>();

    #[inline]
    pub const fn is_match(level: c_int, ty: c_int) -> bool {
        level == SOL_UDP as c_int && ty == UDP_SEGMENT as c_int
    }

    // This value represents the Maximum value MaxSegments can be set to, i.e. a Max of a Max. The
    // value comes from the Linux kernel:
    //
    // https://github.com/torvalds/linux/blob/e9f1cbc0c4114880090c7a578117d3b9cf184ad4/tools/testing/selftests/net/udpgso.c#L37
    // ```
    // #define UDP_MAX_SEGMENTS	(1 << 6UL)
    // ```
    pub const MAX_SEGMENTS: MaxSegments =
        MaxSegments(unsafe { NonZeroUsize::new_unchecked(1 << 6) });

    // The packet pacer enforces a burst limit of 10 packets, so generally there is no benefit to
    // exceeding that value for GSO segments. However, in low RTT/high bandwidth networks the pacing
    // interval may drop below the timer granularity, resulting in `MAX_BURST_PACKETS` being
    // exceeded. In such networks, setting a MaxSegments size higher than the default may have a
    // positive effect on efficiency.
    //= https://www.rfc-editor.org/rfc/rfc9002#section-7.7
    //# Senders SHOULD limit bursts to the initial congestion window
    pub const DEFAULT_SEGMENTS: MaxSegments = MaxSegments(unsafe {
        NonZeroUsize::new_unchecked(s2n_quic_core::recovery::MAX_BURST_PACKETS as usize)
    });

    #[derive(Clone, Debug)]
    pub struct Gso(Arc<AtomicUsize>);

    impl Default for Gso {
        fn default() -> Self {
            MaxSegments::DEFAULT.into()
        }
    }

    impl Gso {
        #[inline]
        pub fn max_segments(&self) -> usize {
            self.0.load(Ordering::Relaxed)
        }

        #[inline]
        pub fn disable(&self) {
            self.0.store(1, Ordering::Relaxed);
        }

        #[inline]
        pub fn handle_socket_error(&self, error: &std::io::Error) -> Option<usize> {
            let raw_error = error.raw_os_error()?;
            s2n_quic_core::ensure!(raw_error == libc::EIO, None);
            let prev = self.0.swap(1, Ordering::Relaxed);
            Some(prev)
        }
    }

    impl From<MaxSegments> for Gso {
        #[inline]
        fn from(segments: MaxSegments) -> Self {
            Self(Arc::new(AtomicUsize::new(segments.0.into())))
        }
    }
}

#[cfg(any(not(s2n_quic_platform_gso), test))]
mod gso_disabled {
    #![cfg_attr(test, allow(dead_code))]

    use super::*;

    pub const LEVEL: Option<c_int> = None;
    pub const TYPE: Option<c_int> = None;
    pub const CMSG_SPACE: usize = 0;

    #[inline]
    pub const fn is_match(level: c_int, ty: c_int) -> bool {
        let _ = level;
        let _ = ty;
        false
    }

    pub const MAX_SEGMENTS: MaxSegments = MaxSegments(unsafe { NonZeroUsize::new_unchecked(1) });
    pub const DEFAULT_SEGMENTS: MaxSegments = MAX_SEGMENTS;

    #[derive(Clone, Default, Debug)]
    pub struct Gso(());

    impl Gso {
        #[inline]
        pub fn max_segments(&self) -> usize {
            1
        }

        #[inline]
        #[allow(dead_code)] // this may or may not be used on certain platforms
        pub fn disable(&self) {
            // it's already disabled
        }

        #[inline(always)]
        pub fn handle_socket_error(&self, error: &std::io::Error) -> Option<usize> {
            let _ = error;
            None
        }
    }

    impl From<MaxSegments> for Gso {
        #[inline]
        fn from(_segments: MaxSegments) -> Self {
            Self(())
        }
    }
}

mod gso_impl {
    #[cfg(not(s2n_quic_platform_gso))]
    pub use super::gso_disabled::*;
    #[cfg(s2n_quic_platform_gso)]
    pub use super::gso_enabled::*;
}

pub use gso_impl::*;
pub type Cmsg = u16;

pub const IS_SUPPORTED: bool = cfg!(s2n_quic_platform_gso);

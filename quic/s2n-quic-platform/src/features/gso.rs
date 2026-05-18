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

// ── Platform constants (libc-dependent) ──────────────────────────────────────

#[cfg(s2n_quic_platform_gso)]
mod platform {
    use super::*;
    use libc::{SOL_UDP, UDP_SEGMENT};

    pub const LEVEL: Option<c_int> = Some(SOL_UDP as _);
    pub const TYPE: Option<c_int> = Some(UDP_SEGMENT as _);
    pub const CMSG_SPACE: usize = crate::message::cmsg::size_of_cmsg::<super::Cmsg>();

    #[inline]
    pub const fn is_match(level: c_int, ty: c_int) -> bool {
        level == SOL_UDP as c_int && ty == UDP_SEGMENT as c_int
    }

    /// On Linux, EIO from sendmsg indicates the kernel rejected the GSO segmentation.
    #[inline]
    pub fn handle_socket_error(error: &std::io::Error) -> bool {
        error.raw_os_error() == Some(libc::EIO)
    }
}

#[cfg(not(s2n_quic_platform_gso))]
mod platform {
    use super::*;

    pub const LEVEL: Option<c_int> = None;
    pub const TYPE: Option<c_int> = None;
    pub const CMSG_SPACE: usize = 0;

    #[inline]
    pub const fn is_match(_level: c_int, _ty: c_int) -> bool {
        false
    }

    #[inline]
    pub fn handle_socket_error(_error: &std::io::Error) -> bool {
        false
    }
}

pub use platform::{is_match, CMSG_SPACE, LEVEL, TYPE};

// ── Gso struct variants ──────────────────────────────────────────────────────

/// Noop Gso: always reports 1 segment, cannot be reconfigured.
#[cfg(any(not(s2n_quic_platform_gso), test))]
mod gso_noop {
    #![cfg_attr(test, allow(dead_code))]

    use super::*;

    #[derive(Clone, Default, Debug)]
    pub struct Gso(());

    impl Gso {
        #[inline]
        pub fn max_segments(&self) -> usize {
            1
        }

        #[inline]
        pub fn disable(&self) {}

        #[inline(always)]
        pub fn handle_socket_error(&self, _error: &std::io::Error) -> Option<usize> {
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

/// Configurable Gso backed by an atomic counter.
#[cfg(any(s2n_quic_platform_gso, feature = "testing", test))]
mod gso_configurable {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    #[derive(Clone, Debug)]
    pub struct Gso(Arc<AtomicUsize>);

    impl Gso {
        #[cfg(feature = "testing")]
        pub fn for_testing(max_segments: usize) -> Self {
            assert!(max_segments >= 1);
            Self(Arc::new(AtomicUsize::new(max_segments)))
        }

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
            if !platform::handle_socket_error(error) {
                return None;
            }
            let prev = self.0.swap(1, Ordering::Relaxed);
            Some(prev)
        }
    }

    impl Default for Gso {
        /// Returns a Gso with the platform-appropriate default segment count.
        ///
        /// When running inside a bach simulation, the real kernel GSO constraint
        /// does not apply (bach has its own platform-independent networking stack),
        /// so the default is raised to `MAX_BURST_PACKETS` to match Linux behavior.
        /// Outside of bach, this respects the compile-time platform default: 10
        /// segments on Linux (real GSO), 1 segment elsewhere.
        fn default() -> Self {
            #[cfg(feature = "testing")]
            if bach::is_active() {
                return Self(Arc::new(AtomicUsize::new(
                    s2n_quic_core::recovery::MAX_BURST_PACKETS as usize,
                )));
            }

            MaxSegments::DEFAULT.into()
        }
    }

    impl From<MaxSegments> for Gso {
        #[inline]
        fn from(segments: MaxSegments) -> Self {
            Self(Arc::new(AtomicUsize::new(segments.0.into())))
        }
    }
}

// ── Module selection ─────────────────────────────────────────────────────────

mod gso_impl {
    use super::*;

    // Use the configurable variant when GSO is supported or testing is enabled.
    #[cfg(any(s2n_quic_platform_gso, feature = "testing"))]
    pub use super::gso_configurable::Gso;
    #[cfg(not(any(s2n_quic_platform_gso, feature = "testing")))]
    pub use super::gso_noop::Gso;

    // This value represents the Maximum value MaxSegments can be set to, i.e. a Max of a Max. The
    // value comes from the Linux kernel:
    //
    // https://github.com/torvalds/linux/blob/e9f1cbc0c4114880090c7a578117d3b9cf184ad4/tools/testing/selftests/net/udpgso.c#L37
    // ```
    // #define UDP_MAX_SEGMENTS	(1 << 6UL)
    // ```
    #[cfg(any(s2n_quic_platform_gso, feature = "testing"))]
    pub const MAX_SEGMENTS: MaxSegments = MaxSegments(NonZeroUsize::new(1 << 6).unwrap());
    #[cfg(not(any(s2n_quic_platform_gso, feature = "testing")))]
    pub const MAX_SEGMENTS: MaxSegments = MaxSegments(NonZeroUsize::new(1).unwrap());

    // The packet pacer enforces a burst limit of 10 packets, so generally there is no benefit to
    // exceeding that value for GSO segments. However, in low RTT/high bandwidth networks the pacing
    // interval may drop below the timer granularity, resulting in `MAX_BURST_PACKETS` being
    // exceeded. In such networks, setting a MaxSegments size higher than the default may have a
    // positive effect on efficiency.
    //= https://www.rfc-editor.org/rfc/rfc9002#section-7.7
    //# Senders SHOULD limit bursts to the initial congestion window
    #[cfg(s2n_quic_platform_gso)]
    pub const DEFAULT_SEGMENTS: MaxSegments = MaxSegments(
        NonZeroUsize::new(s2n_quic_core::recovery::MAX_BURST_PACKETS as usize).unwrap(),
    );
    #[cfg(not(s2n_quic_platform_gso))]
    pub const DEFAULT_SEGMENTS: MaxSegments = MaxSegments(NonZeroUsize::new(1).unwrap());
}

pub use gso_impl::*;
pub type Cmsg = u16;

pub const IS_SUPPORTED: bool = cfg!(s2n_quic_platform_gso);

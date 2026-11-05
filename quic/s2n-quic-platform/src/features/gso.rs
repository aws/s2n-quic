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

// The module is split into two orthogonal axes:
//
// * `cmsg_*`: how the segment size is communicated to the kernel. This is only
//   meaningful on unix, where the platform encodes a `UDP_SEGMENT` control
//   message into the entry's `msghdr` via `message::cmsg::Encoder`. Windows USO
//   uses `UDP_SEND_MSG_SIZE`, which `syscall::win` writes itself, so the
//   `LEVEL`/`TYPE`/`CMSG_SPACE` constants stay disabled there.
// * `segments_*`: whether more than one packet may be packed into a single ring
//   entry, and the `Gso` handle used to turn that off at runtime. This is
//   enabled for both Linux GSO and Windows USO via the derived
//   `s2n_quic_platform_segment_offload` cfg.
//
// Before Windows support these two were the same switch (`s2n_quic_platform_gso`).

#[cfg(s2n_quic_platform_gso)]
mod cmsg_enabled {
    use super::*;
    use libc::{SOL_UDP, UDP_SEGMENT};

    pub const LEVEL: Option<c_int> = Some(SOL_UDP as _);
    pub const TYPE: Option<c_int> = Some(UDP_SEGMENT as _);
    pub const CMSG_SPACE: usize = crate::message::cmsg::size_of_cmsg::<super::Cmsg>();

    #[inline]
    pub const fn is_match(level: c_int, ty: c_int) -> bool {
        level == SOL_UDP as c_int && ty == UDP_SEGMENT as c_int
    }
}

#[cfg(any(not(s2n_quic_platform_gso), test))]
mod cmsg_disabled {
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
}

#[cfg(s2n_quic_platform_segment_offload)]
mod segments_enabled {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    // This value represents the Maximum value MaxSegments can be set to, i.e. a Max of a Max. The
    // value comes from the Linux kernel:
    //
    // https://github.com/torvalds/linux/blob/e9f1cbc0c4114880090c7a578117d3b9cf184ad4/tools/testing/selftests/net/udpgso.c#L37
    // ```
    // #define UDP_MAX_SEGMENTS	(1 << 6UL)
    // ```
    //
    // Windows doesn't document a segment count limit for USO; it is instead bounded by the
    // `u16::MAX` payload limit that applies to both platforms, so the same value is used.
    pub const MAX_SEGMENTS: MaxSegments = MaxSegments(NonZeroUsize::new(1 << 6).unwrap());

    // The packet pacer enforces a burst limit of 10 packets, so generally there is no benefit to
    // exceeding that value for GSO segments. However, in low RTT/high bandwidth networks the pacing
    // interval may drop below the timer granularity, resulting in `MAX_BURST_PACKETS` being
    // exceeded. In such networks, setting a MaxSegments size higher than the default may have a
    // positive effect on efficiency.
    //= https://www.rfc-editor.org/rfc/rfc9002#section-7.7
    //# Senders SHOULD limit bursts to the initial congestion window
    pub const DEFAULT_SEGMENTS: MaxSegments = MaxSegments(
        NonZeroUsize::new(s2n_quic_core::recovery::MAX_BURST_PACKETS as usize).unwrap(),
    );

    /// The error that indicates the platform refused to segment the payload and
    /// that offloading should be disabled.
    ///
    /// This is a permanent, capability-level failure rather than a transient
    /// per-packet one, so the response is to drop back to one segment per send
    /// for the lifetime of the endpoint.
    #[inline]
    fn is_offload_error(raw_error: i32) -> bool {
        #[cfg(unix)]
        return raw_error == libc::EIO;

        // `WSAEMSGSIZE`: the provider rejected the size of the coalesced buffer.
        //
        // Providers without USO support are detected up front with
        // `syscall::supports_gso`, so this is the only runtime failure that
        // indicates segmentation itself is at fault.
        //
        // The same error is also returned for a datagram larger than the MTU the
        // stack enforces, since `syscall::configure_mtu_disc` sets the DF bit.
        // That case is filtered out by the send path in
        // `io::tokio::task::win::TxSocket::send`, which only reports the error
        // here when the entry actually held multiple segments.
        #[cfg(windows)]
        return raw_error == 10040;
    }

    /// A shared, runtime-adjustable segment count.
    ///
    /// Shared between the `TxQueue` (which reads it to decide how many packets
    /// to pack into an entry) and the socket task (which clears it when the
    /// platform reports an offload error), hence the atomic.
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
            s2n_quic_core::ensure!(is_offload_error(raw_error), None);
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

#[cfg(any(not(s2n_quic_platform_segment_offload), test))]
mod segments_disabled {
    #![cfg_attr(test, allow(dead_code))]

    use super::*;

    pub const MAX_SEGMENTS: MaxSegments = MaxSegments(NonZeroUsize::new(1).unwrap());
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
    pub use super::cmsg_disabled::*;
    #[cfg(s2n_quic_platform_gso)]
    pub use super::cmsg_enabled::*;

    #[cfg(not(s2n_quic_platform_segment_offload))]
    pub use super::segments_disabled::*;
    #[cfg(s2n_quic_platform_segment_offload)]
    pub use super::segments_enabled::*;
}

pub use gso_impl::*;
pub type Cmsg = u16;

/// Indicates the platform reports the segment size with a `UDP_SEGMENT` control message.
///
/// Windows USO uses a different control message (`UDP_SEND_MSG_SIZE`, written by
/// `syscall::win`) so this is `false` there, even though segmentation offload is
/// available. Use [`MaxSegments::MAX`] to check if multiple segments can be written.
pub const IS_SUPPORTED: bool = cfg!(s2n_quic_platform_gso);

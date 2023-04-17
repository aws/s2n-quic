// Copyright Intel Corporation
// SPDX-License-Identifier: GPL-2.0 WITH Linux-syscall-note
// Modifications copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::Result;
use bitflags::bitflags;
use core::mem::size_of;
use std::ffi::CStr;

bitflags!(
    /// Options for the `flags` field in [`Address`]
    ///
    /// See [if_xdp.h](https://github.com/torvalds/linux/blob/2bac7dc169af3cd4a0cb5200aa1f7b89affa042a/include/uapi/linux/if_xdp.h#L16-L27)
    ///
    /// Note that the size of the value is specified in the [sxdp_flags field](
    /// https://github.com/torvalds/linux/blob/0d3eb744aed40ffce820cded61d7eac515199165/include/uapi/linux/if_xdp.h#L34).
    #[derive(Copy, Clone, Debug, Default)]
    #[repr(transparent)]
    pub struct XdpFlags: u16 {
        const SHARED_UMEM = 1 << 0;
        /// Force copy mode
        const COPY = 1 << 1;
        /// Force zero-copy mode
        const ZEROCOPY = 1 << 2;
        /// If this option is set, the driver might go to sleep and in that case the
        /// XDP_RING_NEED_WAKEUP flag in the fill and/or Tx rings will be set.
        ///
        /// If it is set, the application needs to explicitly wake up the driver with a `poll()` for
        /// Rx or `sendto()` for Tx. If you are running the driver and the application on the same
        /// core, you should use this option so that the kernel will yield to the user space
        /// application.
        const USE_NEED_WAKEUP = 1 << 3;
    }
);

bitflags!(
    /// Flags for the umem config
    ///
    /// See [if_xdp.h](https://github.com/torvalds/linux/blob/2bac7dc169af3cd4a0cb5200aa1f7b89affa042a/include/uapi/linux/if_xdp.h#L30>)
    ///
    /// Note that in `if_xdp.h`, the size is left unspecified. However, when it's used in libxdp,
    /// the size is set to `u32`: [xsk.h](https://github.com/xdp-project/xdp-tools/blob/a76e7a2b156b8cfe38992206abe9df1df0a29e38/headers/xdp/xsk.h#L203).
    #[derive(Copy, Clone, Debug, Default)]
    #[repr(transparent)]
    pub struct UmemFlags: u32 {
        const UNALIGNED_CHUNK_FLAG = 1 << 0;
    }
);

/// A structure for representing the address of an AF_XDP socket
///
/// See [if_xdp.h](https://github.com/torvalds/linux/blob/2bac7dc169af3cd4a0cb5200aa1f7b89affa042a/include/uapi/linux/if_xdp.h#L32-L38)
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct Address {
    pub family: u16,
    pub flags: XdpFlags,
    pub ifindex: u32,
    pub queue_id: u32,
    pub shared_umem_fd: u32,
}

impl Default for Address {
    fn default() -> Self {
        Self {
            family: libc::PF_XDP as _,
            flags: Default::default(),
            ifindex: 0,
            queue_id: 0,
            shared_umem_fd: 0,
        }
    }
}

impl Address {
    /// Resolves the `ifindex` for the provided `if_name` and associates it with the address.
    ///
    /// If the device does not exist, then an error is returned.
    #[inline]
    pub fn set_if_name(&mut self, name: &CStr) -> Result<&mut Self> {
        unsafe {
            let ifindex = libc::if_nametoindex(name.as_ptr());

            // https://man7.org/linux/man-pages/man3/if_nametoindex.3.html
            // > On success, if_nametoindex() returns the index number of the
            // > network interface; on error, 0 is returned and errno is set to
            // > indicate the error.
            if ifindex == 0 {
                return Err(std::io::Error::last_os_error());
            }

            self.ifindex = ifindex;
        }
        Ok(self)
    }
}

bitflags!(
    /// Flags set on a particular ring state `flags` field
    ///
    /// See [if_xdp.h](https://github.com/torvalds/linux/blob/2bac7dc169af3cd4a0cb5200aa1f7b89affa042a/include/uapi/linux/if_xdp.h#L41)
    ///
    /// Note that in `if_xdp.h`, the size is left unspecified. However, when it's used in libxdp,
    /// the size is set to `u32`: [xsk.h](
    /// https://github.com/xdp-project/xdp-tools/blob/a76e7a2b156b8cfe38992206abe9df1df0a29e38/headers/xdp/xsk.h#L42)
    #[derive(Default)]
    #[repr(transparent)]
    pub struct RingFlags: u32 {
        const NEED_WAKEUP = 1 << 0;
    }
);

/// A structure to communicate the offsets for an individual ring for an AF_XDP socket
///
/// Note that the `flags` field was added in kernel 5.4.
///
/// See [if_xdp.h](https://github.com/torvalds/linux/blob/9116e5e2b1fff71dce501d971e86a3695acc3dba/include/uapi/linux/if_xdp.h#L28-L32)
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct RingOffsetV1 {
    pub producer: u64,
    pub consumer: u64,
    pub desc: u64,
}

/// A structure to communicate the offsets for an individual ring for an AF_XDP socket
///
/// See [if_xdp.h](https://github.com/torvalds/linux/blob/2bac7dc169af3cd4a0cb5200aa1f7b89affa042a/include/uapi/linux/if_xdp.h#L43)
#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
pub struct RingOffsetV2 {
    pub producer: u64,
    pub consumer: u64,
    pub desc: u64,
    pub flags: u64,
}

impl RingOffsetV2 {
    /// Converts an offset description from an older version
    #[inline]
    fn set_v1(&mut self, v1: RingOffsetV1) {
        // Logic from https://github.com/xdp-project/xdp-tools/blob/a76e7a2b156b8cfe38992206abe9df1df0a29e38/lib/libxdp/xsk.c#L197
        self.producer = v1.producer;
        self.consumer = v1.consumer;
        self.desc = v1.desc;
        self.flags = v1.consumer + (size_of::<u32>() as u64);
    }
}

/// A structure to communicate the offsets of each of the 4 rings for an AF_XDP socket
///
/// See [if_xdp.h](https://github.com/torvalds/linux/blob/2bac7dc169af3cd4a0cb5200aa1f7b89affa042a/include/uapi/linux/if_xdp.h#L50)
#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
pub struct MmapOffsets<O = RingOffsetV2> {
    pub rx: O,
    pub tx: O,
    pub fill: O,
    pub completion: O,
}

impl MmapOffsets {
    /// See [if_xdp.h](https://github.com/torvalds/linux/blob/2bac7dc169af3cd4a0cb5200aa1f7b89affa042a/include/uapi/linux/if_xdp.h#L92)
    pub const RX_RING: usize = 0;
    /// See [if_xdp.h](https://github.com/torvalds/linux/blob/2bac7dc169af3cd4a0cb5200aa1f7b89affa042a/include/uapi/linux/if_xdp.h#L93)
    pub const TX_RING: usize = 0x8000_0000;
    /// See [if_xdp.h](https://github.com/torvalds/linux/blob/2bac7dc169af3cd4a0cb5200aa1f7b89affa042a/include/uapi/linux/if_xdp.h#L94)
    pub const FILL_RING: usize = 0x1_0000_0000;
    /// See [if_xdp.h](https://github.com/torvalds/linux/blob/2bac7dc169af3cd4a0cb5200aa1f7b89affa042a/include/uapi/linux/if_xdp.h#L95)
    pub const COMPLETION_RING: usize = 0x1_8000_0000;

    #[inline]
    pub(crate) fn as_v1(mut self) -> Self {
        // getsockopt on a kernel <= 5.3 has no flags fields.
        // Copy over the offsets to the correct places in the >=5.4 format
        // and put the flags where they would have been on that kernel.

        // Logic from https://github.com/xdp-project/xdp-tools/blob/a76e7a2b156b8cfe38992206abe9df1df0a29e38/lib/libxdp/xsk.c#L197
        let v1 = unsafe { *(&self as *const Self as *const MmapOffsets<RingOffsetV1>) };
        self.rx.set_v1(v1.rx);
        self.tx.set_v1(v1.tx);
        self.fill.set_v1(v1.fill);
        self.completion.set_v1(v1.completion);

        self
    }
}

/// Socket option ids passed to an AF_XDP socket with the sockopt syscalls.
///
/// See [if_xdp.h](https://github.com/torvalds/linux/blob/2bac7dc169af3cd4a0cb5200aa1f7b89affa042a/include/uapi/linux/if_xdp.h#L58-L65)
///
/// Note that in `if_xdp.h`, the size is left unspecified. However, it is used in the `name`
/// argument for [`libc::setsockopt`] and [`libc::getsockopt`], which is a [`libc::c_int`] (i32).
#[derive(Clone, Copy, Debug)]
#[repr(i32)]
pub enum SocketOptions {
    MmapOffsets = 1,
    RxRing = 2,
    TxRing = 3,
    UmemReg = 4,
    UmemFillRing = 5,
    UmemCompletionRing = 6,
    Statistics = 7,
    Options = 8,
}

/// Umem configuration on an AF_XDP socket
///
/// This struct is used with the [`libc::setsockopt`] call with the `name` parameter set to
/// [`SocketOptions::UmemReg`].
///
/// See [if_xdp.h](https://github.com/torvalds/linux/blob/2bac7dc169af3cd4a0cb5200aa1f7b89affa042a/include/uapi/linux/if_xdp.h#L67-L73).
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct UmemReg {
    /// Start of packet data area
    pub addr: u64,
    /// Length of packet data area
    pub len: u64,
    pub chunk_size: u32,
    /// The length reserved before packets
    pub headroom: u32,
    /// The flag value for the Umem
    pub flags: UmemFlags,
}

/// Statistics returned from an AF_XDP socket
///
/// This struct is used with the [`libc::getsockopt`] call with the `name` parameter set to
/// [`SocketOptions::Statistics`].
///
/// See [if_xdp.h](https://github.com/torvalds/linux/blob/2bac7dc169af3cd4a0cb5200aa1f7b89affa042a/include/uapi/linux/if_xdp.h#L75-L82)
#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
pub struct Statistics {
    /// Dropped for other reasons
    pub rx_dropped: u64,
    /// Dropped due to invalid descriptor
    pub rx_invalid_descriptors: u64,
    /// Dropped due to invalid descriptor
    pub tx_invalid_descriptors: u64,
    /// Dropped due to rx ring being full
    pub rx_ring_full: u64,
    /// Failed to retrieve item from fill ring
    pub rx_fill_ring_empty_descriptors: u64,
    /// Failed to retrieve item from tx ring
    pub tx_ring_empty_descriptors: u64,
}

bitflags!(
    /// Options returned from an AF_XDP socket
    ///
    /// This struct is used with the [`libc::getsockopt`] call with the `name` parameter set to
    /// [`SocketOptions::Options`].
    ///
    /// See [if_xdp.h](https://github.com/torvalds/linux/blob/2bac7dc169af3cd4a0cb5200aa1f7b89affa042a/include/uapi/linux/if_xdp.h#L89)
    #[derive(Clone, Copy, Debug, Default)]
    #[repr(transparent)]
    pub struct XdpOptions: u32 {
        const ZEROCOPY = 1 << 0;
    }
);

/// Masks for unaligned chunks mode
///
/// See [if_xdp](https://github.com/torvalds/linux/blob/0d3eb744aed40ffce820cded61d7eac515199165/include/uapi/linux/if_xdp.h#L97-L100).
pub const XSK_UNALIGNED_BUF_ADDR_MASK: u64 = (1 << XSK_UNALIGNED_BUF_OFFSET_SHIFT) - 1;
pub const XSK_UNALIGNED_BUF_OFFSET_SHIFT: u64 = 48;

/// Rx/Tx descriptor
///
/// See [if_xdp.h](https://github.com/torvalds/linux/blob/2bac7dc169af3cd4a0cb5200aa1f7b89affa042a/include/uapi/linux/if_xdp.h#L103-L107)
#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
pub struct RxTxDescriptor {
    /// Offset into the umem where the packet starts
    pub address: u64,
    /// Length of the packet
    pub len: u32,
    /// Options set on the descriptor
    pub options: u32,
}

/// Umem Descriptor
///
/// See [if_xdp.h](https://github.com/torvalds/linux/blob/0d3eb744aed40ffce820cded61d7eac515199165/include/uapi/linux/if_xdp.h#L109).
#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
pub struct UmemDescriptor {
    /// Offset into the umem where the packet starts
    pub address: u64,
}

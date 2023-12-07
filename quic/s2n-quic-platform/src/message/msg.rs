// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    features,
    message::{cmsg, cmsg::Encoder, Message as MessageTrait},
};
use core::{
    alloc::Layout,
    mem::{size_of, size_of_val},
};
use libc::{iovec, msghdr, sockaddr_in, sockaddr_in6, AF_INET, AF_INET6};
use s2n_quic_core::{
    inet::{
        datagram, ExplicitCongestionNotification, IpV4Address, IpV6Address, SocketAddress,
        SocketAddressV4, SocketAddressV6,
    },
    io::tx,
    path::{self, Handle as _},
};

mod ext;
mod handle;
#[cfg(test)]
mod tests;

pub use ext::Ext;
pub use handle::Handle;
pub use libc::msghdr as Message;

#[cfg(any(test, feature = "generator"))]
use bolero_generator::*;

impl MessageTrait for msghdr {
    type Handle = Handle;

    const SUPPORTS_GSO: bool = features::gso::IS_SUPPORTED;
    const SUPPORTS_ECN: bool = cfg!(s2n_quic_platform_tos);
    const SUPPORTS_FLOW_LABELS: bool = true;

    #[inline]
    fn alloc(entries: u32, payload_len: u32, offset: usize) -> super::Storage {
        unsafe { alloc(entries, payload_len, offset, |msg| msg) }
    }

    #[inline]
    fn payload_len(&self) -> usize {
        debug_assert!(!self.msg_iov.is_null());
        let len = unsafe { (*self.msg_iov).iov_len as _ };
        debug_assert!(len <= u16::MAX as usize);
        len
    }

    #[inline]
    unsafe fn set_payload_len(&mut self, payload_len: usize) {
        debug_assert!(payload_len <= u16::MAX as usize);
        debug_assert!(!self.msg_iov.is_null());
        (*self.msg_iov).iov_len = payload_len;
    }

    #[inline]
    fn set_segment_size(&mut self, size: usize) {
        let level = features::gso::LEVEL.expect("gso is unsupported");
        let ty = features::gso::TYPE.expect("gso is unsupported");
        self.encode_cmsg(level, ty, size as features::gso::Cmsg)
            .unwrap();
    }

    #[inline]
    unsafe fn reset(&mut self, mtu: usize) {
        // reset the payload
        self.set_payload_len(mtu);

        // reset the address
        self.set_remote_address(&SocketAddress::IpV6(Default::default()));

        #[inline]
        unsafe fn check_cmsg(msghdr: &msghdr) {
            if cfg!(debug_assertions) {
                let ptr = msghdr.msg_control as *mut u8;
                let cmsg = core::slice::from_raw_parts_mut(ptr, cmsg::MAX_LEN);
                // make sure nothing was written to the control message if it was set to 0
                #[cfg(not(kani))]
                {
                    assert!(cmsg.iter().all(|v| *v == 0), "msg_control was not cleared");
                }

                #[cfg(kani)]
                {
                    let index: usize = kani::any();
                    kani::assume(index < cmsg.len());
                    assert_eq!(cmsg[index], 0);
                }
            }
        }

        // make sure we didn't get any data written without setting the len
        if self.msg_controllen == 0 {
            check_cmsg(self);
        }

        // reset the control messages if it isn't set to the default value

        // some platforms encode lengths as `u32` so we cast everything to be safe
        #[allow(clippy::unnecessary_cast)]
        let msg_controllen = self.msg_controllen as usize;

        if msg_controllen != cmsg::MAX_LEN {
            core::slice::from_raw_parts_mut(self.msg_control as *mut u8, msg_controllen).fill(0);
        }

        check_cmsg(self);

        self.msg_controllen = cmsg::MAX_LEN as _;
    }

    #[inline]
    fn payload_ptr_mut(&mut self) -> *mut u8 {
        unsafe {
            let iovec = &mut *self.msg_iov;
            iovec.iov_base as *mut _
        }
    }

    #[inline]
    fn validate_replication(source: &Self, dest: &Self) {
        assert_eq!(source.msg_name, dest.msg_name);
        assert_eq!(source.msg_iov, dest.msg_iov);
        assert_eq!(source.msg_control, dest.msg_control);
    }

    #[inline]
    fn rx_read(
        &mut self,
        local_address: &path::LocalAddress,
    ) -> Option<super::RxMessage<Self::Handle>> {
        if cfg!(test) {
            assert_eq!(
                self.msg_flags & libc::MSG_CTRUNC,
                0,
                "control message buffers should always have enough capacity"
            );
        }

        let (mut header, cmsg) = self.header()?;

        // only copy the port if we are told the IP address
        if cfg!(s2n_quic_platform_pktinfo) {
            header.path.local_address.set_port(local_address.port());
        } else {
            header.path.local_address = *local_address;
        }

        let payload = self.payload_mut();

        let segment_size = if cmsg.segment_size == 0 {
            payload.len()
        } else {
            cmsg.segment_size as _
        };

        let message = crate::message::RxMessage {
            header,
            segment_size,
            payload,
        };

        Some(message)
    }

    #[inline]
    fn tx_write<M: tx::Message<Handle = Self::Handle>>(
        &mut self,
        mut message: M,
    ) -> Result<usize, tx::Error> {
        let payload = self.payload_mut();

        let max_len = payload.len();
        let len = message.write_payload(tx::PayloadBuffer::new(payload), 0)?;

        debug_assert_ne!(len, 0);
        debug_assert!(len <= max_len);
        let len = len.min(max_len);

        debug_assert_eq!(
            cmsg::MAX_LEN,
            self.msg_controllen as _,
            "message should be reset before writing"
        );
        self.msg_controllen = 0;

        unsafe {
            self.set_payload_len(len);
        }

        let handle = *message.path_handle();
        handle.update_msg_hdr(self);
        self.set_ecn(message.ecn(), &handle.remote_address.0);

        Ok(len)
    }
}

/// Allocates a region of memory holding `entries` number of `T` messages, each with `payload_len`
/// payloads.
///
/// # Safety
///
/// * `T` can be initialized with zero bytes and still be valid
#[inline]
pub(super) unsafe fn alloc<T: Copy + Sized, F: Fn(&mut T) -> &mut msghdr>(
    entries: u32,
    payload_len: u32,
    offset: usize,
    on_entry: F,
) -> super::Storage {
    // calculate the layout of the storage for the given configuration
    let (layout, entry_offset, header_offset, payload_offset) =
        layout::<T>(entries, payload_len, offset);

    // allocate a single contiguous block of memory
    let storage = super::Storage::new(layout);

    {
        let ptr = storage.as_ptr();

        // calculate each of the pointers we need to set up a message
        let mut entry_ptr = ptr.add(entry_offset) as *mut T;
        let mut header_ptr = ptr.add(header_offset) as *mut Header;
        let mut payload_ptr = ptr.add(payload_offset);

        for _ in 0..entries {
            // for each message update all of the pointers to the correct locations

            let entry = on_entry(&mut *entry_ptr);
            (*header_ptr).update(entry, payload_ptr, payload_len);

            // increment the pointers for the next iteration
            entry_ptr = entry_ptr.add(1);
            header_ptr = header_ptr.add(1);
            payload_ptr = payload_ptr.add(payload_len as _);

            // make sure the pointers are within the bounds of the allocation
            storage.check_bounds(entry_ptr);
            storage.check_bounds(header_ptr);
            storage.check_bounds(payload_ptr);
        }

        // replicate the primary messages into the secondary region
        let primary = ptr.add(entry_offset) as *mut T;
        let secondary = primary.add(entries as _);
        storage.check_bounds(secondary.add(entries as _));
        core::ptr::copy_nonoverlapping(primary, secondary, entries as _);
    }

    storage
}

/// Computes the following layout
///
/// ```ignore
/// struct Storage {
///    cursor: Cursor,
///    headers: [Header; entries],
///    payloads: [[u8; payload_len]; entries],
///    entries: [T; entries * 2],
/// }
/// ```
fn layout<T: Copy + Sized>(
    entries: u32,
    payload_len: u32,
    offset: usize,
) -> (Layout, usize, usize, usize) {
    let cursor = Layout::array::<u8>(offset).unwrap();
    let headers = Layout::array::<Header>(entries as _).unwrap();
    let payloads = Layout::array::<u8>(entries as usize * payload_len as usize).unwrap();
    // double the number of entries we allocate to support the primary/secondary regions
    let entries = Layout::array::<T>((entries * 2) as usize).unwrap();
    let (layout, entry_offset) = cursor.extend(entries).unwrap();
    let (layout, header_offset) = layout.extend(headers).unwrap();
    let (layout, payload_offset) = layout.extend(payloads).unwrap();
    (layout, entry_offset, header_offset, payload_offset)
}

/// A structure for holding data pointed to in the [`libc::msghdr`] struct.
struct Header {
    pub iovec: iovec,
    pub msg_name: sockaddr_in6,
    pub cmsg: cmsg::Storage,
}

impl Header {
    /// sets all of the pointers of the provided `entry` to the correct locations
    unsafe fn update(&mut self, entry: &mut msghdr, payload: *mut u8, payload_len: u32) {
        let iovec = &mut self.iovec;

        iovec.iov_base = payload as *mut _;
        iovec.iov_len = payload_len as _;

        let entry = &mut *entry;

        entry.msg_name = &mut self.msg_name as *mut _ as *mut _;
        entry.msg_namelen = size_of_val(&self.msg_name) as _;
        entry.msg_iov = &mut self.iovec as *mut _;
        entry.msg_iovlen = 1;
        entry.msg_controllen = self.cmsg.len() as _;
        entry.msg_control = self.cmsg.as_mut_ptr() as *mut _;

        // make sure that the control pointer is well-aligned
        debug_assert_eq!(
            entry
                .msg_control
                .align_offset(core::mem::align_of::<cmsg::Storage>()),
            0
        );
    }
}

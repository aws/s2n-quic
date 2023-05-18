// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::message::{cmsg, cmsg::Encoder, Message as MessageTrait};
use alloc::boxed::Box;
use core::{
    alloc::Layout,
    cell::UnsafeCell,
    mem::{size_of, size_of_val},
    ptr::NonNull,
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

use ext::Ext as _;

pub use handle::Handle;
pub use libc::msghdr as Message;

#[cfg(any(test, feature = "generator"))]
use bolero_generator::*;

impl MessageTrait for msghdr {
    type Handle = Handle;

    const SUPPORTS_GSO: bool = cfg!(s2n_quic_platform_gso);
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

    #[cfg(s2n_quic_platform_gso)]
    #[inline]
    fn set_segment_size(&mut self, size: usize) {
        self.encode_cmsg(libc::SOL_UDP, libc::UDP_SEGMENT, size as cmsg::UdpGso);
    }

    #[inline]
    unsafe fn reset(&mut self, mtu: usize) {
        // reset the payload
        self.set_payload_len(mtu);

        // reset the address
        self.set_remote_address(&SocketAddress::IpV6(Default::default()));

        if cfg!(debug_assertions) && self.msg_controllen == 0 {
            // make sure nothing was written to the control message if it was set to 0
            assert!(
                core::slice::from_raw_parts_mut(self.msg_control as *mut u8, cmsg::MAX_LEN)
                    .iter()
                    .all(|v| *v == 0)
            )
        }

        // reset the control messages if it isn't set to the default value

        // some platforms encode lengths as `u32` so we cast everything to be safe
        #[allow(clippy::unnecessary_cast)]
        let msg_controllen = self.msg_controllen as usize;

        if msg_controllen != cmsg::MAX_LEN {
            let cmsg = core::slice::from_raw_parts_mut(self.msg_control as *mut u8, msg_controllen);

            for byte in cmsg.iter_mut() {
                *byte = 0;
            }
        }

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

        let len = message.write_payload(tx::PayloadBuffer::new(payload), 0)?;

        unsafe {
            debug_assert!(len <= payload.len());
            let len = len.min(payload.len());
            self.set_payload_len(len);
        }

        let handle = *message.path_handle();
        handle.update_msg_hdr(self);
        self.set_ecn(message.ecn(), &handle.remote_address.0);

        Ok(len)
    }
}

#[inline]
pub(super) unsafe fn alloc<T: Copy + Sized, F: Fn(&mut T) -> &mut msghdr>(
    entries: u32,
    payload_len: u32,
    offset: usize,
    on_entry: F,
) -> super::Storage {
    let (layout, entry_offset, header_offset, payload_offset) =
        layout::<T>(entries, payload_len, offset);

    let ptr = alloc::alloc::alloc_zeroed(layout);

    let end_pointer = ptr.add(layout.size());

    let ptr = NonNull::new(ptr).expect("could not allocate socket message ring");

    {
        let mut entry_ptr = ptr.as_ptr().add(entry_offset) as *mut UnsafeCell<T>;
        let mut header_ptr = ptr.as_ptr().add(header_offset) as *mut UnsafeCell<Header>;
        let mut payload_ptr = ptr.as_ptr().add(payload_offset) as *mut UnsafeCell<u8>;
        for _ in 0..entries {
            let entry = on_entry((*entry_ptr).get_mut());
            (*header_ptr)
                .get_mut()
                .update(entry, &*payload_ptr, payload_len);

            entry_ptr = entry_ptr.add(1);
            debug_assert!(end_pointer >= entry_ptr as *mut u8);
            header_ptr = header_ptr.add(1);
            debug_assert!(end_pointer >= header_ptr as *mut u8);
            payload_ptr = payload_ptr.add(payload_len as _);
            debug_assert!(end_pointer >= payload_ptr as *mut u8);
        }

        let primary = ptr.as_ptr().add(entry_offset) as *mut T;
        let secondary = primary.add(entries as _);
        debug_assert!(end_pointer >= secondary.add(entries as _) as *mut u8);
        core::ptr::copy_nonoverlapping(primary, secondary, entries as _);
    }

    let slice = core::slice::from_raw_parts_mut(ptr.as_ptr() as *mut UnsafeCell<u8>, layout.size());
    Box::from_raw(slice).into()
}

fn layout<T: Copy + Sized>(
    entries: u32,
    payload_len: u32,
    offset: usize,
) -> (Layout, usize, usize, usize) {
    let cursor = Layout::array::<UnsafeCell<u8>>(offset).unwrap();
    let headers = Layout::array::<UnsafeCell<Header>>(entries as _).unwrap();
    let payloads =
        Layout::array::<UnsafeCell<u8>>(entries as usize * payload_len as usize).unwrap();
    let entries = Layout::array::<UnsafeCell<T>>((entries * 2) as usize).unwrap();
    let (layout, entry_offset) = cursor.extend(entries).unwrap();
    let (layout, header_offset) = layout.extend(headers).unwrap();
    let (layout, payload_offset) = layout.extend(payloads).unwrap();
    (layout, entry_offset, header_offset, payload_offset)
}

#[repr(C)]
struct Header {
    pub iovec: Aligned<iovec>,
    pub msg_name: Aligned<sockaddr_in6>,
    pub cmsg: Aligned<[u8; cmsg::MAX_LEN]>,
}

#[repr(C, align(8))]
struct Aligned<T>(UnsafeCell<T>);

impl Header {
    unsafe fn update(&mut self, entry: &mut msghdr, payload: &UnsafeCell<u8>, payload_len: u32) {
        let iovec = self.iovec.0.get_mut();

        iovec.iov_base = payload.get() as *mut _;
        iovec.iov_len = payload_len as _;

        let entry = &mut *entry;

        entry.msg_name = self.msg_name.0.get() as *mut _;
        entry.msg_namelen = size_of_val(&self.msg_name) as _;
        entry.msg_iov = self.iovec.0.get();
        entry.msg_iovlen = 1;
        entry.msg_control = self.cmsg.0.get() as *mut _;
        entry.msg_controllen = cmsg::MAX_LEN as _;
    }
}

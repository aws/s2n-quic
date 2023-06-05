// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::message::{cmsg, cmsg::Encoder, Message as MessageTrait};
use alloc::vec::Vec;
use core::{
    alloc::Layout,
    cell::UnsafeCell,
    mem::{size_of, size_of_val, zeroed},
    pin::Pin,
    ptr::NonNull,
};
use libc::{c_void, iovec, msghdr, sockaddr_in, sockaddr_in6, AF_INET, AF_INET6};
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

fn new(
    iovec: *mut iovec,
    msg_name: *mut c_void,
    msg_namelen: usize,
    msg_control: *mut c_void,
    msg_controllen: usize,
) -> Message {
    let mut msghdr = unsafe { core::mem::zeroed::<msghdr>() };

    msghdr.msg_iov = iovec;
    msghdr.msg_iovlen = 1; // a single iovec is allocated per message

    msghdr.msg_name = msg_name;
    msghdr.msg_namelen = msg_namelen as _;

    msghdr.msg_control = msg_control;
    msghdr.msg_controllen = msg_controllen as _;

    msghdr
}

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
        unsafe { (*self.msg_iov).iov_len }
    }

    #[inline]
    unsafe fn set_payload_len(&mut self, payload_len: usize) {
        debug_assert!(!self.msg_iov.is_null());
        (*self.msg_iov).iov_len = payload_len;
    }

    #[inline]
    fn can_gso<M: tx::Message<Handle = Self::Handle>>(&self, other: &mut M) -> bool {
        if let Some((header, _cmsg)) = self.header() {
            let mut other_handle = *other.path_handle();

            // when reading the header back from the msghdr, we don't know the port
            // so set the other port to 0 as well.
            other_handle.local_address.set_port(0);

            // check the path handles match
            header.path.strict_eq(&other_handle) &&
                // check the ECN markings match
                header.ecn == other.ecn()
        } else {
            false
        }
    }

    #[cfg(s2n_quic_platform_gso)]
    #[inline]
    fn set_segment_size(&mut self, size: usize) {
        type SegmentType = u16;
        self.encode_cmsg(libc::SOL_UDP, libc::UDP_SEGMENT, size as SegmentType);
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
    fn replicate_fields_from(&mut self, other: &Self) {
        debug_assert_eq!(
            self.msg_name, other.msg_name,
            "msg_name needs to point to the same data"
        );
        debug_assert_eq!(
            self.msg_control, other.msg_control,
            "msg_control needs to point to the same data"
        );
        debug_assert_eq!(self.msg_iov, other.msg_iov);
        debug_assert_eq!(self.msg_iovlen, other.msg_iovlen);
        self.msg_namelen = other.msg_namelen;
        self.msg_controllen = other.msg_controllen;
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
    // calculate the layout of the storage for the given configuration
    let (layout, entry_offset, header_offset, payload_offset) =
        layout::<T>(entries, payload_len, offset);

    // allocate a single contiguous block of memory
    let ptr = alloc::alloc::alloc_zeroed(layout);

    // compute the end pointer of the whole allocation so we can check ourselves on the pointer
    // arithmetic.
    let end_pointer = ptr.add(layout.size());

    // make sure the allocation didn't fail
    let ptr = NonNull::new(ptr).expect("could not allocate socket message ring");

    {
        // calculate each of the pointers we need to set up a message
        let mut entry_ptr = ptr.as_ptr().add(entry_offset) as *mut UnsafeCell<T>;
        let mut header_ptr = ptr.as_ptr().add(header_offset) as *mut UnsafeCell<Header>;
        let mut payload_ptr = ptr.as_ptr().add(payload_offset) as *mut UnsafeCell<u8>;

        for _ in 0..entries {
            // for each message update all of the pointers to the correct locations

            let entry = on_entry((*entry_ptr).get_mut());
            (*header_ptr)
                .get_mut()
                .update(entry, &*payload_ptr, payload_len);

            // increment the pointers for the next iteration
            entry_ptr = entry_ptr.add(1);
            debug_assert!(end_pointer >= entry_ptr as *mut u8);
            header_ptr = header_ptr.add(1);
            debug_assert!(end_pointer >= header_ptr as *mut u8);
            payload_ptr = payload_ptr.add(payload_len as _);
            debug_assert!(end_pointer >= payload_ptr as *mut u8);
        }

        // replicate the primary messages into the secondary region
        let primary = ptr.as_ptr().add(entry_offset) as *mut T;
        let secondary = primary.add(entries as _);
        debug_assert!(end_pointer >= secondary.add(entries as _) as *mut u8);
        core::ptr::copy_nonoverlapping(primary, secondary, entries as _);
    }

    let slice = core::slice::from_raw_parts_mut(ptr.as_ptr() as *mut UnsafeCell<u8>, layout.size());
    Box::from_raw(slice).into()
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
    let cursor = Layout::array::<UnsafeCell<u8>>(offset).unwrap();
    let headers = Layout::array::<UnsafeCell<Header>>(entries as _).unwrap();
    let payloads =
        Layout::array::<UnsafeCell<u8>>(entries as usize * payload_len as usize).unwrap();
    // double the number of entries we allocate to support the primary/secondary regions
    let entries = Layout::array::<UnsafeCell<T>>((entries * 2) as usize).unwrap();
    let (layout, entry_offset) = cursor.extend(entries).unwrap();
    let (layout, header_offset) = layout.extend(headers).unwrap();
    let (layout, payload_offset) = layout.extend(payloads).unwrap();
    (layout, entry_offset, header_offset, payload_offset)
}

/// A structure for holding data pointed to in the [`libc::msghdr`] struct.
#[repr(C)]
struct Header {
    pub iovec: Aligned<iovec>,
    pub msg_name: Aligned<sockaddr_in6>,
    pub cmsg: Aligned<[u8; cmsg::MAX_LEN]>,
}

/// Ensures the `T` is aligned to the nearest 8 bytes
///
/// This is required for each type to make sure the pointer is well-aligned
#[repr(C, align(8))]
struct Aligned<T>(UnsafeCell<T>);

impl Header {
    /// sets all of the pointers of the provided `entry` to the correct locations
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

pub struct Ring<Payloads> {
    pub(crate) messages: Vec<msghdr>,
    pub(crate) storage: Storage<Payloads>,
}

pub struct Storage<Payloads> {
    // this field holds references to allocated payloads, but is never read directly
    #[allow(dead_code)]
    pub(crate) payloads: Pin<Payloads>,

    // this field holds references to allocated iovecs, but is never read directly
    #[allow(dead_code)]
    pub(crate) iovecs: Pin<Box<[iovec]>>,

    // this field holds references to allocated msg_names, but is never read directly
    #[allow(dead_code)]
    pub(crate) msg_names: Pin<Box<[sockaddr_in6]>>,

    // this field holds references to allocated msg_names, but is never read directly
    #[allow(dead_code)]
    pub(crate) cmsgs: Pin<Box<[u8]>>,

    /// The maximum payload for any given message
    mtu: usize,

    /// The maximum number of segments that can be offloaded in a single message
    gso: crate::features::Gso,
}

impl<Payloads: crate::buffer::Buffer> Storage<Payloads> {
    #[inline]
    pub fn mtu(&self) -> usize {
        self.mtu
    }

    #[inline]
    pub fn max_gso(&self) -> usize {
        self.gso.max_segments()
    }

    #[inline]
    pub fn disable_gso(&mut self) {
        // TODO recompute message offsets
        // https://github.com/aws/s2n-quic/issues/762
        self.gso.disable()
    }
}

/// Even though `Ring` contains raw pointers, it owns all of the data
/// and can be sent across threads safely.
#[allow(unknown_lints, clippy::non_send_fields_in_send_ty)]
unsafe impl<Payloads: Send> Send for Ring<Payloads> {}

impl<Payloads: crate::buffer::Buffer + Default> Default for Ring<Payloads> {
    fn default() -> Self {
        Self::new(
            Payloads::default(),
            crate::features::gso::MaxSegments::DEFAULT.into(),
        )
    }
}

impl<Payloads: crate::buffer::Buffer> Ring<Payloads> {
    pub fn new(payloads: Payloads, max_gso: usize) -> Self {
        let gso = crate::features::gso::MaxSegments::try_from(max_gso)
            .expect("invalid max segments value")
            .into();

        let mtu = payloads.mtu();
        let capacity = payloads.len() / mtu / max_gso;

        let mut payloads = Pin::new(payloads);
        let mut iovecs = Pin::new(vec![unsafe { zeroed() }; capacity].into_boxed_slice());
        let mut msg_names = Pin::new(vec![unsafe { zeroed() }; capacity].into_boxed_slice());
        let mut cmsgs = Pin::new(vec![0u8; capacity * cmsg::MAX_LEN].into_boxed_slice());

        // double message capacity to enable contiguous access
        let mut messages = Vec::with_capacity(capacity * 2);

        let mut payload_buf = &mut payloads.as_mut()[..];
        let mut cmsg_buf = &mut cmsgs.as_mut()[..];

        for index in 0..capacity {
            let (payload, remaining) = payload_buf.split_at_mut(mtu * max_gso);
            payload_buf = remaining;
            let (cmsg, remaining) = cmsg_buf.split_at_mut(cmsg::MAX_LEN);
            cmsg_buf = remaining;

            let mut iovec = unsafe { zeroed::<iovec>() };
            iovec.iov_base = payload.as_mut_ptr() as _;
            iovec.iov_len = mtu;
            iovecs[index] = iovec;

            let msg = new(
                (&mut iovecs[index]) as *mut _,
                (&mut msg_names[index]) as *mut _ as *mut _,
                size_of::<sockaddr_in6>(),
                cmsg as *mut _ as *mut _,
                cmsg::MAX_LEN,
            );

            messages.push(msg);
        }

        for index in 0..capacity {
            messages.push(messages[index]);
        }

        Self {
            messages,
            storage: Storage {
                payloads,
                iovecs,
                msg_names,
                cmsgs,
                mtu,
                gso,
            },
        }
    }
}

impl<Payloads: crate::buffer::Buffer> super::Ring for Ring<Payloads> {
    type Message = Message;

    #[inline]
    fn len(&self) -> usize {
        self.messages.len() / 2
    }

    #[inline]
    fn mtu(&self) -> usize {
        self.storage.mtu()
    }

    #[inline]
    fn max_gso(&self) -> usize {
        // TODO recompute message offsets
        self.storage.max_gso()
    }

    fn disable_gso(&mut self) {
        self.storage.disable_gso()
    }

    #[inline]
    fn as_slice(&self) -> &[Self::Message] {
        &self.messages[..]
    }

    #[inline]
    fn as_mut_slice(&mut self) -> &mut [Self::Message] {
        &mut self.messages[..]
    }
}

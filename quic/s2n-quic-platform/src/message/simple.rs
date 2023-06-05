// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::message::Message as MessageTrait;
use alloc::vec::Vec;
use core::{alloc::Layout, cell::UnsafeCell, pin::Pin, ptr::NonNull};
use s2n_quic_core::{
    inet::{datagram, ExplicitCongestionNotification, SocketAddress},
    io::tx,
    path,
};

/// A simple message type that holds an address and payload
///
/// All other fields are not supported by the platform.
#[derive(Clone, Copy, Debug)]
pub struct Message {
    address: SocketAddress,
    payload_ptr: *mut u8,
    payload_len: usize,
}

impl Message {
    fn ecn(&self) -> ExplicitCongestionNotification {
        ExplicitCongestionNotification::default()
    }

    #[inline]
    pub fn remote_address(&self) -> &SocketAddress {
        &self.address
    }

    pub(crate) fn set_remote_address(&mut self, remote_address: &SocketAddress) {
        let remote_address = *remote_address;

        self.address = remote_address;
    }
}

pub type Handle = path::Tuple;

impl MessageTrait for Message {
    type Handle = Handle;

    const SUPPORTS_GSO: bool = false;
    const SUPPORTS_ECN: bool = false;
    const SUPPORTS_FLOW_LABELS: bool = false;

    #[inline]
    fn alloc(entries: u32, payload_len: u32, offset: usize) -> super::Storage {
        unsafe { alloc(entries, payload_len, offset) }
    }

    fn payload_len(&self) -> usize {
        self.payload_len
    }

    unsafe fn set_payload_len(&mut self, len: usize) {
        self.payload_len = len;
    }

    fn can_gso<M: tx::Message>(&self, _other: &mut M) -> bool {
        false
    }

    unsafe fn reset(&mut self, mtu: usize) {
        self.address = Default::default();
        self.set_payload_len(mtu)
    }

    fn payload_ptr_mut(&mut self) -> *mut u8 {
        self.payload_ptr
    }

    fn replicate_fields_from(&mut self, other: &Self) {
        debug_assert_eq!(self.payload_ptr, other.payload_ptr);
        self.address = other.address;
        self.payload_len = other.payload_len;
    }

    #[inline]
    fn validate_replication(source: &Self, dest: &Self) {
        assert_eq!(source.payload_ptr, dest.payload_ptr);
    }

    #[inline]
    fn rx_read(
        &mut self,
        local_address: &path::LocalAddress,
    ) -> Option<super::RxMessage<Self::Handle>> {
        let path = path::Tuple {
            remote_address: self.address.into(),
            local_address: *local_address,
        };
        let header = datagram::Header {
            path,
            ecn: self.ecn(),
        };
        let payload = self.payload_mut();

        let message = super::RxMessage {
            header,
            segment_size: payload.len(),
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

        let remote_address = message.path_handle().remote_address;

        self.set_remote_address(&remote_address);

        Ok(len)
    }
}

#[inline]
unsafe fn alloc(entries: u32, payload_len: u32, offset: usize) -> super::Storage {
    let (layout, entry_offset, payload_offset) = layout(entries, payload_len, offset);

    let ptr = alloc::alloc::alloc_zeroed(layout);

    let end_pointer = ptr.add(layout.size());

    let ptr = NonNull::new(ptr).expect("could not allocate socket message ring");

    {
        let mut entry_ptr = ptr.as_ptr().add(entry_offset) as *mut UnsafeCell<Message>;
        let mut payload_ptr = ptr.as_ptr().add(payload_offset) as *mut UnsafeCell<u8>;
        for _ in 0..entries {
            let entry = (*entry_ptr).get_mut();
            entry.payload_ptr = (*payload_ptr).get();
            entry.payload_len = payload_len as _;

            entry_ptr = entry_ptr.add(1);
            debug_assert!(end_pointer >= entry_ptr as *mut u8);
            payload_ptr = payload_ptr.add(payload_len as _);
            debug_assert!(end_pointer >= payload_ptr as *mut u8);
        }

        let primary = ptr.as_ptr().add(entry_offset) as *mut Message;
        let secondary = primary.add(entries as _);
        debug_assert!(end_pointer >= secondary.add(entries as _) as *mut u8);
        core::ptr::copy_nonoverlapping(primary, secondary, entries as _);
    }

    let slice = core::slice::from_raw_parts_mut(ptr.as_ptr() as *mut UnsafeCell<u8>, layout.size());
    Box::from_raw(slice).into()
}

fn layout(entries: u32, payload_len: u32, offset: usize) -> (Layout, usize, usize) {
    let cursor = Layout::array::<UnsafeCell<u8>>(offset).unwrap();
    let payloads =
        Layout::array::<UnsafeCell<u8>>(entries as usize * payload_len as usize).unwrap();
    let entries = Layout::array::<UnsafeCell<Message>>((entries * 2) as usize).unwrap();
    let (layout, entry_offset) = cursor.extend(entries).unwrap();
    let (layout, payload_offset) = layout.extend(payloads).unwrap();
    (layout, entry_offset, payload_offset)
}

pub struct Ring<Payloads> {
    messages: Vec<Message>,

    // this field holds references to allocated payloads, but is never read directly
    #[allow(dead_code)]
    payloads: Pin<Payloads>,

    mtu: usize,
}

/// Even though `Ring` contains raw pointers, it owns all of the data
/// and can be sent across threads safely.
#[allow(unknown_lints, clippy::non_send_fields_in_send_ty)]
unsafe impl<Payloads: Send> Send for Ring<Payloads> {}

impl<Payloads: crate::buffer::Buffer + Default> Default for Ring<Payloads> {
    fn default() -> Self {
        Self::new(Payloads::default(), 1)
    }
}

impl<Payloads: crate::buffer::Buffer> Ring<Payloads> {
    pub fn new(payloads: Payloads, _max_gso: usize) -> Self {
        let mtu = payloads.mtu();
        let capacity = payloads.len() / mtu;

        let mut payloads = Pin::new(payloads);

        // double message capacity to enable contiguous access
        let mut messages = Vec::with_capacity(capacity * 2);

        let mut buf = &mut payloads.as_mut()[..];

        for _ in 0..capacity {
            let (payload, remaining) = buf.split_at_mut(mtu);
            buf = remaining;

            let payload_ptr = payload.as_mut_ptr() as _;
            messages.push(Message {
                payload_ptr,
                payload_len: mtu,
                address: Default::default(),
            });
        }

        for index in 0..capacity {
            messages.push(messages[index]);
        }

        Self {
            messages,
            payloads,
            mtu,
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
        self.mtu
    }

    #[inline]
    fn max_gso(&self) -> usize {
        1
    }

    #[inline]
    fn disable_gso(&mut self) {
        panic!("GSO is not supported by simple messages");
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

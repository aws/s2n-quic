// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::message::Message as MessageTrait;
use alloc::vec::Vec;
use core::pin::Pin;
use s2n_quic_core::{
    inet::{datagram, ExplicitCongestionNotification, SocketAddress},
    io::{rx, tx},
    path::{self, Handle as _},
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

pub type Handle = path::Tuple;

impl MessageTrait for Message {
    type Handle = Handle;

    const SUPPORTS_GSO: bool = false;

    fn ecn(&self) -> ExplicitCongestionNotification {
        ExplicitCongestionNotification::default()
    }

    fn set_ecn(&mut self, _ecn: ExplicitCongestionNotification, _remote_address: &SocketAddress) {
        // the std UDP socket doesn't provide a method to set ECN
    }

    fn remote_address(&self) -> Option<SocketAddress> {
        Some(self.address)
    }

    fn set_remote_address(&mut self, remote_address: &SocketAddress) {
        let remote_address = *remote_address;

        self.address = remote_address;
    }

    fn path_handle(&self) -> Option<Self::Handle> {
        Some(Handle::from_remote_address(self.address.into()))
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

    fn payload_ptr(&self) -> *const u8 {
        self.payload_ptr as *const _
    }

    fn payload_ptr_mut(&mut self) -> *mut u8 {
        self.payload_ptr
    }

    fn replicate_fields_from(&mut self, other: &Self) {
        debug_assert_eq!(self.payload_ptr, other.payload_ptr);
        self.address = other.address;
        self.payload_len = other.payload_len;
    }
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

impl tx::Entry for Message {
    type Handle = Handle;

    fn set<M: tx::Message<Handle = Self::Handle>>(
        &mut self,
        mut message: M,
    ) -> Result<usize, tx::Error> {
        let payload = MessageTrait::payload_mut(self);

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

    #[inline]
    fn payload(&self) -> &[u8] {
        MessageTrait::payload(self)
    }

    #[inline]
    fn payload_mut(&mut self) -> &mut [u8] {
        MessageTrait::payload_mut(self)
    }
}

impl rx::Entry for Message {
    type Handle = Handle;

    #[inline]
    fn read(
        &mut self,
        local_address: &path::LocalAddress,
    ) -> Option<(datagram::Header<Self::Handle>, &mut [u8])> {
        let mut header = datagram::Header {
            path: self.path_handle()?,
            ecn: self.ecn(),
        };

        // set the correct local address
        header.path.local_address = *local_address;

        let payload = self.payload_mut();
        Some((header, payload))
    }
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::message::Message as MessageTrait;
use alloc::vec::Vec;
use s2n_quic_core::{
    inet::{ExplicitCongestionNotification, SocketAddress},
    io::{rx, tx},
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

impl MessageTrait for Message {
    fn ecn(&self) -> ExplicitCongestionNotification {
        ExplicitCongestionNotification::default()
    }

    fn set_ecn(&mut self, _ecn: ExplicitCongestionNotification) {
        // the std UDP socket doesn't provide a method to set ECN
    }

    fn remote_address(&self) -> Option<SocketAddress> {
        Some(self.address)
    }

    fn set_remote_address(&mut self, remote_address: &SocketAddress) {
        let remote_address = *remote_address;

        // macos doesn't like sending ipv4 addresses on ipv6 sockets
        #[cfg(all(target_os = "macos", feature = "ipv6"))]
        let remote_address = remote_address.to_ipv6_mapped().into();

        self.address = remote_address;
    }

    fn reset_remote_address(&mut self) {
        self.address = Default::default();
    }

    fn payload_len(&self) -> usize {
        self.payload_len as usize
    }

    unsafe fn set_payload_len(&mut self, len: usize) {
        self.payload_len = len;
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
    payloads: Payloads,
}

/// Even though `Ring` contains raw pointers, it owns all of the data
/// and can be sent across threads safely.
unsafe impl<Payloads: Send> Send for Ring<Payloads> {}

impl<Payloads: crate::buffer::Buffer + Default> Default for Ring<Payloads> {
    fn default() -> Self {
        Self::new(Payloads::default())
    }
}

impl<Payloads: crate::buffer::Buffer> Ring<Payloads> {
    pub fn new(mut payloads: Payloads) -> Self {
        let capacity = payloads.len();
        let mtu = payloads.mtu();

        // double message capacity to enable contiguous access
        let mut messages = Vec::with_capacity(capacity * 2);

        for index in 0..capacity {
            let payload_ptr = payloads[index].as_mut_ptr() as _;
            messages.push(Message {
                payload_ptr,
                payload_len: mtu,
                address: Default::default(),
            });
        }

        for index in 0..capacity {
            messages.push(messages[index]);
        }

        Self { payloads, messages }
    }
}

impl<Payloads: crate::buffer::Buffer> super::Ring for Ring<Payloads> {
    type Message = Message;

    fn len(&self) -> usize {
        self.payloads.len()
    }

    fn mtu(&self) -> usize {
        self.payloads.mtu()
    }

    fn as_slice(&self) -> &[Self::Message] {
        &self.messages[..]
    }

    fn as_mut_slice(&mut self) -> &mut [Self::Message] {
        &mut self.messages[..]
    }
}

impl tx::Entry for Message {
    fn set<M: tx::Message>(&mut self, mut message: M) -> Result<usize, tx::Error> {
        let payload = MessageTrait::payload_mut(self);

        let len = message.write_payload(payload);

        // don't send empty payloads
        if len == 0 {
            return Err(tx::Error::EmptyPayload);
        }

        unsafe {
            debug_assert!(len <= payload.len());
            let len = len.min(payload.len());
            self.set_payload_len(len);
        }
        self.set_remote_address(&message.remote_address());

        Ok(len)
    }

    fn payload(&self) -> &[u8] {
        MessageTrait::payload(self)
    }

    fn payload_mut(&mut self) -> &mut [u8] {
        MessageTrait::payload_mut(self)
    }
}

impl rx::Entry for Message {
    fn remote_address(&self) -> Option<SocketAddress> {
        MessageTrait::remote_address(self)
    }

    fn ecn(&self) -> ExplicitCongestionNotification {
        MessageTrait::ecn(self)
    }

    fn payload(&self) -> &[u8] {
        MessageTrait::payload(self)
    }

    fn payload_mut(&mut self) -> &mut [u8] {
        MessageTrait::payload_mut(self)
    }
}

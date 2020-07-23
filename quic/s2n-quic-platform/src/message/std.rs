use crate::message::Message as MessageTrait;
use alloc::vec::Vec;
use s2n_quic_core::inet::{ExplicitCongestionNotification, SocketAddress};

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
        self.address = *remote_address;
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

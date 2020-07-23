use crate::message::{msg::Ring as MsgRing, Message};
use alloc::vec::Vec;
use core::mem::zeroed;
use libc::{iovec, mmsghdr, sockaddr_in6};
use s2n_quic_core::inet::{ExplicitCongestionNotification, SocketAddress};

impl Message for mmsghdr {
    fn ecn(&self) -> ExplicitCongestionNotification {
        self.msg_hdr.ecn()
    }

    fn set_ecn(&mut self, ecn: ExplicitCongestionNotification) {
        self.msg_hdr.set_ecn(ecn)
    }

    fn remote_address(&self) -> Option<SocketAddress> {
        self.msg_hdr.remote_address()
    }

    fn set_remote_address(&mut self, remote_address: &SocketAddress) {
        self.msg_hdr.set_remote_address(remote_address)
    }

    fn reset_remote_address(&mut self) {
        self.msg_hdr.reset_remote_address()
    }

    fn payload_len(&self) -> usize {
        self.msg_len as usize
    }

    unsafe fn set_payload_len(&mut self, len: usize) {
        debug_assert!(len <= core::u32::MAX as usize);
        self.msg_len = len as _;
        self.msg_hdr.set_payload_len(len);
    }

    fn payload_ptr_mut(&mut self) -> *mut u8 {
        self.msg_hdr.payload_ptr_mut()
    }

    fn replicate_fields_from(&mut self, other: &Self) {
        self.msg_len = other.msg_len;
        self.msg_hdr.replicate_fields_from(&other.msg_hdr)
    }
}

pub struct Ring<Payloads> {
    messages: Vec<mmsghdr>,

    // this field holds references to allocated payloads, but is never read directly
    #[allow(dead_code)]
    payloads: Payloads,

    // this field holds references to allocated iovecs, but is never read directly
    #[allow(dead_code)]
    iovecs: Vec<iovec>,

    // this field holds references to allocated msg_names, but is never read directly
    #[allow(dead_code)]
    msg_names: Vec<sockaddr_in6>,
}

/// Even though `Ring` contains raw pointers, it owns all of the data
/// and can be sent across threads safely.
unsafe impl<Payloads: Send> Send for Ring<Payloads> {}

impl<Payloads: crate::buffer::Buffer> Ring<Payloads> {
    pub fn new(payloads: Payloads) -> Self {
        let MsgRing {
            mut messages,
            payloads,
            iovecs,
            msg_names,
        } = MsgRing::new(payloads);

        // convert msghdr into mmsghdr
        let messages = messages
            .drain(..)
            .map(|msg_hdr| unsafe {
                let mut mmsghdr = zeroed::<mmsghdr>();
                let payload_len = msg_hdr.payload_len();
                mmsghdr.msg_hdr = msg_hdr;
                mmsghdr.set_payload_len(payload_len);
                mmsghdr
            })
            .collect();

        Self {
            payloads,
            messages,
            iovecs,
            msg_names,
        }
    }
}

impl<Payloads: crate::buffer::Buffer> super::Ring for Ring<Payloads> {
    type Message = mmsghdr;

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

use crate::message::{msg::Ring as MsgRing, Message as MessageTrait};
use alloc::vec::Vec;
use core::{fmt, mem::zeroed};
use libc::{iovec, mmsghdr, sockaddr_in6};
use s2n_quic_core::{
    inet::{ExplicitCongestionNotification, SocketAddress},
    io::{rx, tx},
};

#[repr(transparent)]
pub struct Message(pub(crate) mmsghdr);

impl_message_delegate!(Message, 0);

impl fmt::Debug for Message {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("mmsghdr")
            .field("ecn", &self.ecn())
            .field("remote_address", &self.remote_address())
            .field("payload", &self.payload())
            .finish()
    }
}

impl MessageTrait for mmsghdr {
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

    fn payload_ptr(&self) -> *const u8 {
        self.msg_hdr.payload_ptr()
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
    messages: Vec<Message>,

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
                mmsghdr.msg_hdr = msg_hdr.0;
                mmsghdr.set_payload_len(payload_len);
                Message(mmsghdr)
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

        // TODO ecn

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

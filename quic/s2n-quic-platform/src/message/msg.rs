// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::message::Message as MessageTrait;
use alloc::vec::Vec;
use core::{
    fmt,
    mem::{size_of, zeroed},
};
use libc::{c_void, iovec, msghdr, sockaddr_in, sockaddr_in6, AF_INET, AF_INET6};
use s2n_quic_core::{
    inet::{ExplicitCongestionNotification, IpV4Address, SocketAddress, SocketAddressV4},
    io::{rx, tx},
};

#[cfg(feature = "ipv6")]
use s2n_quic_core::inet::{IpV6Address, SocketAddressV6};

#[repr(transparent)]
pub struct Message(pub(crate) msghdr);

impl_message_delegate!(Message, 0);

impl fmt::Debug for Message {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("msghdr")
            .field("ecn", &self.ecn())
            .field("remote_address", &self.remote_address())
            .field("payload", &self.payload())
            .finish()
    }
}

impl Message {
    fn new(
        iovec: *mut iovec,
        msg_name: *mut c_void,
        msg_namelen: usize,
        msg_control: *mut c_void,
        msg_controllen: usize,
    ) -> Self {
        let mut msghdr = unsafe { core::mem::zeroed::<msghdr>() };

        msghdr.msg_iov = iovec;
        msghdr.msg_iovlen = 1; // a single iovec is allocated per message

        msghdr.msg_name = msg_name;
        msghdr.msg_namelen = msg_namelen as _;

        msghdr.msg_control = msg_control;
        msghdr.msg_controllen = msg_controllen as _;

        Self(msghdr)
    }
}

impl MessageTrait for msghdr {
    fn ecn(&self) -> ExplicitCongestionNotification {
        // TODO support ecn
        ExplicitCongestionNotification::default()
    }

    fn set_ecn(&mut self, _ecn: ExplicitCongestionNotification) {
        // TODO support ecn
    }

    fn remote_address(&self) -> Option<SocketAddress> {
        debug_assert!(!self.msg_name.is_null());
        match self.msg_namelen {
            size if size == size_of::<sockaddr_in>() as _ => {
                let sockaddr: &sockaddr_in = unsafe { &*(self.msg_name as *const _) };
                let port = sockaddr.sin_port.to_be();
                let addr: IpV4Address = sockaddr.sin_addr.s_addr.to_ne_bytes().into();
                Some(SocketAddressV4::new(addr, port).into())
            }
            #[cfg(feature = "ipv6")]
            size if size == size_of::<sockaddr_in6>() as _ => {
                let sockaddr: &sockaddr_in6 = unsafe { &*(self.msg_name as *const _) };
                let port = sockaddr.sin6_port.to_be();
                let addr: IpV6Address = sockaddr.sin6_addr.s6_addr.into();
                Some(SocketAddressV6::new(addr, port).into())
            }
            _ => None,
        }
    }

    fn set_remote_address(&mut self, remote_address: &SocketAddress) {
        debug_assert!(!self.msg_name.is_null());

        // macos doesn't like sending ipv4 addresses on ipv6 sockets
        #[cfg(all(target_os = "macos", feature = "ipv6"))]
        let remote_address = remote_address.to_ipv6_mapped().into();

        match remote_address {
            SocketAddress::IpV4(addr) => {
                let sockaddr: &mut sockaddr_in = unsafe { &mut *(self.msg_name as *mut _) };
                sockaddr.sin_family = AF_INET as _;
                sockaddr.sin_port = addr.port().to_be();
                sockaddr.sin_addr.s_addr = u32::from_ne_bytes((*addr.ip()).into());
                self.msg_namelen = size_of::<sockaddr_in>() as _;
            }
            SocketAddress::IpV6(addr) => {
                let sockaddr: &mut sockaddr_in6 = unsafe { &mut *(self.msg_name as *mut _) };
                sockaddr.sin6_family = AF_INET6 as _;
                sockaddr.sin6_port = addr.port().to_be();
                sockaddr.sin6_addr.s6_addr = (*addr.ip()).into();
                self.msg_namelen = size_of::<sockaddr_in6>() as _;
            }
        }
    }

    fn reset_remote_address(&mut self) {
        self.msg_namelen = size_of::<sockaddr_in6>() as _;
    }

    fn payload_len(&self) -> usize {
        debug_assert!(!self.msg_iov.is_null());
        unsafe { (*self.msg_iov).iov_len }
    }

    unsafe fn set_payload_len(&mut self, payload_len: usize) {
        debug_assert!(!self.msg_iov.is_null());
        (*self.msg_iov).iov_len = payload_len;
    }

    fn replicate_fields_from(&mut self, other: &Self) {
        debug_assert_eq!(
            self.msg_name, other.msg_name,
            "msg_name needs to point to the same data"
        );
        debug_assert_eq!(self.msg_iov, other.msg_iov);
        debug_assert_eq!(self.msg_iovlen, other.msg_iovlen);
        self.msg_namelen = other.msg_namelen;
    }

    fn payload_ptr(&self) -> *const u8 {
        unsafe {
            let iovec = &*self.msg_iov;
            iovec.iov_base as *const _
        }
    }

    fn payload_ptr_mut(&mut self) -> *mut u8 {
        unsafe {
            let iovec = &mut *self.msg_iov;
            iovec.iov_base as *mut _
        }
    }
}

pub struct Ring<Payloads> {
    pub(crate) messages: Vec<Message>,

    // this field holds references to allocated payloads, but is never read directly
    #[allow(dead_code)]
    pub(crate) payloads: Payloads,

    // this field holds references to allocated iovecs, but is never read directly
    #[allow(dead_code)]
    pub(crate) iovecs: Vec<iovec>,

    // this field holds references to allocated msg_names, but is never read directly
    #[allow(dead_code)]
    pub(crate) msg_names: Vec<sockaddr_in6>,
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

        let mut iovecs = Vec::with_capacity(capacity);
        let mut msg_names = Vec::with_capacity(capacity);

        // double message capacity to enable contiguous access
        let mut messages = Vec::with_capacity(capacity * 2);

        for index in 0..capacity {
            let mut iovec = unsafe { zeroed::<iovec>() };
            iovec.iov_base = payloads[index].as_mut_ptr() as _;
            iovec.iov_len = mtu;
            iovecs.push(iovec);

            msg_names.push(unsafe { zeroed() });

            let msg = Message::new(
                (&mut iovecs[index]) as *mut _,
                (&mut msg_names[index]) as *mut _ as *mut _,
                size_of::<sockaddr_in6>(),
                core::ptr::null_mut(),
                0,
            );

            messages.push(msg);
        }

        for index in 0..capacity {
            messages.push(Message(messages[index].0));
        }

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

#[cfg(test)]
mod tests {
    use super::*;
    use bolero::check;

    #[cfg(feature = "ipv6")]
    use s2n_quic_core::inet::SocketAddress;
    #[cfg(not(feature = "ipv6"))]
    use s2n_quic_core::inet::SocketAddressV4 as SocketAddress;

    #[test]
    fn address_inverse_pair_test() {
        use core::mem::zeroed;

        let mut msghdr = unsafe { zeroed::<msghdr>() };

        let mut msgname = unsafe { zeroed::<sockaddr_in6>() };
        msghdr.msg_name = &mut msgname as *mut _ as *mut _;
        msghdr.msg_namelen = size_of::<sockaddr_in6>() as _;

        let mut message = Message(msghdr);

        check!()
            .with_type::<SocketAddress>()
            .cloned()
            .for_each(|addr| {
                #[cfg(not(feature = "ipv6"))]
                let addr = addr.into();
                message.reset_remote_address();
                message.set_remote_address(&addr);

                #[cfg(all(target_os = "macos", feature = "ipv6"))]
                let addr = addr.to_ipv6_mapped().into();

                assert_eq!(message.remote_address(), Some(addr));
            });
    }
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    features,
    message::{cmsg, Message as MessageTrait},
};
use alloc::vec::Vec;
use core::{
    fmt,
    mem::{size_of, zeroed},
    pin::Pin,
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

impl_message_delegate!(Message, 0, msghdr);

impl fmt::Debug for Message {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("msghdr")
            .field("ecn", &self.ecn())
            .field("remote_address", &self.remote_address())
            .field("payload", &self.payload())
            .finish()
    }
}

/// The maximum number of bytes allocated for cmsg data
///
/// This should be enough for UDP_SEGMENT + IP_TOS + IP_PKTINFO. It may need to be increased
/// to allow for future control messages.
const MAX_CMSG_LEN: usize = 128;

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
    const SUPPORTS_GSO: bool = true;

    #[inline]
    fn ecn(&self) -> ExplicitCongestionNotification {
        let ancilliary_data = cmsg::decode(&self);
        ancilliary_data.ecn
    }

    #[inline]
    fn set_ecn(&mut self, ecn: ExplicitCongestionNotification, remote_address: &SocketAddress) {
        let cmsg = unsafe {
            // Safety: the msg_control buffer should always be allocated to MAX_CMSG_LEN
            core::slice::from_raw_parts_mut(self.msg_control as *mut u8, MAX_CMSG_LEN)
        };
        let remaining = &mut cmsg[(self.msg_controllen as usize)..];
        let ecn = ecn as libc::c_int;

        let len = match remote_address {
            SocketAddress::IpV4(_) => {
                // FreeBSD uses an unsigned_char for IP_TOS
                // see https://svnweb.freebsd.org/base/stable/8/sys/netinet/ip_input.c?view=markup&pathrev=247944#l1716
                #[cfg(target_os = "freebsd")]
                let ecn = ecn as libc::c_uchar;

                cmsg::encode(remaining, libc::IPPROTO_IP, libc::IP_TOS, ecn)
            }
            SocketAddress::IpV6(_) => {
                cmsg::encode(remaining, libc::IPPROTO_IPV6, libc::IPV6_TCLASS, ecn)
            }
        };

        // add the values as a usize to make sure we work cross-platform
        self.msg_controllen = (len + self.msg_controllen as usize) as _;
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
    fn set_segment_size(&mut self, size: usize) {
        let cmsg = unsafe {
            // Safety: the msg_control buffer should always be allocated to MAX_CMSG_LEN
            core::slice::from_raw_parts_mut(self.msg_control as *mut u8, MAX_CMSG_LEN)
        };
        let remaining = &mut cmsg[(self.msg_controllen as usize)..];
        let len = features::Gso::set(remaining, size);
        // add the values as a usize to make sure we work cross-platform
        self.msg_controllen = (len + self.msg_controllen as usize) as _;
    }

    #[inline]
    unsafe fn reset(&mut self, mtu: usize) {
        // reset the payload
        self.set_payload_len(mtu);

        // reset the address
        self.msg_namelen = size_of::<sockaddr_in6>() as _;

        if cfg!(debug_assertions) && self.msg_controllen == 0 {
            // make sure nothing was written to the control message if it was set to 0
            assert!(
                core::slice::from_raw_parts_mut(self.msg_control as *mut u8, MAX_CMSG_LEN)
                    .iter()
                    .all(|v| *v == 0)
            )
        }

        // reset the control messages
        let cmsg =
            core::slice::from_raw_parts_mut(self.msg_control as *mut u8, self.msg_controllen as _);

        for byte in cmsg.iter_mut() {
            *byte = 0;
        }
        self.msg_controllen = 0;
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
    fn payload_ptr(&self) -> *const u8 {
        unsafe {
            let iovec = &*self.msg_iov;
            iovec.iov_base as *const _
        }
    }

    #[inline]
    fn payload_ptr_mut(&mut self) -> *mut u8 {
        unsafe {
            let iovec = &mut *self.msg_iov;
            iovec.iov_base as *mut _
        }
    }
}

pub struct Ring<Payloads> {
    pub(crate) messages: Vec<Message>,
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
    max_gso: usize,
}

impl<Payloads: crate::buffer::Buffer> Storage<Payloads> {
    #[inline]
    pub fn mtu(&self) -> usize {
        self.mtu
    }

    #[inline]
    pub fn max_gso(&self) -> usize {
        self.max_gso
    }

    #[inline]
    pub fn disable_gso(&mut self) {
        // TODO recompute message offsets
        // https://github.com/awslabs/s2n-quic/issues/762
        self.max_gso = 1;
    }
}

/// Even though `Ring` contains raw pointers, it owns all of the data
/// and can be sent across threads safely.
unsafe impl<Payloads: Send> Send for Ring<Payloads> {}

impl<Payloads: crate::buffer::Buffer + Default> Default for Ring<Payloads> {
    fn default() -> Self {
        Self::new(
            Payloads::default(),
            crate::features::get().gso.default_max_segments(),
        )
    }
}

impl<Payloads: crate::buffer::Buffer> Ring<Payloads> {
    pub fn new(payloads: Payloads, max_gso: usize) -> Self {
        assert!(max_gso <= crate::features::get().gso.max_segments());

        let mtu = payloads.mtu();
        let capacity = payloads.len() / mtu / max_gso;

        let mut payloads = Pin::new(payloads);
        let mut iovecs = Pin::new(vec![unsafe { zeroed() }; capacity].into_boxed_slice());
        let mut msg_names = Pin::new(vec![unsafe { zeroed() }; capacity].into_boxed_slice());
        let mut cmsgs = Pin::new(vec![0u8; capacity * MAX_CMSG_LEN].into_boxed_slice());

        // double message capacity to enable contiguous access
        let mut messages = Vec::with_capacity(capacity * 2);

        let mut payload_buf = &mut payloads.as_mut()[..];
        let mut cmsg_buf = &mut cmsgs.as_mut()[..];

        for index in 0..capacity {
            let (payload, remaining) = payload_buf.split_at_mut(mtu * max_gso);
            payload_buf = remaining;
            let (cmsg, remaining) = cmsg_buf.split_at_mut(MAX_CMSG_LEN);
            cmsg_buf = remaining;

            let mut iovec = unsafe { zeroed::<iovec>() };
            iovec.iov_base = payload.as_mut_ptr() as _;
            iovec.iov_len = mtu;
            iovecs[index] = iovec;

            let msg = Message::new(
                (&mut iovecs[index]) as *mut _,
                (&mut msg_names[index]) as *mut _ as *mut _,
                size_of::<sockaddr_in6>(),
                cmsg as *mut _ as *mut _,
                0,
            );

            messages.push(msg);
        }

        for index in 0..capacity {
            messages.push(Message(messages[index].0));
        }

        Self {
            messages,
            storage: Storage {
                payloads,
                iovecs,
                msg_names,
                cmsgs,
                mtu,
                max_gso,
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
    #[inline]
    fn remote_address(&self) -> Option<SocketAddress> {
        MessageTrait::remote_address(self)
    }

    #[inline]
    fn ecn(&self) -> ExplicitCongestionNotification {
        MessageTrait::ecn(self)
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

        let mut iovec = unsafe { zeroed::<iovec>() };
        msghdr.msg_iov = &mut iovec;

        let mut message = Message(msghdr);

        check!()
            .with_type::<SocketAddress>()
            .cloned()
            .for_each(|addr| {
                #[cfg(not(feature = "ipv6"))]
                let addr = addr.into();
                unsafe {
                    message.reset(0);
                }
                message.set_remote_address(&addr);

                #[cfg(all(target_os = "macos", feature = "ipv6"))]
                let addr = addr.to_ipv6_mapped().into();

                assert_eq!(message.remote_address(), Some(addr));
            });
    }
}

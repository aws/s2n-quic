use crate::message::Message;
use alloc::vec::Vec;
use core::mem::{size_of, zeroed};
use libc::{c_void, iovec, msghdr, sockaddr_in, sockaddr_in6, AF_INET, AF_INET6};
use s2n_quic_core::inet::{
    ExplicitCongestionNotification, IPv4Address, IPv6Address, SocketAddress, SocketAddressV4,
    SocketAddressV6,
};

impl Message for msghdr {
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
                let addr: IPv4Address = sockaddr.sin_addr.s_addr.to_ne_bytes().into();
                Some(SocketAddressV4::new(addr, port).into())
            }
            size if size == size_of::<sockaddr_in6>() as _ => {
                let sockaddr: &sockaddr_in6 = unsafe { &*(self.msg_name as *const _) };
                let port = sockaddr.sin6_port.to_be();
                let addr: IPv6Address = sockaddr.sin6_addr.s6_addr.into();
                Some(SocketAddressV6::new(addr, port).into())
            }
            _ => None,
        }
    }

    fn set_remote_address(&mut self, remote_address: &SocketAddress) {
        debug_assert!(!self.msg_name.is_null());
        match remote_address {
            SocketAddress::IPv4(addr) => {
                let sockaddr: &mut sockaddr_in = unsafe { &mut *(self.msg_name as *mut _) };
                sockaddr.sin_family = AF_INET as _;
                sockaddr.sin_port = addr.port().to_be();
                sockaddr.sin_addr.s_addr = u32::from_ne_bytes((*addr.ip()).into());
                self.msg_namelen = size_of::<sockaddr_in>() as _;
            }
            SocketAddress::IPv6(addr) => {
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

    fn payload_ptr_mut(&mut self) -> *mut u8 {
        unsafe {
            let iovec = &*self.msg_iov;
            iovec.iov_base as *mut _
        }
    }
}

pub struct Ring<Payloads> {
    pub(crate) messages: Vec<msghdr>,

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

            let msg = new_msg(
                (&mut iovecs[index]) as *mut _,
                (&mut msg_names[index]) as *mut _ as *mut _,
                size_of::<sockaddr_in6>(),
                core::ptr::null_mut(),
                0,
            );

            messages.push(msg);
        }

        for index in 0..capacity {
            messages.push(messages[index]);
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
    type Message = msghdr;

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

fn new_msg(
    iovec: *mut iovec,
    msg_name: *mut c_void,
    msg_namelen: usize,
    msg_control: *mut c_void,
    msg_controllen: usize,
) -> msghdr {
    let mut msghdr = unsafe { core::mem::zeroed::<msghdr>() };

    msghdr.msg_iov = iovec;
    msghdr.msg_iovlen = 1; // a single iovec is allocated per message

    msghdr.msg_name = msg_name;
    msghdr.msg_namelen = msg_namelen as _;

    msghdr.msg_control = msg_control;
    msghdr.msg_controllen = msg_controllen as _;

    msghdr
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn address_inverse_pair_test() {
        use core::mem::zeroed;

        let mut msghdr = unsafe { zeroed::<msghdr>() };

        let mut msgname = unsafe { zeroed::<sockaddr_in6>() };
        msghdr.msg_name = &mut msgname as *mut _ as *mut _;
        msghdr.msg_namelen = size_of::<sockaddr_in6>() as _;

        let tests = ["192.168.1.2:1337", "[2001:db8:85a3::8a2e:370:7334]:1337"];

        for addr in tests.iter() {
            let addr: std::net::SocketAddr = addr.parse().unwrap();
            let addr: SocketAddress = addr.into();

            msghdr.set_remote_address(&addr);
            assert_eq!(msghdr.remote_address(), Some(addr));

            msghdr.reset_remote_address();
        }
    }
}

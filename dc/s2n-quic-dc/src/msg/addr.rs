// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::{fmt, mem::size_of};
use libc::{msghdr, sockaddr_in, sockaddr_in6, AF_INET, AF_INET6};
use s2n_quic_core::{
    assume,
    inet::{self, SocketAddress},
};

const SIZE: usize = {
    let v4 = size_of::<sockaddr_in>();
    let v6 = size_of::<sockaddr_in6>();
    if v4 > v6 {
        v4
    } else {
        v6
    }
};

#[repr(align(8))]
pub struct Addr {
    msg_name: [u8; SIZE],
    msg_namelen: u8,
}

impl fmt::Debug for Addr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.get().fmt(f)
    }
}

impl fmt::Display for Addr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.get().fmt(f)
    }
}

impl Default for Addr {
    #[inline]
    fn default() -> Self {
        Self::new(SocketAddress::default())
    }
}

impl Addr {
    #[inline]
    pub fn new(value: SocketAddress) -> Self {
        let mut v = Self {
            msg_name: Default::default(),
            msg_namelen: Default::default(),
        };
        v.set(value);
        v
    }

    #[inline]
    pub fn get(&self) -> SocketAddress {
        match self.msg_namelen as usize {
            size if size == size_of::<sockaddr_in>() => {
                let sockaddr: &sockaddr_in = unsafe { &*(self.msg_name.as_ptr() as *const _) };
                let port = sockaddr.sin_port.to_be();
                let addr: inet::IpV4Address = sockaddr.sin_addr.s_addr.to_ne_bytes().into();
                inet::SocketAddressV4::new(addr, port).into()
            }
            size if size == size_of::<sockaddr_in6>() => {
                let sockaddr: &sockaddr_in6 = unsafe { &*(self.msg_name.as_ptr() as *const _) };
                let port = sockaddr.sin6_port.to_be();
                let addr: inet::IpV6Address = sockaddr.sin6_addr.s6_addr.into();
                inet::SocketAddressV6::new(addr, port).into()
            }
            _ => unsafe {
                assume!(false, "invalid remote address");
            },
        }
    }

    #[inline]
    pub fn set(&mut self, remote_address: SocketAddress) {
        match remote_address {
            SocketAddress::IpV4(addr) => {
                let sockaddr: &mut sockaddr_in =
                    unsafe { &mut *(self.msg_name.as_mut_ptr() as *mut _) };
                sockaddr.sin_family = AF_INET as _;
                sockaddr.sin_port = addr.port().to_be();
                sockaddr.sin_addr.s_addr = u32::from_ne_bytes((*addr.ip()).into());
                self.msg_namelen = size_of::<sockaddr_in>() as _;
            }
            SocketAddress::IpV6(addr) => {
                let sockaddr: &mut sockaddr_in6 =
                    unsafe { &mut *(self.msg_name.as_mut_ptr() as *mut _) };
                sockaddr.sin6_family = AF_INET6 as _;
                sockaddr.sin6_port = addr.port().to_be();
                sockaddr.sin6_addr.s6_addr = (*addr.ip()).into();
                self.msg_namelen = size_of::<sockaddr_in6>() as _;
            }
        }
    }

    #[inline]
    pub fn set_port(&mut self, port: u16) {
        match self.msg_namelen as usize {
            size if size == size_of::<sockaddr_in>() => {
                let sockaddr: &mut sockaddr_in =
                    unsafe { &mut *(self.msg_name.as_mut_ptr() as *mut _) };
                sockaddr.sin_port = port.to_be();
            }
            size if size == size_of::<sockaddr_in6>() => {
                let sockaddr: &mut sockaddr_in6 =
                    unsafe { &mut *(self.msg_name.as_mut_ptr() as *mut _) };
                sockaddr.sin6_port = port.to_be();
            }
            _ => unsafe {
                assume!(false, "invalid remote address");
            },
        }
    }

    #[inline]
    pub fn send_with_msg(&mut self, msg: &mut msghdr) {
        msg.msg_name = self.msg_name.as_mut_ptr() as *mut _;
        msg.msg_namelen = self.msg_namelen as _;
    }

    #[inline]
    pub fn recv_with_msg(&mut self, msg: &mut msghdr) {
        msg.msg_name = self.msg_name.as_mut_ptr() as *mut _;
        // use the max size, in case the length changes
        msg.msg_namelen = self.msg_name.len() as _;
    }

    #[inline]
    pub fn update_with_msg(&mut self, msg: &msghdr) {
        debug_assert_eq!(self.msg_name.as_ptr(), msg.msg_name as *const u8);
        match msg.msg_namelen as usize {
            size if size == size_of::<sockaddr_in>() => {
                self.msg_namelen = size as _;
            }
            size if size == size_of::<sockaddr_in6>() => {
                self.msg_namelen = size as _;
            }
            _ => {
                unreachable!("invalid remote address")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bolero::check;

    #[test]
    fn set_port_test() {
        check!().with_type().cloned().for_each(|(addr, port)| {
            let mut addr = Addr::new(addr);
            addr.set_port(port);
            assert_eq!(addr.get().port(), port);
        });
    }
}

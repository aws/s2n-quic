// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;

pub trait Ext: cmsg::Encoder {
    fn header(&self) -> Option<(datagram::Header<Handle>, datagram::AncillaryData)>;
    fn set_ecn(&mut self, ecn: ExplicitCongestionNotification, remote_address: &SocketAddress);
    fn remote_address(&self) -> Option<SocketAddress>;
    fn set_remote_address(&mut self, remote_address: &SocketAddress);
}

impl Ext for msghdr {
    #[inline]
    fn header(&self) -> Option<(datagram::Header<Handle>, datagram::AncillaryData)> {
        let addr = self.remote_address()?;
        let mut path = Handle::from_remote_address(addr.into());

        let ancillary_data = cmsg::decode(self);
        let ecn = ancillary_data.ecn;

        path.with_ancillary_data(ancillary_data);

        let header = datagram::Header { path, ecn };

        Some((header, ancillary_data))
    }

    #[inline]
    fn set_ecn(&mut self, ecn: ExplicitCongestionNotification, remote_address: &SocketAddress) {
        if ecn == ExplicitCongestionNotification::NotEct {
            return;
        }

        let ecn = ecn as libc::c_int;

        // the remote address needs to be unmapped in order to set the appropriate cmsg
        match remote_address.unmap() {
            SocketAddress::IpV4(_) => {
                // FreeBSD uses an unsigned_char for IP_TOS
                // see https://svnweb.freebsd.org/base/stable/8/sys/netinet/ip_input.c?view=markup&pathrev=247944#l1716
                #[cfg(target_os = "freebsd")]
                let ecn = ecn as libc::c_uchar;

                self.encode_cmsg(libc::IPPROTO_IP, libc::IP_TOS, ecn)
            }
            SocketAddress::IpV6(_) => self.encode_cmsg(libc::IPPROTO_IPV6, libc::IPV6_TCLASS, ecn),
        };
    }

    #[inline]
    fn remote_address(&self) -> Option<SocketAddress> {
        debug_assert!(!self.msg_name.is_null());
        match self.msg_namelen as usize {
            size if size == size_of::<sockaddr_in>() => {
                let sockaddr: &sockaddr_in = unsafe { &*(self.msg_name as *const _) };
                let port = sockaddr.sin_port.to_be();
                let addr: IpV4Address = sockaddr.sin_addr.s_addr.to_ne_bytes().into();
                Some(SocketAddressV4::new(addr, port).into())
            }
            size if size == size_of::<sockaddr_in6>() => {
                let sockaddr: &sockaddr_in6 = unsafe { &*(self.msg_name as *const _) };
                let port = sockaddr.sin6_port.to_be();
                let addr: IpV6Address = sockaddr.sin6_addr.s6_addr.into();
                Some(SocketAddressV6::new(addr, port).into())
            }
            _ => None,
        }
    }

    #[inline]
    fn set_remote_address(&mut self, remote_address: &SocketAddress) {
        debug_assert!(!self.msg_name.is_null());

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
}

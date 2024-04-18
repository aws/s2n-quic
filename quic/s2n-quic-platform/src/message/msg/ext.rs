// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;

pub trait Ext {
    type Encoder<'a>: cmsg::Encoder
    where
        Self: 'a;

    fn header(&self) -> Option<(datagram::Header<Handle>, datagram::AncillaryData)>;
    fn cmsg_encoder(&mut self) -> Self::Encoder<'_>;
    fn remote_address(&self) -> Option<SocketAddress>;
    fn set_remote_address(&mut self, remote_address: &SocketAddress);
}

impl Ext for msghdr {
    type Encoder<'a> = MsghdrEncoder<'a>;

    #[inline]
    fn header(&self) -> Option<(datagram::Header<Handle>, datagram::AncillaryData)> {
        let addr = self.remote_address()?;
        let mut path = Handle::from_remote_address(addr.into());

        let ancillary_data = unsafe { cmsg::decode::Iter::from_msghdr(self) }.collect();
        let ecn = ancillary_data.ecn;

        path.with_ancillary_data(ancillary_data);

        let header = datagram::Header { path, ecn };

        Some((header, ancillary_data))
    }

    #[inline]
    fn cmsg_encoder(&mut self) -> Self::Encoder<'_> {
        MsghdrEncoder { msghdr: self }
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

pub struct MsghdrEncoder<'a> {
    msghdr: &'a mut msghdr,
}

impl<'a> Encoder for MsghdrEncoder<'a> {
    #[inline]
    fn encode_cmsg<T: Copy + ?Sized>(
        &mut self,
        level: libc::c_int,
        ty: libc::c_int,
        value: T,
    ) -> Result<usize, cmsg::encode::Error> {
        let storage =
            unsafe { &mut *(self.msghdr.msg_control as *mut cmsg::Storage<{ cmsg::MAX_LEN }>) };

        let mut encoder = storage.encoder();
        encoder.seek(self.msghdr.msg_controllen as _);

        let msg_len = encoder.encode_cmsg(level, ty, value)?;

        // update the cursor
        self.msghdr.msg_controllen = encoder.len() as _;

        Ok(msg_len)
    }
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::message::{cmsg, cmsg::Encoder, Message as MessageTrait};
use alloc::vec::Vec;
use core::{
    fmt,
    mem::{size_of, zeroed},
    pin::Pin,
};
use libc::{c_void, iovec, msghdr, sockaddr_in, sockaddr_in6, AF_INET, AF_INET6};
use s2n_quic_core::{
    inet::{
        datagram, AncillaryData, ExplicitCongestionNotification, IpV4Address, IpV6Address,
        SocketAddress, SocketAddressV4, SocketAddressV6,
    },
    io::{rx, tx},
    path::{self, Handle as _, LocalAddress, RemoteAddress},
};

#[cfg(any(test, feature = "generator"))]
use bolero_generator::*;

#[repr(transparent)]
pub struct Message(pub(crate) msghdr);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(any(test, feature = "generator"), derive(TypeGenerator))]
pub struct Handle {
    pub remote_address: RemoteAddress,
    pub local_address: LocalAddress,
}

impl Handle {
    #[inline]
    fn with_ancillary_data(&mut self, ancillary_data: AncillaryData) {
        self.local_address = ancillary_data.local_address;
    }

    #[inline]
    pub(crate) fn update_msg_hdr(self, msghdr: &mut msghdr) {
        // when sending a packet, we start out with no cmsg items
        msghdr.msg_controllen = 0;

        msghdr.set_remote_address(&self.remote_address.0);

        #[cfg(s2n_quic_platform_pktinfo)]
        match self.local_address.0 {
            SocketAddress::IpV4(addr) => {
                use s2n_quic_core::inet::Unspecified;

                let ip = addr.ip();

                if ip.is_unspecified() {
                    return;
                }

                let mut pkt_info = unsafe { core::mem::zeroed::<libc::in_pktinfo>() };
                pkt_info.ipi_spec_dst.s_addr = u32::from_ne_bytes((*ip).into());

                msghdr.encode_cmsg(libc::IPPROTO_IP, libc::IP_PKTINFO, pkt_info);
            }
            SocketAddress::IpV6(addr) => {
                use s2n_quic_core::inet::Unspecified;

                let ip = addr.ip();

                if ip.is_unspecified() {
                    return;
                }

                let mut pkt_info = unsafe { core::mem::zeroed::<libc::in6_pktinfo>() };

                pkt_info.ipi6_addr.s6_addr = (*ip).into();

                msghdr.encode_cmsg(libc::IPPROTO_IPV6, libc::IPV6_PKTINFO, pkt_info);
            }
        }
    }
}

impl path::Handle for Handle {
    #[inline]
    fn from_remote_address(remote_address: RemoteAddress) -> Self {
        Self {
            remote_address,
            local_address: SocketAddressV4::UNSPECIFIED.into(),
        }
    }

    #[inline]
    fn remote_address(&self) -> RemoteAddress {
        self.remote_address
    }

    #[inline]
    fn local_address(&self) -> LocalAddress {
        self.local_address
    }

    #[inline]
    fn eq(&self, other: &Self) -> bool {
        let mut eq = true;

        // only compare local addresses if the OS returns them
        if cfg!(s2n_quic_platform_pktinfo) {
            eq &= self.local_address.eq(&other.local_address);
        }

        eq && path::Handle::eq(&self.remote_address, &other.remote_address)
    }

    #[inline]
    fn strict_eq(&self, other: &Self) -> bool {
        PartialEq::eq(self, other)
    }

    #[inline]
    fn maybe_update(&mut self, other: &Self) {
        // once we discover our path, update the address local address
        if self.local_address.port() == 0 {
            self.local_address = other.local_address;
        }
    }
}

impl_message_delegate!(Message, 0, msghdr);

impl fmt::Debug for Message {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let alt = f.alternate();
        let mut s = f.debug_struct("msghdr");

        s.field("remote_address", &self.remote_address())
            .field("anciliary_data", &cmsg::decode(&self.0));

        if alt {
            s.field("payload", &self.payload());
        } else {
            s.field("payload_len", &self.payload_len());
        }

        s.finish()
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

    #[inline]
    pub(crate) fn header(msghdr: &msghdr) -> Option<datagram::Header<Handle>> {
        let addr = msghdr.remote_address()?;
        let mut path = Handle::from_remote_address(addr.into());

        let ancillary_data = cmsg::decode(msghdr);
        let ecn = ancillary_data.ecn;

        path.with_ancillary_data(ancillary_data);

        Some(datagram::Header { path, ecn })
    }
}

impl MessageTrait for msghdr {
    type Handle = Handle;

    const SUPPORTS_GSO: bool = cfg!(s2n_quic_platform_gso);

    #[inline]
    fn ecn(&self) -> ExplicitCongestionNotification {
        let ancillary_data = cmsg::decode(self);
        ancillary_data.ecn
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

    #[inline]
    fn path_handle(&self) -> Option<Self::Handle> {
        let header = Message::header(self)?;
        Some(header.path)
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
    fn can_gso<M: tx::Message<Handle = Self::Handle>>(&self, other: &mut M) -> bool {
        if let Some(header) = Message::header(self) {
            let mut other_handle = *other.path_handle();

            // when reading the header back from the msghdr, we don't know the port
            // so set the other port to 0 as well.
            other_handle.local_address.set_port(0);

            // check the path handles match
            header.path.strict_eq(&other_handle) &&
                // check the ECN markings match
                header.ecn == other.ecn()
        } else {
            false
        }
    }

    #[cfg(s2n_quic_platform_gso)]
    #[inline]
    fn set_segment_size(&mut self, size: usize) {
        type SegmentType = u16;
        self.encode_cmsg(libc::SOL_UDP, libc::UDP_SEGMENT, size as SegmentType);
    }

    #[inline]
    unsafe fn reset(&mut self, mtu: usize) {
        // reset the payload
        self.set_payload_len(mtu);

        // reset the address
        self.set_remote_address(&SocketAddress::IpV6(Default::default()));

        if cfg!(debug_assertions) && self.msg_controllen == 0 {
            // make sure nothing was written to the control message if it was set to 0
            assert!(
                core::slice::from_raw_parts_mut(self.msg_control as *mut u8, cmsg::MAX_LEN)
                    .iter()
                    .all(|v| *v == 0)
            )
        }

        // reset the control messages if it isn't set to the default value

        // some platforms encode lengths as `u32` so we cast everything to be safe
        #[allow(clippy::unnecessary_cast)]
        let msg_controllen = self.msg_controllen as usize;

        if msg_controllen != cmsg::MAX_LEN {
            let cmsg = core::slice::from_raw_parts_mut(self.msg_control as *mut u8, msg_controllen);

            for byte in cmsg.iter_mut() {
                *byte = 0;
            }
        }

        self.msg_controllen = cmsg::MAX_LEN as _;
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
        // https://github.com/aws/s2n-quic/issues/762
        self.max_gso = 1;
    }
}

/// Even though `Ring` contains raw pointers, it owns all of the data
/// and can be sent across threads safely.
#[allow(unknown_lints, clippy::non_send_fields_in_send_ty)]
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
        let mut cmsgs = Pin::new(vec![0u8; capacity * cmsg::MAX_LEN].into_boxed_slice());

        // double message capacity to enable contiguous access
        let mut messages = Vec::with_capacity(capacity * 2);

        let mut payload_buf = &mut payloads.as_mut()[..];
        let mut cmsg_buf = &mut cmsgs.as_mut()[..];

        for index in 0..capacity {
            let (payload, remaining) = payload_buf.split_at_mut(mtu * max_gso);
            payload_buf = remaining;
            let (cmsg, remaining) = cmsg_buf.split_at_mut(cmsg::MAX_LEN);
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
                cmsg::MAX_LEN,
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
    type Handle = Handle;

    #[inline]
    fn set<M: tx::Message<Handle = Self::Handle>>(
        &mut self,
        mut message: M,
    ) -> Result<usize, tx::Error> {
        let payload = MessageTrait::payload_mut(self);

        let len = message.write_payload(tx::PayloadBuffer::new(payload), 0)?;

        unsafe {
            debug_assert!(len <= payload.len());
            let len = len.min(payload.len());
            self.set_payload_len(len);
        }

        let handle = *message.path_handle();
        handle.update_msg_hdr(&mut self.0);
        self.set_ecn(message.ecn(), &handle.remote_address.0);

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
    type Handle = Handle;

    #[inline]
    fn read(
        &mut self,
        local_address: &path::LocalAddress,
    ) -> Option<(datagram::Header<Self::Handle>, &mut [u8])> {
        let mut header = Self::header(&self.0)?;

        // only copy the port if we are told the IP address
        if cfg!(s2n_quic_platform_pktinfo) {
            header.path.local_address.set_port(local_address.port());
        } else {
            header.path.local_address = *local_address;
        }

        let payload = self.payload_mut();
        Some((header, payload))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bolero::check;

    use s2n_quic_core::inet::{SocketAddress, Unspecified};

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
                unsafe {
                    message.reset(0);
                }
                message.set_remote_address(&addr);

                assert_eq!(message.remote_address(), Some(addr));
            });
    }

    #[test]
    fn handle_get_set_test() {
        check!()
            .with_generator((
                gen::<Handle>(),
                1..=crate::features::get().gso.max_segments(),
            ))
            .cloned()
            .for_each(|(handle, segment_size)| {
                use core::mem::zeroed;

                let mut msghdr = unsafe { zeroed::<msghdr>() };

                let mut msgname = unsafe { zeroed::<sockaddr_in6>() };
                msghdr.msg_name = &mut msgname as *mut _ as *mut _;
                msghdr.msg_namelen = size_of::<sockaddr_in6>() as _;

                let mut iovec = unsafe { zeroed::<iovec>() };
                let mut iovec_buf = [0u8; 16];
                iovec.iov_len = iovec_buf.len() as _;
                iovec.iov_base = (&mut iovec_buf[0]) as *mut u8 as _;
                msghdr.msg_iov = &mut iovec;

                let mut cmsg_buf = [0u8; cmsg::MAX_LEN];
                msghdr.msg_controllen = cmsg_buf.len() as _;
                msghdr.msg_control = (&mut cmsg_buf[0]) as *mut u8 as _;

                let mut message = Message(msghdr);

                handle.update_msg_hdr(&mut message.0);

                if segment_size > 1 {
                    message.set_segment_size(segment_size);
                }

                let header = Message::header(&message.0).unwrap();

                assert_eq!(header.path.remote_address, handle.remote_address);

                if cfg!(s2n_quic_platform_pktinfo) && !handle.local_address.ip().is_unspecified() {
                    assert_eq!(header.path.local_address.ip(), handle.local_address.ip());
                }

                // reset the message and ensure everything is zeroed
                unsafe {
                    message.reset(0);
                }

                let header = Message::header(&msghdr).unwrap();
                assert!(header.path.remote_address.is_unspecified());
            });
    }
}

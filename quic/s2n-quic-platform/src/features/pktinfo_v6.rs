// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::c_int;
use s2n_quic_core::inet::IpV6Address;

#[cfg(s2n_quic_platform_pktinfo)]
mod pktinfo_enabled {
    use super::*;
    use crate::message::cmsg;
    use libc::{IPPROTO_IPV6, IPV6_PKTINFO, IPV6_RECVPKTINFO};

    pub const LEVEL: Option<c_int> = Some(IPPROTO_IPV6 as _);
    pub const TYPE: Option<c_int> = Some(IPV6_PKTINFO as _);
    pub const SOCKOPT: Option<(c_int, c_int)> = Some((IPPROTO_IPV6 as _, IPV6_RECVPKTINFO));
    pub const CMSG_SPACE: usize = crate::message::cmsg::size_of_cmsg::<Cmsg>();

    pub type Cmsg = libc::in6_pktinfo;

    #[inline]
    pub const fn is_match(level: c_int, ty: c_int) -> bool {
        level == IPPROTO_IPV6 as c_int && ty == IPV6_PKTINFO as c_int
    }

    /// # Safety
    ///
    /// * The provided bytes must be aligned to `cmsghdr`
    pub unsafe fn decode(bytes: &[u8]) -> Option<(IpV6Address, u32)> {
        let pkt_info = cmsg::decode::value_from_bytes::<Cmsg>(bytes)?;

        let local_address = pkt_info.ipi6_addr.s6_addr;

        let address = IpV6Address::new(local_address);
        let interface = pkt_info.ipi6_ifindex as _;

        Some((address, interface))
    }

    #[inline]
    pub fn encode(addr: &IpV6Address, local_interface: Option<u32>) -> Cmsg {
        let mut pkt_info = unsafe { core::mem::zeroed::<Cmsg>() };
        pkt_info.ipi6_addr.s6_addr = (*addr).into();
        if let Some(interface) = local_interface {
            pkt_info.ipi6_ifindex = interface as _;
        }
        pkt_info
    }
}

#[cfg(any(not(s2n_quic_platform_pktinfo), test))]
mod pktinfo_disabled {
    #![cfg_attr(test, allow(dead_code))]
    use super::*;

    pub const LEVEL: Option<c_int> = None;
    pub const TYPE: Option<c_int> = None;
    pub const SOCKOPT: Option<(c_int, c_int)> = None;
    pub const CMSG_SPACE: usize = 0;

    pub type Cmsg = c_int;

    #[inline]
    pub const fn is_match(level: c_int, ty: c_int) -> bool {
        let _ = level;
        let _ = ty;
        false
    }

    /// # Safety
    ///
    /// * The provided bytes must be aligned to `cmsghdr`
    pub unsafe fn decode(bytes: &[u8]) -> Option<(IpV6Address, u32)> {
        let _ = bytes;
        None
    }

    #[inline]
    pub fn encode(addr: &IpV6Address, local_interface: Option<u32>) -> Cmsg {
        let _ = addr;
        let _ = local_interface;
        unimplemented!("this platform does not support pktinfo")
    }
}

mod pktinfo_impl {
    #[cfg(not(s2n_quic_platform_pktinfo))]
    pub use super::pktinfo_disabled::*;
    #[cfg(s2n_quic_platform_pktinfo)]
    pub use super::pktinfo_enabled::*;
}

pub use pktinfo_impl::*;

pub const IS_SUPPORTED: bool = cfg!(s2n_quic_platform_pktinfo);

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::c_int;

#[cfg(s2n_quic_platform_tos)]
mod tos_enabled {
    use super::*;
    use libc::{IPPROTO_IPV6, IPV6_RECVTCLASS, IPV6_TCLASS};

    pub const LEVEL: Option<c_int> = Some(IPPROTO_IPV6 as _);
    pub const TYPE: Option<c_int> = Some(IPV6_TCLASS as _);
    pub const SOCKOPT: Option<(c_int, c_int)> = Some((IPPROTO_IPV6 as _, IPV6_RECVTCLASS as _));
    pub const CMSG_SPACE: usize = crate::message::cmsg::size_of_cmsg::<super::Cmsg>();

    #[inline]
    pub const fn is_match(level: c_int, ty: c_int) -> bool {
        level == IPPROTO_IPV6 as c_int
            && (ty == IPV6_TCLASS as c_int || ty == IPV6_RECVTCLASS as c_int)
    }
}

#[cfg(any(not(s2n_quic_platform_tos), test))]
mod tos_disabled {
    #![cfg_attr(test, allow(dead_code))]
    use super::*;

    pub const LEVEL: Option<c_int> = None;
    pub const TYPE: Option<c_int> = None;
    pub const SOCKOPT: Option<(c_int, c_int)> = None;
    pub const CMSG_SPACE: usize = 0;

    #[inline]
    pub const fn is_match(level: c_int, ty: c_int) -> bool {
        let _ = level;
        let _ = ty;
        false
    }
}

mod tos_impl {
    #[cfg(not(s2n_quic_platform_tos))]
    pub use super::tos_disabled::*;
    #[cfg(s2n_quic_platform_tos)]
    pub use super::tos_enabled::*;
}

pub use tos_impl::*;
pub type Cmsg = c_int;
pub const IS_SUPPORTED: bool = cfg!(s2n_quic_platform_tos);

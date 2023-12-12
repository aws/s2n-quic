// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::c_int;

#[cfg(s2n_quic_platform_gro)]
mod gro_enabled {
    use super::*;
    use libc::{SOL_UDP, UDP_GRO};

    pub const LEVEL: Option<c_int> = Some(SOL_UDP as _);
    pub const TYPE: Option<c_int> = Some(UDP_GRO as _);
    pub const SOCKOPT: Option<(c_int, c_int)> = Some((SOL_UDP as _, UDP_GRO as _));
    pub const CMSG_SPACE: usize = crate::message::cmsg::size_of_cmsg::<super::Cmsg>();
    pub const MAX_SEGMENTS: usize = {
        // https://elixir.bootlin.com/linux/latest/source/net/ipv4/udp_offload.c#L463
        //# #define UDP_GRO_CNT_MAX 64
        64
    };

    #[inline]
    pub const fn is_match(level: c_int, ty: c_int) -> bool {
        level == SOL_UDP as c_int && ty == UDP_GRO as c_int
    }
}

#[cfg(any(not(s2n_quic_platform_gro), test))]
mod gro_disabled {
    #![cfg_attr(test, allow(dead_code))]
    use super::*;

    pub const LEVEL: Option<c_int> = None;
    pub const TYPE: Option<c_int> = None;
    pub const SOCKOPT: Option<(c_int, c_int)> = None;
    pub const CMSG_SPACE: usize = 0;
    pub const MAX_SEGMENTS: usize = 1;

    #[inline]
    pub const fn is_match(level: c_int, ty: c_int) -> bool {
        let _ = level;
        let _ = ty;
        false
    }
}

mod gro_impl {
    #[cfg(not(s2n_quic_platform_gro))]
    pub use super::gro_disabled::*;
    #[cfg(s2n_quic_platform_gro)]
    pub use super::gro_enabled::*;
}

pub use gro_impl::*;
pub type Cmsg = c_int;
pub const IS_SUPPORTED: bool = cfg!(s2n_quic_platform_gro);

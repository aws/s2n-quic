// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#![allow(clippy::unnecessary_cast)] // some platforms encode lengths as `u32` so we cast everything to be safe

use crate::features;
use core::mem::size_of;
use libc::cmsghdr;

pub mod decode;
pub mod encode;
pub mod storage;

#[cfg(test)]
mod tests;

pub use encode::Encoder;
pub use storage::Storage;

pub const fn size_of_cmsg<T: Copy + Sized>() -> usize {
    unsafe { libc::CMSG_SPACE(size_of::<T>() as _) as _ }
}

const fn const_max(a: usize, b: usize) -> usize {
    if a > b {
        a
    } else {
        b
    }
}

/// The maximum number of bytes allocated for cmsg data
///
/// This should be enough for UDP_SEGMENT + IP_TOS + IP_PKTINFO. It may need to be increased
/// to allow for future control messages.
pub const MAX_LEN: usize = {
    let tos_v4_size = features::tos_v4::CMSG_SPACE;
    let tos_v6_size = features::tos_v6::CMSG_SPACE;

    let tos_size = const_max(tos_v4_size, tos_v6_size);

    let gso_size = features::gso::CMSG_SPACE;
    let gro_size = features::gro::CMSG_SPACE;

    let segment_offload_size = const_max(gso_size, gro_size);

    // rather than taking the max, we add these in case the OS gives us both
    let pktinfo_size = features::pktinfo_v4::CMSG_SPACE + features::pktinfo_v6::CMSG_SPACE;

    // This is currently needed due to how we detect if CMSG data has been written or not.
    //
    // TODO remove this once we split the `reset` traits into TX and RX types
    let padding = size_of::<cmsghdr>();

    tos_size + segment_offload_size + pktinfo_size + padding
};

#[cfg(test)]
mod tests_ {}

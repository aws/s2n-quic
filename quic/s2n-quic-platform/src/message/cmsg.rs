// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#![allow(clippy::unnecessary_cast)] // some platforms encode lengths as `u32` so we cast everything to be safe

use core::mem::{align_of, size_of};
use s2n_quic_core::inet::{AncillaryData, ExplicitCongestionNotification};

/// The maximum number of bytes allocated for cmsg data
///
/// This should be enough for UDP_SEGMENT + IP_TOS + IP_PKTINFO. It may need to be increased
/// to allow for future control messages.
pub const MAX_LEN: usize = 128;

#[test]
fn max_len_test() {
    let mut len = 0;

    unsafe {
        // UDP_SEGMENT
        len += libc::CMSG_LEN(size_of::<u16>() as _) as usize;

        // IP_TOS
        len += libc::CMSG_LEN(size_of::<libc::c_int>() as _) as usize;

        // IP_PKTINFO
        #[cfg(s2n_quic_platform_pktinfo)]
        {
            len += libc::CMSG_LEN(
                size_of::<libc::in_pktinfo>().max(size_of::<libc::in6_pktinfo>()) as _,
            ) as usize;
        }
    }

    // We use the MAX_LEN to determine if the cmsg has been populated at all so the actual
    // len must be less than it, rather than less than or equal.
    assert!(
        len < MAX_LEN,
        "required len should be less than maximum allocated len"
    );
}

pub trait Encoder {
    /// Encodes the given value as a control message in the cmsg buffer.
    ///
    /// The msghdr.msg_control should be zero-initialized and aligned and contain enough
    /// room for the value to be written.
    fn encode_cmsg<T: Copy + ?Sized>(&mut self, level: libc::c_int, ty: libc::c_int, value: T);
}

impl Encoder for libc::msghdr {
    fn encode_cmsg<T: Copy + ?Sized>(&mut self, level: libc::c_int, ty: libc::c_int, value: T) {
        unsafe {
            debug_assert_ne!(
                self.msg_controllen as usize, MAX_LEN,
                "msg_controllen should be reset when encoding a msg"
            );

            let cmsg =
                // Safety: the msg_control buffer should always be allocated to MAX_LEN
                core::slice::from_raw_parts_mut(self.msg_control as *mut u8, MAX_LEN);
            let cmsg = &mut cmsg[(self.msg_controllen as usize)..];

            debug_assert!(align_of::<T>() <= align_of::<libc::cmsghdr>());
            // CMSG_SPACE() returns the number of bytes an ancillary element
            // with payload of the passed data length occupies.
            let len = libc::CMSG_SPACE(size_of::<T>() as _) as usize;
            debug_assert_ne!(len, 0);
            assert!(
                cmsg.len() >= len,
                "out of space in cmsg: needed {}, got {}",
                len,
                cmsg.len()
            );

            // interpret the start of cmsg as a cmsghdr
            // Safety: the cmsg slice should already be zero-initialized and aligned
            debug_assert!(cmsg.iter().all(|b| *b == 0));
            let cmsg = &mut *(&mut cmsg[0] as *mut u8 as *mut libc::cmsghdr);

            // Indicate the type of cmsg
            cmsg.cmsg_level = level;
            cmsg.cmsg_type = ty;

            // CMSG_LEN() returns the value to store in the cmsg_len member
            // of the cmsghdr structure, taking into account any necessary
            // alignment.  It takes the data length as an argument.
            cmsg.cmsg_len = libc::CMSG_LEN(size_of::<T>() as _) as _;

            // Write the actual value in the data space of the cmsg
            // Safety: we asserted we had enough space in the cmsg buffer above
            // CMSG_DATA() returns a pointer to the data portion of a
            // cmsghdr. The pointer returned cannot be assumed to be
            // suitably aligned for accessing arbitrary payload data types.
            // Applications should not cast it to a pointer type matching the
            // payload, but should instead use memcpy(3) to copy data to or
            // from a suitably declared object.
            core::ptr::write(libc::CMSG_DATA(cmsg) as *const _ as *mut _, value);

            // add the values as a usize to make sure we work cross-platform
            self.msg_controllen = (len + self.msg_controllen as usize) as _;
        }
    }
}

/// Decodes all recognized control messages in the given `msghdr` into `AncillaryData`
#[inline]
pub fn decode(msghdr: &libc::msghdr) -> AncillaryData {
    use core::mem;
    let mut result = AncillaryData::default();
    let cmsg_iter = unsafe { Iter::new(msghdr) };

    for cmsg in cmsg_iter {
        unsafe {
            match (cmsg.cmsg_level, cmsg.cmsg_type, cmsg.cmsg_len as usize) {
                // Linux uses IP_TOS, FreeBSD uses IP_RECVTOS
                (libc::IPPROTO_IP, libc::IP_TOS, cmsg_len)
                | (libc::IPPROTO_IP, libc::IP_RECVTOS, cmsg_len)
                    if cmsg_len == libc::CMSG_LEN(mem::size_of::<u8>() as _) as usize =>
                {
                    result.ecn = ExplicitCongestionNotification::new(decode_value::<u8>(cmsg));
                }
                (libc::IPPROTO_IP, libc::IP_TOS, cmsg_len)
                | (libc::IPPROTO_IP, libc::IP_RECVTOS, cmsg_len)
                    if cmsg_len == libc::CMSG_LEN(mem::size_of::<libc::c_int>() as _) as usize =>
                {
                    // IP_TOS cmsgs should be 1 byte, but occasionally are reported as 4 bytes
                    result.ecn = ExplicitCongestionNotification::new(decode_value::<libc::c_int>(
                        cmsg,
                    ) as u8);
                }
                (libc::IPPROTO_IPV6, libc::IPV6_TCLASS, _) => {
                    result.ecn = ExplicitCongestionNotification::new(decode_value::<libc::c_int>(
                        cmsg,
                    ) as u8);
                }
                #[cfg(s2n_quic_platform_pktinfo)]
                (libc::IPPROTO_IP, libc::IP_PKTINFO, _) => {
                    let pkt_info = decode_value::<libc::in_pktinfo>(cmsg);

                    // read from both fields in case only one is set and not the other
                    //
                    // from https://man7.org/linux/man-pages/man7/ip.7.html:
                    //
                    // > ipi_spec_dst is the local address
                    // > of the packet and ipi_addr is the destination address in
                    // > the packet header.
                    let local_address =
                        match (pkt_info.ipi_addr.s_addr, pkt_info.ipi_spec_dst.s_addr) {
                            (0, v) => v.to_ne_bytes(),
                            (v, _) => v.to_ne_bytes(),
                        };

                    // The port should be specified by a different layer that has that information
                    let port = 0;
                    let local_address =
                        s2n_quic_core::inet::SocketAddressV4::new(local_address, port);
                    result.local_address = local_address.into();
                    result.local_interface = Some(pkt_info.ipi_ifindex as _);
                }
                #[cfg(s2n_quic_platform_pktinfo)]
                (libc::IPPROTO_IPV6, libc::IPV6_PKTINFO, _) => {
                    let pkt_info = decode_value::<libc::in6_pktinfo>(cmsg);
                    let local_address = pkt_info.ipi6_addr.s6_addr;
                    // The port should be specified by a different layer that has that information
                    let port = 0;
                    let local_address =
                        s2n_quic_core::inet::SocketAddressV6::new(local_address, port);
                    result.local_address = local_address.into();
                    result.local_interface = Some(pkt_info.ipi6_ifindex as _);
                }
                #[cfg(s2n_quic_platform_gso)]
                (libc::SOL_UDP, libc::UDP_SEGMENT, _) => {
                    // ignore GSO settings when reading
                    continue;
                }
                (level, ty, len) if cfg!(test) => {
                    // if we're getting an unexpected cmsg we should know about it in testing
                    panic!(
                        "unexpected cmsghdr {{ level: {}, type: {}, len: {} }}",
                        level, ty, len
                    );
                }
                _ => {}
            }
        }
    }

    result
}

/// Decodes a value of type `T` from the given `cmsghdr`
/// # Safety
///
/// `cmsghdr` must refer to a cmsg containing a payload of type `T`
unsafe fn decode_value<T: Copy>(cmsghdr: &libc::cmsghdr) -> T {
    use core::{mem, ptr};

    assert!(mem::align_of::<T>() <= mem::align_of::<libc::cmsghdr>());
    debug_assert_eq!(
        cmsghdr.cmsg_len as usize,
        libc::CMSG_LEN(mem::size_of::<T>() as _) as usize
    );
    ptr::read(libc::CMSG_DATA(cmsghdr) as *const T)
}

struct Iter<'a> {
    msghdr: &'a libc::msghdr,
    cmsghdr: Option<&'a libc::cmsghdr>,
}

impl<'a> Iter<'a> {
    /// Creates a new cmsg::Iter used for iterating over control message headers in the given
    /// `msghdr` using the `CMSG_FIRSTHDR` and `CMSG_NXTHDR` macros.
    ///
    /// # Safety
    ///
    /// `msghdr` must contain valid control messages and be readable for the lifetime
    /// of the returned `Iter`
    unsafe fn new(msghdr: &'a libc::msghdr) -> Self {
        Self {
            msghdr,
            // CMSG_FIRSTHDR() returns a pointer to the first cmsghdr in the
            // ancillary data buffer associated with the passed msghdr.  It
            // returns NULL if there isn't enough space for a cmsghdr in the
            // buffer.
            cmsghdr: libc::CMSG_FIRSTHDR(msghdr).as_ref().filter(|cmsghdr| {
                // make sure we have a length, otherwise it'll loop forever
                cmsghdr.cmsg_len > 0
            }),
        }
    }
}

impl<'a> Iterator for Iter<'a> {
    type Item = &'a libc::cmsghdr;

    fn next(&mut self) -> Option<&'a libc::cmsghdr> {
        let current = self.cmsghdr.take()?;
        // CMSG_NXTHDR() returns the next valid cmsghdr after the passed
        // cmsghdr.  It returns NULL when there isn't enough space left
        // in the buffer.
        self.cmsghdr =
            unsafe { libc::CMSG_NXTHDR(self.msghdr, current).as_ref() }.filter(|cmsghdr| {
                // CMSG_NXTHDR on some operating systems returns the previous pointer when its
                // length is zero, so check if the previous pointer is the same as
                // the current one. Also check if the length is zero, otherwise it'll loop forever.
                cmsghdr.cmsg_len > 0 && !core::ptr::eq(*cmsghdr, current)
            });
        Some(current)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bolero::check;
    use core::mem::zeroed;

    #[test]
    #[cfg_attr(any(target_os = "linux", target_os = "android"), ignore)] // the linux implementation currently has an integer overflow on garbage data
    fn iter_test() {
        check!().for_each(|cmsg| {
            let mut msghdr = unsafe { zeroed::<libc::msghdr>() };
            msghdr.msg_controllen = cmsg.len() as _;
            msghdr.msg_control = if cmsg.is_empty() {
                core::ptr::null_mut() as _
            } else {
                (&cmsg[0]) as *const u8 as _
            };

            for cmsghdr in unsafe { Iter::new(&msghdr) } {
                assert_ne!(
                    cmsghdr.cmsg_len, 0,
                    "iterator should not return empty messages"
                );
            }
        });
    }
}

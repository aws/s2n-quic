// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#![allow(clippy::unnecessary_cast)] // some platforms encode lengths as `u32` so we cast everything to be safe

use core::mem::{align_of, size_of};
use libc::cmsghdr;
use s2n_quic_core::inet::{AncillaryData, ExplicitCongestionNotification};

const fn size_of_cmsg<T: Copy + Sized>() -> usize {
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
    let tos_size = size_of_cmsg::<IpTos>();

    #[cfg(s2n_quic_platform_gso)]
    let gso_size = size_of_cmsg::<UdpGso>();
    #[cfg(not(s2n_quic_platform_gso))]
    let gso_size = 0;

    #[cfg(s2n_quic_platform_gro)]
    let gro_size = size_of_cmsg::<UdpGro>();
    #[cfg(not(s2n_quic_platform_gro))]
    let gro_size = 0;

    let segment_offload_size = const_max(gso_size, gro_size);

    // rather than taking the max, we add these in case the OS gives us both
    #[cfg(s2n_quic_platform_pktinfo)]
    let pktinfo_size = size_of_cmsg::<libc::in_pktinfo>() + size_of_cmsg::<libc::in6_pktinfo>();
    #[cfg(not(s2n_quic_platform_pktinfo))]
    let pktinfo_size = 0;

    // This is currently needed due to how we detect if CMSG data has been written or not.
    //
    // TODO remove this once we split the `reset` traits into TX and RX types
    let padding = size_of::<cmsghdr>();

    tos_size + segment_offload_size + pktinfo_size + padding
};

#[cfg(s2n_quic_platform_gso)]
pub type UdpGso = u16;
#[cfg(s2n_quic_platform_gro)]
pub type UdpGro = libc::c_int;
pub type IpTos = libc::c_int;

#[repr(align(8))] // the storage needs to be aligned to the same as `cmsghdr`
#[derive(Clone, Debug)]
pub struct Storage([u8; MAX_LEN]);

impl Default for Storage {
    fn default() -> Self {
        Self([0; MAX_LEN])
    }
}

impl Storage {
    #[inline]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[inline]
    #[allow(dead_code)] // clippy wants this to exist but we don't use it
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    #[inline]
    pub fn as_mut_ptr(&mut self) -> *mut u8 {
        self.0.as_mut_ptr()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct OutOfSpace;

pub struct SliceEncoder<'a> {
    storage: &'a mut [u8],
    cursor: usize,
}

impl<'a> Encoder for SliceEncoder<'a> {
    #[inline]
    fn encode_cmsg<T: Copy + ?Sized>(
        &mut self,
        level: libc::c_int,
        ty: libc::c_int,
        value: T,
    ) -> Result<usize, OutOfSpace> {
        unsafe {
            debug_assert!(
                align_of::<T>() <= align_of::<cmsghdr>(),
                "alignment of T should be less than or equal to cmsghdr"
            );

            // CMSG_SPACE() returns the number of bytes an ancillary element
            // with payload of the passed data length occupies.
            let element_len = size_of_cmsg::<T>();
            debug_assert_ne!(element_len, 0);
            debug_assert_eq!(libc::CMSG_SPACE(size_of::<T>() as _) as usize, element_len);

            let new_cursor = self.cursor.checked_add(element_len).ok_or(OutOfSpace)?;

            self.storage
                .len()
                .checked_sub(new_cursor)
                .ok_or(OutOfSpace)?;

            let cmsg_ptr = {
                // Safety: the msg_control buffer should always be allocated to MAX_LEN
                let msg_controllen = self.cursor;
                let msg_control = self.storage.as_mut_ptr().add(msg_controllen as _);
                msg_control as *mut cmsghdr
            };

            {
                let cmsg = &mut *cmsg_ptr;

                // interpret the start of cmsg as a cmsghdr
                // Safety: the cmsg slice should already be zero-initialized and aligned

                // Indicate the type of cmsg
                cmsg.cmsg_level = level;
                cmsg.cmsg_type = ty;

                // CMSG_LEN() returns the value to store in the cmsg_len member
                // of the cmsghdr structure, taking into account any necessary
                // alignment.  It takes the data length as an argument.
                cmsg.cmsg_len = libc::CMSG_LEN(size_of::<T>() as _) as _;
            }

            {
                // Write the actual value in the data space of the cmsg
                // Safety: we asserted we had enough space in the cmsg buffer above
                // CMSG_DATA() returns a pointer to the data portion of a
                // cmsghdr. The pointer returned cannot be assumed to be
                // suitably aligned for accessing arbitrary payload data types.
                // Applications should not cast it to a pointer type matching the
                // payload, but should instead use memcpy(3) to copy data to or
                // from a suitably declared object.
                let data_ptr = cmsg_ptr.add(1);

                debug_assert_eq!(data_ptr as *mut u8, libc::CMSG_DATA(cmsg_ptr) as *mut u8);

                core::ptr::copy_nonoverlapping(
                    &value as *const T as *const u8,
                    data_ptr as *mut u8,
                    size_of::<T>(),
                );
            }

            // add the values as a usize to make sure we work cross-platform
            self.cursor = new_cursor;
            debug_assert!(
                self.cursor <= self.storage.len(),
                "msg should not exceed max allocated"
            );

            Ok(self.cursor)
        }
    }
}

pub trait Encoder {
    /// Encodes the given value as a control message in the cmsg buffer.
    ///
    /// The msghdr.msg_control should be zero-initialized and aligned and contain enough
    /// room for the value to be written.
    fn encode_cmsg<T: Copy + ?Sized>(
        &mut self,
        level: libc::c_int,
        ty: libc::c_int,
        value: T,
    ) -> Result<usize, OutOfSpace>;
}

impl Encoder for libc::msghdr {
    #[inline]
    fn encode_cmsg<T: Copy + ?Sized>(
        &mut self,
        level: libc::c_int,
        ty: libc::c_int,
        value: T,
    ) -> Result<usize, OutOfSpace> {
        let storage = unsafe { &mut *(self.msg_control as *mut Storage) };

        let mut encoder = SliceEncoder {
            storage: &mut storage.0,
            cursor: self.msg_controllen as _,
        };

        let cursor = encoder.encode_cmsg(level, ty, value)?;

        self.msg_controllen = cursor as _;

        Ok(cursor)
    }
}

/// Decodes all recognized control messages in the given `msghdr` into `AncillaryData`
#[inline]
pub fn decode(msghdr: &libc::msghdr) -> AncillaryData {
    let mut result = AncillaryData::default();

    let iter = unsafe { Iter::from_msghdr(msghdr) };

    for (cmsg, value) in iter {
        unsafe {
            match (cmsg.cmsg_level, cmsg.cmsg_type) {
                // Linux uses IP_TOS, FreeBSD uses IP_RECVTOS
                (libc::IPPROTO_IP, libc::IP_TOS)
                | (libc::IPPROTO_IP, libc::IP_RECVTOS)
                | (libc::IPPROTO_IPV6, libc::IPV6_TCLASS) => {
                    // IP_TOS cmsgs should be 1 byte, but occasionally are reported as 4 bytes
                    let value = match value.len() {
                        1 => decode_value::<u8>(value),
                        4 => decode_value::<u32>(value) as u8,
                        len => {
                            if cfg!(test) {
                                panic!(
                                    "invalid size for ECN marking. len: {len}, value: {value:?}"
                                );
                            }
                            continue;
                        }
                    };

                    result.ecn = ExplicitCongestionNotification::new(value);
                }
                #[cfg(s2n_quic_platform_pktinfo)]
                (libc::IPPROTO_IP, libc::IP_PKTINFO) => {
                    let pkt_info = decode_value::<libc::in_pktinfo>(value);

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
                (libc::IPPROTO_IPV6, libc::IPV6_PKTINFO) => {
                    let pkt_info = decode_value::<libc::in6_pktinfo>(value);
                    let local_address = pkt_info.ipi6_addr.s6_addr;
                    // The port should be specified by a different layer that has that information
                    let port = 0;
                    let local_address =
                        s2n_quic_core::inet::SocketAddressV6::new(local_address, port);
                    result.local_address = local_address.into();
                    result.local_interface = Some(pkt_info.ipi6_ifindex as _);
                }
                #[cfg(s2n_quic_platform_gso)]
                (libc::SOL_UDP, libc::UDP_SEGMENT) => {
                    // ignore GSO settings when reading
                    continue;
                }
                #[cfg(s2n_quic_platform_gro)]
                (libc::SOL_UDP, libc::UDP_GRO) => {
                    let segment_size = decode_value::<UdpGro>(value);
                    result.segment_size = segment_size as _;
                }
                (level, ty) if cfg!(test) => {
                    // if we're getting an unexpected cmsg we should know about it in testing
                    panic!("unexpected cmsghdr {{ level: {level}, type: {ty}, value: {value:?} }}");
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
#[inline]
unsafe fn decode_value<T: Copy>(value: &[u8]) -> T {
    use core::mem;

    debug_assert!(mem::align_of::<T>() <= mem::align_of::<cmsghdr>());
    debug_assert!(value.len() >= size_of::<T>());

    let mut v = mem::zeroed::<T>();

    core::ptr::copy_nonoverlapping(value.as_ptr(), &mut v as *mut T as *mut u8, size_of::<T>());

    v
}

struct Iter<'a> {
    cursor: *const u8,
    len: usize,
    contents: core::marker::PhantomData<&'a [u8]>,
}

impl<'a> Iter<'a> {
    /// Creates a new cmsg::Iter used for iterating over control message headers in the given
    /// slice of bytes.
    ///
    /// # Safety
    ///
    /// * `contents` must be aligned to cmsghdr
    #[inline]
    unsafe fn new(contents: &'a [u8]) -> Iter<'a> {
        let cursor = contents.as_ptr();
        let len = contents.len();

        debug_assert_eq!(
            cursor.align_offset(align_of::<cmsghdr>()),
            0,
            "contents must be aligned to cmsghdr"
        );

        Self {
            cursor,
            len,
            contents: Default::default(),
        }
    }

    #[inline]
    unsafe fn from_msghdr(msghdr: &'a libc::msghdr) -> Self {
        let ptr = msghdr.msg_control as *const u8;
        let len = msghdr.msg_controllen as usize;
        let slice = core::slice::from_raw_parts(ptr, len);
        Self::new(slice)
    }
}

impl<'a> Iterator for Iter<'a> {
    type Item = (&'a cmsghdr, &'a [u8]);

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        unsafe {
            let cursor = self.cursor;

            // make sure we can decode a cmsghdr
            self.len.checked_sub(size_of::<cmsghdr>())?;
            let cmsg = &*(cursor as *const cmsghdr);
            let data_ptr = cursor.add(size_of::<cmsghdr>());

            let cmsg_len = cmsg.cmsg_len as usize;

            // make sure we have capacity to decode the provided cmsg_len
            self.len.checked_sub(cmsg_len)?;

            // the cmsg_len includes the header itself so it needs to be subtracted off
            let data_len = cmsg_len.checked_sub(size_of::<cmsghdr>())?;
            // construct a slice with the provided data len
            let data = core::slice::from_raw_parts(data_ptr, data_len);

            // empty messages are invalid
            if data.is_empty() {
                return None;
            }

            // calculate the next message and update the cursor/len
            {
                let space = libc::CMSG_SPACE(data_len as _) as usize;
                debug_assert!(
                    space >= data_len,
                    "space ({space}) should be at least of size len ({data_len})"
                );
                self.len = self.len.saturating_sub(space);
                self.cursor = cursor.add(space);
            }

            Some((cmsg, data))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bolero::{check, TypeGenerator};
    use libc::c_int;

    /// Ensures the cmsg iterator doesn't crash or segfault
    #[test]
    #[cfg_attr(kani, kani::proof, kani::solver(cadical), kani::unwind(17))]
    fn iter_test() {
        check!().for_each(|cmsg| {
            // the bytes needs to be aligned to a cmsghdr
            let offset = cmsg.as_ptr().align_offset(align_of::<cmsghdr>());

            if let Some(cmsg) = cmsg.get(offset..) {
                for (cmsghdr, value) in unsafe { Iter::new(cmsg) } {
                    let _ = cmsghdr;
                    let _ = value;
                }
            }
        });
    }

    #[derive(Clone, Copy, Debug, TypeGenerator)]
    struct Op {
        level: c_int,
        ty: c_int,
        value: Value,
    }

    #[derive(Clone, Copy, Debug, TypeGenerator)]
    enum Value {
        U8(u8),
        U16(u16),
        U32(u32),
        // alignment can't exceed that of cmsghdr
        U64([u32; 2]),
        U128([u32; 4]),
    }

    impl Value {
        fn check_value(&self, bytes: &[u8]) {
            let expected_len = match self {
                Self::U8(_) => 1,
                Self::U16(_) => 2,
                Self::U32(_) => 4,
                Self::U64(_) => 8,
                Self::U128(_) => 16,
            };
            assert_eq!(expected_len, bytes.len());
        }
    }

    fn round_trip(ops: &[Op]) {
        let mut storage = Storage::default();
        let mut encoder = SliceEncoder {
            storage: &mut storage.0,
            cursor: 0,
        };

        let mut expected_encoded_count = 0;

        for op in ops {
            let res = match op.value {
                Value::U8(value) => encoder.encode_cmsg(op.level, op.ty, value),
                Value::U16(value) => encoder.encode_cmsg(op.level, op.ty, value),
                Value::U32(value) => encoder.encode_cmsg(op.level, op.ty, value),
                Value::U64(value) => encoder.encode_cmsg(op.level, op.ty, value),
                Value::U128(value) => encoder.encode_cmsg(op.level, op.ty, value),
            };

            match res {
                Ok(_) => expected_encoded_count += 1,
                Err(_) => break,
            }
        }

        let cursor = encoder.cursor;
        let mut actual_decoded_count = 0;
        let mut iter = unsafe { Iter::new(&storage.0[..cursor]) };

        for (op, (cmsghdr, value)) in ops.iter().zip(&mut iter) {
            assert_eq!(op.level, cmsghdr.cmsg_level);
            assert_eq!(op.ty, cmsghdr.cmsg_type);
            op.value.check_value(value);
            actual_decoded_count += 1;
        }

        assert_eq!(expected_encoded_count, actual_decoded_count);
        assert!(iter.next().is_none());
    }

    #[cfg(not(kani))]
    type Ops = Vec<Op>;
    #[cfg(kani)]
    type Ops = s2n_quic_core::testing::InlineVec<Op, 8>;

    #[test]
    #[cfg_attr(kani, kani::proof, kani::solver(cadical), kani::unwind(9))]
    fn round_trip_test() {
        check!().with_type::<Ops>().for_each(|ops| round_trip(ops));
    }
}

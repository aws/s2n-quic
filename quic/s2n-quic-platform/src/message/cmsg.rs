// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0
use s2n_quic_core::inet::{AncillaryData, ExplicitCongestionNotification};

/// Encodes the given value as a control message in the given cmsg buffer.
///
/// The cmsg slice should be zero-initialized and aligned and contain enough
/// room for the value to be written.
pub fn encode<T: Copy + ?Sized>(
    cmsg: &mut [u8],
    level: libc::c_int,
    ty: libc::c_int,
    value: T,
) -> usize {
    use core::mem::{align_of, size_of};

    unsafe {
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

        len
    }
}

/// Decodes all recognized control messages in the given `msghdr` into `AncillaryData`
pub fn decode(msghdr: &libc::msghdr) -> AncillaryData {
    let mut result = AncillaryData::default();
    let cmsg_iter = unsafe { Iter::new(msghdr) };

    for cmsg in cmsg_iter {
        match (cmsg.cmsg_type, cmsg.cmsg_level) {
            // Linux uses IP_TOS, FreeBSD uses IP_RECVTOS
            (libc::IPPROTO_IP, libc::IP_TOS) | (libc::IPPROTO_IP, libc::IP_RECVTOS) => unsafe {
                result.ecn = ExplicitCongestionNotification::new(decode_value::<u8>(cmsg));
            },
            (libc::IPPROTO_IPV6, libc::IPV6_TCLASS) => unsafe {
                result.ecn =
                    ExplicitCongestionNotification::new(decode_value::<libc::c_int>(cmsg) as u8);
            },
            _ => {}
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
            cmsghdr: libc::CMSG_FIRSTHDR(msghdr).as_ref(),
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
        self.cmsghdr = unsafe { libc::CMSG_NXTHDR(self.msghdr, current).as_ref() };
        Some(current)
    }
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

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

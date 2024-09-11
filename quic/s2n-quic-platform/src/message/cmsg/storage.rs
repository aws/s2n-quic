// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{encode, size_of_cmsg};
use core::{
    mem::{align_of, size_of},
    ops::{Deref, DerefMut},
};
use libc::cmsghdr;

#[repr(align(8))] // the storage needs to be aligned to the same as `cmsghdr`
#[derive(Clone, Debug)]
pub struct Storage<const L: usize>([u8; L]);

impl<const L: usize> Storage<L> {
    #[inline]
    pub fn encoder(&mut self) -> Encoder<L> {
        Encoder {
            storage: self,
            cursor: 0,
        }
    }

    #[inline]
    pub fn iter(&self) -> super::decode::Iter {
        super::decode::Iter::new(self)
    }
}

impl<const L: usize> Default for Storage<L> {
    #[inline]
    fn default() -> Self {
        Self([0; L])
    }
}

impl<const L: usize> Deref for Storage<L> {
    type Target = [u8];

    #[inline]
    fn deref(&self) -> &[u8] {
        &self.0
    }
}

impl<const L: usize> DerefMut for Storage<L> {
    #[inline]
    fn deref_mut(&mut self) -> &mut [u8] {
        &mut self.0
    }
}

pub struct Encoder<'a, const L: usize> {
    storage: &'a mut Storage<L>,
    cursor: usize,
}

impl<'a, const L: usize> Encoder<'a, L> {
    #[inline]
    pub fn new(storage: &'a mut Storage<L>) -> Self {
        Self { storage, cursor: 0 }
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.cursor
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.cursor == 0
    }

    #[inline]
    pub fn seek(&mut self, len: usize) {
        self.cursor += len;
        debug_assert!(self.cursor <= L);
    }

    #[inline]
    pub fn iter(&self) -> super::decode::Iter {
        unsafe {
            // SAFETY: bytes are aligned with Storage type
            super::decode::Iter::from_bytes(self)
        }
    }
}

impl<'a, const L: usize> Deref for Encoder<'a, L> {
    type Target = [u8];

    #[inline]
    fn deref(&self) -> &[u8] {
        &self.storage[..self.cursor]
    }
}

impl<'a, const L: usize> DerefMut for Encoder<'a, L> {
    #[inline]
    fn deref_mut(&mut self) -> &mut [u8] {
        &mut self.storage[..self.cursor]
    }
}

impl<'a, const L: usize> super::Encoder for Encoder<'a, L> {
    #[inline]
    fn encode_cmsg<T: Copy>(
        &mut self,
        level: libc::c_int,
        ty: libc::c_int,
        value: T,
    ) -> Result<usize, encode::Error> {
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

            let new_cursor = self.cursor.checked_add(element_len).ok_or(encode::Error)?;

            self.storage
                .len()
                .checked_sub(new_cursor)
                .ok_or(encode::Error)?;

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

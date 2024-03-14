// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    syscall::{mmap, munmap},
    Result,
};
use core::{
    ffi::c_void,
    ops::{Deref, DerefMut},
    ptr::NonNull,
};
use std::os::unix::io::RawFd;

/// A mmap'd region in memory
#[derive(Debug)]
pub struct Mmap {
    addr: NonNull<c_void>,
    len: usize,
}

#[derive(Debug)]
pub enum Options {
    Huge,
    Fd(RawFd),
}

/// Safety: Mmap pointer can be sent between threads
unsafe impl Send for Mmap {}

/// Safety: Mmap pointer can be shared between threads
unsafe impl Sync for Mmap {}

impl Mmap {
    /// Creates a new mmap'd region, with an optional file descriptor.
    #[inline]
    pub fn new(len: usize, offset: usize, flags: Option<Options>) -> Result<Self> {
        let addr = match flags {
            Some(Options::Huge) => mmap(len, offset, None, true),
            Some(Options::Fd(fd)) => mmap(len, offset, Some(fd), false),
            _ => mmap(len, offset, None, false),
        }?;
        Ok(Self { addr, len })
    }

    /// Returns the raw address for the mmap region
    #[inline]
    pub fn addr(&self) -> NonNull<c_void> {
        self.addr
    }
}

impl Deref for Mmap {
    type Target = [u8];

    #[inline]
    fn deref(&self) -> &Self::Target {
        unsafe { core::slice::from_raw_parts(self.addr.as_ptr() as _, self.len) }
    }
}

impl DerefMut for Mmap {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { core::slice::from_raw_parts_mut(self.addr.as_ptr() as _, self.len) }
    }
}

impl Drop for Mmap {
    #[inline]
    fn drop(&mut self) {
        let _ = unsafe {
            // Safety: the len is the same value as on creation
            munmap(self.addr, self.len)
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mmap_test() {
        let mut area = Mmap::new(32, 0, None).unwrap();
        assert_eq!(area.len(), 32);
        let _ = &area[..];
        let _ = &mut area[..];
    }
}

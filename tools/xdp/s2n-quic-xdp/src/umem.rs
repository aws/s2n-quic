// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    if_xdp::{RxTxDescriptor, UmemDescriptor, UmemFlags, UmemReg},
    mmap::{Mmap, MmapOptions},
    syscall, Result,
};
use core::ptr::NonNull;
use std::{os::unix::io::AsRawFd, sync::Arc};

/// The default value for frame sizes
pub const DEFAULT_FRAME_SIZE: u32 = 4096;

#[derive(Clone, Copy, Debug)]
pub struct Builder {
    /// The maximum number of bytes a frame can hold (MTU)
    pub frame_size: u32,
    /// The number of frames that should be allocated
    pub frame_count: u32,
    /// The headroom size for each frame
    pub frame_headroom: u32,
    /// The flags for the Umem
    pub flags: UmemFlags,
    /// Back the umem with a hugepage
    pub hugepage: bool
}

impl Default for Builder {
    fn default() -> Self {
        Self {
            frame_size: DEFAULT_FRAME_SIZE,
            frame_count: 1024,
            frame_headroom: 0,
            flags: Default::default(),
            hugepage: false,
        }
    }
}

impl Builder {
    pub fn build(self) -> Result<Umem> {
        let len = self.frame_size as usize * self.frame_count as usize;
        let options = if self.hugepage {
            Some(MmapOptions::Huge)
        } else {
            None
        };
        let area = Mmap::new(len, 0, options)?;
        let area = Arc::new(area);
        let mem = area.addr().cast();

        Ok(Umem {
            area,
            mem,
            frame_size: self.frame_size,
            frame_count: self.frame_count,
            flags: self.flags,
            frame_headroom: self.frame_headroom,
        })
    }
}

/// A shared region of memory for holding frame (packet) data
///
/// Callers are responsible for correct descriptor allocation. This means only one descriptor
/// should be alive for each frame. If this invariant is not held, the borrowing rules will be
/// violated and potentially result in UB.
#[derive(Clone)]
pub struct Umem {
    area: Arc<Mmap>,
    mem: NonNull<u8>,
    frame_size: u32,
    frame_count: u32,
    frame_headroom: u32,
    flags: UmemFlags,
}

/// Safety: The umem mmap region can be sent to other threads
unsafe impl Send for Umem {}

/// Safety: The umem mmap region is synchronized by Rings and the data getters are marked as
/// `unsafe`.
unsafe impl Sync for Umem {}

impl Umem {
    /// Creates a Umem builder with defaults
    pub fn builder() -> Builder {
        Builder::default()
    }

    /// Returns the configured size for each frame
    #[inline]
    pub fn frame_size(&self) -> u32 {
        self.frame_size
    }

    /// Returns the total number of frames in the Umem
    #[inline]
    pub fn frame_count(&self) -> u32 {
        self.frame_count
    }

    /// Returns the configured headroom for each frame
    #[inline]
    pub fn frame_headroom(&self) -> u32 {
        self.frame_headroom
    }

    /// Returns the flags for the Umem
    #[inline]
    pub fn flags(&self) -> UmemFlags {
        self.flags
    }

    /// Returns an iterator over all of the frame descriptors
    ///
    /// This can be used to initialize a frame allocator
    pub fn frames(&self) -> impl Iterator<Item = UmemDescriptor> {
        let size = self.frame_size as u64;
        (0..self.frame_count as u64).map(move |idx| UmemDescriptor {
            address: idx * size,
        })
    }

    /// Returns the number of bytes in the Umem
    #[inline]
    pub fn len(&self) -> usize {
        self.area.len()
    }

    /// Returns the pointer to the umem memory region
    #[inline]
    pub fn as_ptr(&self) -> *mut u8 {
        self.mem.as_ptr()
    }

    /// Returns `true` if the Umem is empty
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.area.is_empty()
    }

    /// Returns the region of memory as specified by the index type
    ///
    /// # Safety
    ///
    /// The caller MUST ensure that this index is not already mutably borrowed and that the index
    /// is in bounds for this Umem.
    #[inline]
    pub unsafe fn get<T>(&self, idx: T) -> &[u8]
    where
        Self: UnsafeIndex<T>,
    {
        self.index(idx)
    }

    /// Returns the mutable region of memory as specified by the index type
    ///
    /// # Safety
    ///
    /// The caller MUST ensure that this index is not already mutably borrowed and that the index
    /// is in bounds for this Umem.
    #[inline]
    #[allow(clippy::mut_from_ref)] // interior mutability safety is enforced by the caller
    pub unsafe fn get_mut<T>(&self, idx: T) -> &mut [u8]
    where
        Self: UnsafeIndex<T>,
    {
        self.index_mut(idx)
    }

    /// Attaches the Umem to the specified socket
    pub(crate) fn attach<Fd: AsRawFd>(&self, socket: &Fd) -> Result<()> {
        let umem_conf = UmemReg {
            addr: self.area.addr().as_ptr() as _,
            chunk_size: self.frame_size,
            flags: self.flags,
            headroom: self.frame_headroom,
            len: self.area.len() as _,
        };

        syscall::set_umem(socket, &umem_conf)?;

        Ok(())
    }

    #[inline]
    fn validate_rx_tx_descriptor(&self, desc: RxTxDescriptor) -> *mut u8 {
        debug_assert!(desc.len <= self.frame_size, "frame too large");
        debug_assert!(
            desc.address + desc.len as u64 <= self.area.len() as u64,
            "pointer out of bounds"
        );
        unsafe { self.as_ptr().add(desc.address as _) }
    }

    #[inline]
    fn validate_umem_descriptor(&self, desc: UmemDescriptor) -> *mut u8 {
        debug_assert!(
            desc.address + self.frame_size as u64 <= self.area.len() as u64,
            "pointer out of bounds"
        );
        unsafe { self.as_ptr().add(desc.address as _) }
    }
}

/// Specifies an indexable value, which relies on the caller to guarantee borrowing rules are not
/// violated.
pub trait UnsafeIndex<T> {
    /// # Safety
    ///
    /// Callers need to guarantee the reference is not already exclusively borrowed
    unsafe fn index(&self, idx: T) -> &[u8];

    /// # Safety
    ///
    /// Callers need to guarantee the reference is not already exclusively borrowed
    #[allow(clippy::mut_from_ref)] // interior mutability safety is enforced by the caller
    unsafe fn index_mut(&self, idx: T) -> &mut [u8];
}

impl UnsafeIndex<RxTxDescriptor> for Umem {
    #[inline]
    unsafe fn index(&self, idx: RxTxDescriptor) -> &[u8] {
        let ptr = self.validate_rx_tx_descriptor(idx);
        core::slice::from_raw_parts(ptr, idx.len as _)
    }

    #[inline]
    unsafe fn index_mut(&self, idx: RxTxDescriptor) -> &mut [u8] {
        let ptr = self.validate_rx_tx_descriptor(idx);
        core::slice::from_raw_parts_mut(ptr, idx.len as _)
    }
}

impl UnsafeIndex<UmemDescriptor> for Umem {
    #[inline]
    unsafe fn index(&self, idx: UmemDescriptor) -> &[u8] {
        let ptr = self.validate_umem_descriptor(idx);
        core::slice::from_raw_parts(ptr, self.frame_size as _)
    }

    #[inline]
    unsafe fn index_mut(&self, idx: UmemDescriptor) -> &mut [u8] {
        let ptr = self.validate_umem_descriptor(idx);
        core::slice::from_raw_parts_mut(ptr, self.frame_size as _)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_test() {
        let umem = Umem::builder().build().unwrap();

        for descriptor in umem.frames() {
            unsafe {
                let _ = umem.get(descriptor);
                let _ = umem.get_mut(descriptor);
            }

            let rx = RxTxDescriptor {
                address: descriptor.address,
                len: 1,
                options: Default::default(),
            };

            unsafe {
                let _ = umem.get(rx);
                let _ = umem.get_mut(rx);
            }
        }
    }
}

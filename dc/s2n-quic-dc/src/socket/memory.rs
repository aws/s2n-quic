// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::msg::addr::Addr;
use core::cell::UnsafeCell;
use s2n_quic_core::sync::spsc::Sender;
use std::sync::{Arc, Mutex};

pub struct Memory {
    addresses: Box<[UnsafeCell<Addr>]>,
    packets: Box<[UnsafeCell<u8>]>,
    capacity: u16,
    // TODO get rid of the mutex
    free_sender: Mutex<Sender<u64>>,
}

pub struct FreeDescriptor {
    offset: u64,
    memory: Arc<Memory>,
}

impl FreeDescriptor {
    #[inline]
    fn addr_ptr(&self) -> *const Addr {
        unsafe {
            let ptr = self.memory.addresses.as_ptr().add(self.offset as usize);
            ptr as *const _
        }
    }

    #[inline]
    fn addr_ptr_mut(&mut self) -> *mut Addr {
        unsafe {
            let ptr = self.memory.addresses.as_ptr().add(self.offset as usize);
            // https://doc.rust-lang.org/std/cell/struct.UnsafeCell.html#method.raw_get
            // Gets a mutable pointer to the wrapped value. The difference from get is that this function accepts a
            // raw pointer, which is useful to avoid the creation of temporary references.
            UnsafeCell::raw_get(ptr)
        }
    }

    #[inline]
    fn data_ptr(&self) -> *const u8 {
        unsafe {
            let ptr = self.memory.packets.as_ptr().add(self.offset as usize);
            ptr as *const u8
        }
    }

    #[inline]
    fn data_ptr_mut(&mut self) -> *mut u8 {
        unsafe {
            let ptr = self.memory.packets.as_ptr().add(self.offset as usize);
            // https://doc.rust-lang.org/std/cell/struct.UnsafeCell.html#method.raw_get
            // Gets a mutable pointer to the wrapped value. The difference from get is that this function accepts a
            // raw pointer, which is useful to avoid the creation of temporary references.
            UnsafeCell::raw_get(ptr)
        }
    }
}

impl Drop for FreeDescriptor {
    #[inline]
    fn drop(&mut self) {
        let Ok(mut free) = self.memory.free_sender.lock() else {
            return;
        };

        let Ok(Some(mut free)) = free.try_slice() else {
            return;
        };

        let _ = free.push(self.offset);
    }
}

pub struct FilledDescriptor {
    inner: FreeDescriptor,
    len: u32,
}

impl FilledDescriptor {
    #[inline]
    pub fn addr(&self) -> &Addr {
        unsafe {
            let ptr = self.inner.addr_ptr();
            &*ptr
        }
    }

    #[inline]
    pub fn addr_mut(&mut self) -> &mut Addr {
        unsafe {
            let ptr = self.inner.addr_ptr_mut();
            &mut *ptr
        }
    }

    #[inline]
    pub fn data(&self) -> &[u8] {
        unsafe {
            let ptr = self.inner.data_ptr();
            let len = self.len as usize;
            core::slice::from_raw_parts(ptr, len)
        }
    }

    #[inline]
    pub fn data_mut(&mut self) -> &mut [u8] {
        unsafe {
            let ptr = self.inner.data_ptr_mut();
            let len = self.len as usize;
            core::slice::from_raw_parts_mut(ptr, len)
        }
    }
}

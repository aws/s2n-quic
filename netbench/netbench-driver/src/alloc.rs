// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use probe::probe;

pub struct Allocator {
    inner: mimalloc::MiMalloc,
}

impl Allocator {
    pub const fn new() -> Self {
        Self {
            inner: mimalloc::MiMalloc,
        }
    }
}

#[inline(never)]
#[allow(unused_variables)]
fn alloc(size: usize) {
    probe!(netbench, netbench__alloc, size);
}

#[inline(never)]
#[allow(unused_variables)]
fn dealloc(size: usize) {
    probe!(netbench, netbench__dealloc, size);
}

#[inline(never)]
#[allow(unused_variables)]
fn realloc(prev_size: usize, new_size: usize) {
    probe!(netbench, netbench__realloc, prev_size, new_size);
}

unsafe impl std::alloc::GlobalAlloc for Allocator {
    #[inline]
    unsafe fn alloc(&self, layout: std::alloc::Layout) -> *mut u8 {
        alloc(layout.size());
        self.inner.alloc(layout)
    }

    #[inline]
    unsafe fn dealloc(&self, ptr: *mut u8, layout: std::alloc::Layout) {
        dealloc(layout.size());
        self.inner.dealloc(ptr, layout)
    }

    #[inline]
    unsafe fn realloc(&self, ptr: *mut u8, layout: std::alloc::Layout, new_size: usize) -> *mut u8 {
        realloc(layout.size(), new_size);
        self.inner.realloc(ptr, layout, new_size)
    }
}

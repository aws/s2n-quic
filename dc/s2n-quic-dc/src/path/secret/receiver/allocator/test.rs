// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::Allocator;
use std::alloc::Layout;

#[test]
fn trivial_check() {
    let allocator = Allocator::with_capacity(8192);
    let handle1 = allocator.allocate(Layout::new::<u32>()).handle();
    let handle2 = allocator.allocate(Layout::new::<u32>()).handle();
    let ptr1 = allocator.read_allocation(handle1).unwrap();
    let ptr2 = allocator.read_allocation(handle2).unwrap();
    assert_ne!(ptr1.as_ptr(), ptr2.as_ptr());
    drop(ptr1);
    drop(ptr2);
    unsafe {
        allocator.deallocate(handle1);
        allocator.deallocate(handle2);
    }
}

#[test]
fn fills_page() {
    // 1 means we allocate a single page
    let allocator = Allocator::with_capacity(1);
    let mut handles = vec![];
    for _ in 0..1021 {
        handles.push(allocator.allocate(Layout::new::<u32>()).handle());
    }
    let mut count = 0;
    for handle in handles.iter() {
        count += allocator.read_allocation(*handle).is_some() as usize;
    }
    assert_eq!(count, handles.len());
}

#[test]
fn allocates_indefinitely() {
    // 1 means we allocate a single page
    let allocator = Allocator::with_capacity(1);
    assert_eq!(allocator.inner.lock().unwrap().free_pages.len(), 1);
    let layout = Layout::new::<u32>();
    let mut handles = vec![];
    for _ in 0..(1021 * if cfg!(miri) { 2 } else { 1000 }) {
        handles.push(allocator.allocate(layout).handle());
    }
    let mut count = 0;
    for handle in handles {
        count += allocator.read_allocation(handle).is_some() as usize;

        unsafe {
            allocator.deallocate(handle);
        }
    }
    // no fragmentation - we emptied a bunch of pages but we still have a full page allocated at
    // the end.
    assert_eq!(count, 1021);
}

#[test]
fn allocate_and_deallocate_multipage() {
    let allocator = Allocator::with_capacity(super::PAGE_SIZE * 3);
    assert_eq!(allocator.inner.lock().unwrap().free_pages.len(), 3);
    let mut handles = vec![];
    let layout = Layout::new::<u32>();
    for _ in 0..3000 {
        handles.push(allocator.allocate(layout).handle());
    }
    let mut count = 0;
    for handle in handles.iter() {
        count += allocator.read_allocation(*handle).is_some() as usize;
    }
    assert_eq!(count, 3000);

    for handle in handles {
        unsafe {
            allocator.deallocate(handle);
        }
    }
}

#[test]
fn allocate_and_deallocate_multilayout() {
    let allocator = Allocator::with_capacity(super::PAGE_SIZE * 10);
    assert_eq!(allocator.inner.lock().unwrap().free_pages.len(), 10);
    let mut handles = vec![];
    let layout1 = Layout::new::<[u32; 1]>();
    let layout2 = Layout::new::<[u32; 2]>();
    let layout3 = Layout::new::<[u32; 3]>();
    for _ in 0..1000 {
        handles.push(allocator.allocate(layout1).handle());
    }
    for _ in 0..1000 {
        handles.push(allocator.allocate(layout2).handle());
    }
    for _ in 0..1000 {
        handles.push(allocator.allocate(layout3).handle());
    }
    let mut count = 0;
    for handle in handles.iter() {
        count += allocator.read_allocation(*handle).is_some() as usize;
    }
    assert_eq!(count, 3000);

    for handle in handles[..1000].iter() {
        unsafe {
            allocator.deallocate(*handle);
        }
    }
    for handle in handles[1000..2000].iter() {
        unsafe {
            allocator.deallocate(*handle);
        }
    }
    for handle in handles[2000..].iter() {
        unsafe {
            allocator.deallocate(*handle);
        }
    }
}

#[test]
fn reuse_handle() {
    let allocator = Allocator::with_capacity(1);
    let handle1 = allocator.allocate(Layout::new::<u32>()).handle();
    unsafe {
        allocator.deallocate(handle1);
    }
    let handle2 = allocator.allocate(Layout::new::<u32>()).handle();
    unsafe {
        allocator.deallocate(handle2);
    }
    assert_eq!(handle1, handle2);
}

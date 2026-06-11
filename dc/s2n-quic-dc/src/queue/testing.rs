// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{endpoint::msg, intrusive};
use bytes::BytesMut;
use core::task::{Context, RawWaker, RawWakerVTable, Waker};
use s2n_quic_core::varint::VarInt;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

pub fn make_stream_entry() -> intrusive::Entry<msg::Stream> {
    intrusive::Entry::new(msg::Stream::Data {
        offset: VarInt::ZERO,
        peer_max_offset: VarInt::ZERO,
        fin: false,
        blocked: false,
        payload: BytesMut::from(&[42][..]),
    })
}

pub fn make_control_entry() -> intrusive::Entry<msg::Control> {
    intrusive::Entry::new(msg::Control::Frames {
        payload: BytesMut::from(&[0][..]),
    })
}

pub fn test_waker() -> (Waker, Arc<AtomicUsize>) {
    let count = Arc::new(AtomicUsize::new(0));
    let data = Arc::into_raw(count.clone()) as *const ();
    let raw = RawWaker::new(data, &VTABLE);
    let waker = unsafe { Waker::from_raw(raw) };
    (waker, count)
}

pub fn test_context<'a>(waker: &'a Waker) -> Context<'a> {
    Context::from_waker(waker)
}

const VTABLE: RawWakerVTable = RawWakerVTable::new(clone_fn, wake_fn, wake_by_ref_fn, drop_fn);

unsafe fn clone_fn(data: *const ()) -> RawWaker {
    Arc::increment_strong_count(data as *const AtomicUsize);
    RawWaker::new(data, &VTABLE)
}

unsafe fn wake_fn(data: *const ()) {
    let arc = Arc::from_raw(data as *const AtomicUsize);
    arc.fetch_add(1, Ordering::SeqCst);
}

unsafe fn wake_by_ref_fn(data: *const ()) {
    let arc = unsafe { &*(data as *const AtomicUsize) };
    arc.fetch_add(1, Ordering::SeqCst);
}

unsafe fn drop_fn(data: *const ()) {
    Arc::decrement_strong_count(data as *const AtomicUsize);
}

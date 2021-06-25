// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::{
    sync::atomic::{AtomicBool, Ordering},
    task::{Context, RawWaker, RawWakerVTable, Waker},
};
use std::{
    sync::Arc,
    thread::{self, JoinHandle, Thread},
};

pub struct ThreadSocket {
    open: Arc<AtomicBool>,
    worker: JoinHandle<()>,
}

pub enum Control {
    Continue,
    Break,
    Sleep,
}

pub trait Socket: 'static + Send {
    fn poll_progress(&mut self, cx: &mut Context<'_>) -> Control;
}

impl ThreadSocket {
    pub fn new<S: Socket>(mut socket: S) -> Self {
        let open = Arc::new(AtomicBool::new(true));
        let open_worker = open.clone();

        let worker = thread::spawn(move || {
            let waker = unsafe { Waker::from_raw(raw_waker(thread::current())) };

            while open_worker.load(Ordering::Relaxed) {
                let mut context = Context::from_waker(&waker);
                match socket.poll_progress(&mut context) {
                    Control::Continue => {}
                    Control::Sleep => thread::park(),
                    Control::Break => return,
                }
            }
        });

        Self { open, worker }
    }
}

impl Drop for ThreadSocket {
    fn drop(&mut self) {
        self.open.store(false, Ordering::SeqCst);
        self.worker.thread().unpark();
    }
}

static VTABLE: RawWakerVTable = RawWakerVTable::new(w_clone, w_wake, w_wake_by_ref, w_drop);

fn raw_waker(thread: Thread) -> RawWaker {
    let thread = Box::new(thread);
    let data = Box::into_raw(thread) as *const _;
    RawWaker::new(data, &VTABLE)
}

unsafe fn w_clone(data: *const ()) -> RawWaker {
    let thread = Box::from_raw(data as *const _ as *mut Thread);
    let thread = Box::leak(thread);
    let thread = thread.clone();
    raw_waker(thread)
}

unsafe fn w_wake(data: *const ()) {
    let thread = Box::from_raw(data as *const _ as *mut Thread);
    thread.unpark();
}

unsafe fn w_wake_by_ref(data: *const ()) {
    let thread = Box::from_raw(data as *const _ as *mut Thread);
    let thread = Box::leak(thread);
    thread.unpark();
}

unsafe fn w_drop(data: *const ()) {
    let thread = Box::from_raw(data as *const _ as *mut Thread);
    drop(thread);
}

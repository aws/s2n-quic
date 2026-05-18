// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use std::{cell::UnsafeCell, rc::Rc, task::Poll};

struct Shared<T> {
    value: UnsafeCell<Option<T>>,
}

impl<T> Shared<T> {
    #[inline(always)]
    fn poll(self: &Rc<Self>, f: impl FnOnce(&Option<T>) -> bool) -> Poll<Result<(), ()>> {
        if Rc::strong_count(self) != 2 {
            return Poll::Ready(Err(()));
        }

        unsafe {
            // SAFETY: the Cell is non-Send
            if !f(&*self.value.get()) {
                return Poll::Pending;
            }

            Poll::Ready(Ok(()))
        }
    }
}

pub fn new<T>() -> (Sender<T>, Receiver<T>) {
    let shared = Rc::new(Shared {
        value: UnsafeCell::new(None),
    });
    (
        Sender {
            shared: shared.clone(),
        },
        Receiver { shared },
    )
}

pub struct Sender<T> {
    shared: Rc<Shared<T>>,
}

impl<T> super::super::Sender<T> for Sender<T> {
    #[inline(always)]
    fn poll_send(
        &mut self,
        _cx: &mut core::task::Context<'_>,
        value: &mut core::mem::MaybeUninit<T>,
    ) -> Poll<Result<(), ()>> {
        match self.shared.poll(|v| v.is_none()) {
            Poll::Ready(Ok(())) => {
                unsafe {
                    // SAFETY: the Cell is non-Send and we just checked that it was empty
                    let cell = &mut *self.shared.value.get();
                    *cell = Some(value.assume_init_read());
                }
                Poll::Ready(Ok(()))
            }
            Poll::Ready(Err(())) => Poll::Ready(Err(())),
            Poll::Pending => Poll::Pending,
        }
    }
}

pub struct Receiver<T> {
    shared: Rc<Shared<T>>,
}

impl<T> super::super::Receiver<T> for Receiver<T> {
    #[inline(always)]
    fn poll_recv(
        &mut self,
        _cx: &mut core::task::Context<'_>,
        budget: &mut super::super::Budget,
    ) -> core::task::Poll<Option<T>> {
        if budget.is_exhausted() {
            budget.set_needs_wake();
            return Poll::Pending;
        }

        match self.shared.poll(|v| v.is_some()) {
            Poll::Ready(Ok(())) => {
                budget.consume();
                unsafe {
                    // SAFETY: the Cell is non-Send and we just checked that it was non-empty
                    Poll::Ready(core::mem::take(&mut *self.shared.value.get()))
                }
            }
            Poll::Ready(Err(())) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }

    #[inline(always)]
    fn on_consumed(&mut self, _bytes: u64) {}
}

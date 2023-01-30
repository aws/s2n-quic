// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#[cfg(any(test, kani))]
mod kani {
    #![allow(dead_code)]

    use core::{
        cell::{Cell, UnsafeCell},
        task::Waker,
    };

    #[derive(Debug, Default)]
    pub struct AtomicBool(Cell<bool>);

    impl AtomicBool {
        pub const fn new(value: bool) -> Self {
            Self(Cell::new(value))
        }

        pub fn load(&self, _order: Ordering) -> bool {
            self.0.get()
        }

        pub fn swap(&self, value: bool, _order: Ordering) -> bool {
            self.0.replace(value)
        }
    }

    #[derive(Debug, Default)]
    pub struct AtomicUsize(Cell<usize>);

    impl AtomicUsize {
        pub const fn new(value: usize) -> Self {
            Self(Cell::new(value))
        }

        pub fn store(&self, value: usize, _order: Ordering) {
            self.0.set(value)
        }

        pub fn load(&self, _order: Ordering) -> usize {
            self.0.get()
        }
    }

    pub use ::core::sync::atomic::Ordering;

    #[derive(Debug, Default)]
    pub struct AtomicWaker(UnsafeCell<Option<Waker>>);

    impl AtomicWaker {
        pub const fn new() -> Self {
            Self(UnsafeCell::new(None))
        }

        pub fn wake(&self) {
            if let Some(waker) = self.take() {
                waker.wake();
            }
        }

        pub fn take(&self) -> Option<Waker> {
            unsafe { core::mem::take(&mut *self.0.get()) }
        }

        pub fn register(&self, waker: &Waker) {
            let cell = unsafe { &mut *self.0.get() };
            *cell = Some(waker.clone());
        }
    }
}

#[cfg(kani)]
pub use self::kani::*;

#[cfg(loom)]
mod loom {
    use ::core::task::Waker;
    use ::loom::future::AtomicWaker as Inner;

    pub use ::loom::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    #[derive(Debug, Default)]
    pub struct AtomicWaker(Inner);

    impl AtomicWaker {
        pub fn new() -> Self {
            Self(Inner::new())
        }

        pub fn wake(&self) {
            self.0.wake();
        }

        pub fn take(&self) -> Option<Waker> {
            self.0.take_waker()
        }

        pub fn register(&self, waker: &Waker) {
            self.0.register_by_ref(waker);
        }
    }
}

#[cfg(loom)]
pub use self::loom::*;

#[cfg(not(kani))]
mod core {
    pub use ::core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    pub use atomic_waker::AtomicWaker;
}

#[cfg(not(any(kani, loom)))]
pub use self::core::*;

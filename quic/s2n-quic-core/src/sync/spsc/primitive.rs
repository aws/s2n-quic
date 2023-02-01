// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#[cfg(all(loom, test))]
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

#[cfg(all(loom, test))]
pub use self::loom::*;

mod core {
    pub use ::core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    pub use atomic_waker::AtomicWaker;
}

#[cfg(not(all(loom, test)))]
pub use self::core::*;

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#[cfg(all(loom, test))]
mod loom_primitive {
    use ::core::task::Waker;
    use ::loom::future::AtomicWaker as Inner;

    pub use ::loom::sync::{atomic::*, Arc};

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
pub use self::loom_primitive::*;

mod core_primitive {
    pub use ::core::sync::atomic::*;
    pub use alloc::sync::Arc;
    pub use atomic_waker::AtomicWaker;
}

#[cfg(not(all(loom, test)))]
pub use self::core_primitive::*;

/// Indicates if the type is a zero-sized type
///
/// This can be used to optimize the code to avoid needless calculations.
pub trait IsZst {
    const IS_ZST: bool;
}

impl<T> IsZst for T {
    const IS_ZST: bool = ::core::mem::size_of::<T>() == 0;
}

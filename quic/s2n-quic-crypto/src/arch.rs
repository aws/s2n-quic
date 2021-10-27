// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use cfg_if::cfg_if;

cfg_if! {
    if #[cfg(target_arch = "x86")] {
        pub use core::arch::x86::::*;
        mod x86;
        pub use x86::*;
    } else if #[cfg(target_arch = "x86_64")] {
        pub use core::arch::x86_64::*;
        mod x86;
        pub use x86::*;
    }
}

pub trait Arch {
    fn is_supported() -> bool;

    unsafe fn call<F: FnOnce() -> R, R>(f: F) -> R;

    #[inline]
    fn call_supported<F: FnOnce()>(f: F) {
        if Self::is_supported() {
            unsafe {
                // Safety: this is only called if the arch is supported
                Self::call(f)
            }
        }
    }
}

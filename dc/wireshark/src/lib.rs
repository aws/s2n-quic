// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

mod buffer;
mod dissect;
mod field;
#[cfg(not(test))]
mod plugin;
mod value;
/// This wraps the underlying sys APIs in structures that support a cfg(test) mode that doesn't rely on Wireshark.
mod wireshark;

/// These are bindgen-generated bindings from bindgen 4.2.5.
/// Allow warnings since we don't control the bindgen generation process enough for warnings to be worthwhile to fix.
#[allow(warnings)]
mod wireshark_sys {
    #[cfg(test)]
    #[path = "minimal.rs"]
    mod sys_impl;

    #[cfg(not(test))]
    #[path = "full.rs"]
    mod sys_impl;

    pub use sys_impl::*;
}

#[cfg(test)]
mod test;

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#![allow(unused_macros)]

/// Calls the given libc function and wraps the result in an `io::Result`.
macro_rules! libc {
    ($fn: ident ( $($arg: expr),* $(,)* ) ) => {{
        let res = unsafe { libc::$fn($($arg, )*) };
        if res < 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(res)
        }
    }};
}

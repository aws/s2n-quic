// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Detects platform capability of:
//! * https://learn.microsoft.com/en-us/windows/win32/api/winsock2/nf-winsock2-wsasendmsg

fn main() {
    println!("WSASendMsg {:?}", unsafe { WSASENDMSG });
}

/// Try to resolve the required references from the linker
///
/// The build will fail if they don't exist.
#[cfg(all(windows, feature = "std"))]
#[link(name = "ws2_32")]
extern "system" {
    #[link_name = "WSASendMsg"]
    static WSASENDMSG: *const u8;
}

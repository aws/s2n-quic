// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use s2n_quic_core::ensure;
use std::{io, os::fd::AsRawFd};

pub mod tcp;
pub mod udp;

pub type Flags = libc::c_int;

#[inline]
pub fn peek<T>(fd: &T) -> io::Result<usize>
where
    T: AsRawFd,
{
    libc_call(|| unsafe {
        let flags = libc::MSG_PEEK | libc::MSG_TRUNC;

        // macos doesn't seem to support MSG_TRUNC so we need to give it at least 1 byte
        if cfg!(target_os = "macos") {
            let mut buf = [0u8];
            libc::recv(fd.as_raw_fd(), buf.as_mut_ptr() as *mut _, 1, flags) as _
        } else {
            libc::recv(fd.as_raw_fd(), core::ptr::null_mut(), 0, flags) as _
        }
    })
}

#[inline]
pub fn libc_call(call: impl FnOnce() -> isize) -> io::Result<usize> {
    let res = call();

    ensure!(res >= 0, Err(io::Error::last_os_error()));

    Ok(res as _)
}

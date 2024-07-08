// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{libc_call, Flags};
use std::{
    io::{self, IoSlice, IoSliceMut},
    os::fd::AsRawFd,
};

pub use super::peek;

/// Receives segments on the provided socket
#[inline]
pub fn recv<T>(fd: &T, segments: &mut [IoSliceMut], flags: Flags) -> io::Result<usize>
where
    T: AsRawFd,
{
    let fd = fd.as_raw_fd();

    // if we only have a single segment then use recv, which should be slightly cheaper than recvmsg
    libc_call(|| if segments.len() == 1 {
        let segment: &mut [u8] = &mut segments[0];
        let buf = segment.as_ptr() as *mut _;
        let len = segment.len() as _;
        unsafe { libc::recv(fd, buf, len, flags) }
    } else {
        let mut msg = unsafe { core::mem::zeroed::<libc::msghdr>() };

        msg.msg_iov = segments.as_mut_ptr() as *mut _;
        msg.msg_iovlen = segments.len() as _;

        unsafe { libc::recvmsg(fd.as_raw_fd(), &mut msg, flags) }
    } as _)
}

/// Sends segments on the provided socket
#[inline]
pub fn send<T>(fd: &T, segments: &[IoSlice]) -> io::Result<usize>
where
    T: AsRawFd,
{
    debug_assert!(!segments.is_empty());

    let fd = fd.as_raw_fd();
    let flags = Flags::default();

    // if we only have a single segment then use send, which should be slightly cheaper than sendmsg
    libc_call(|| if segments.len() == 1 {
        let segment: &[u8] = &segments[0];
        let buf = segment.as_ptr() as *const _;
        let len = segment.len() as _;
        unsafe { libc::send(fd, buf, len, flags) }
    } else {
        let mut msg = unsafe { core::mem::zeroed::<libc::msghdr>() };

        // msghdr wants a `*mut iovec` but it doesn't actually end up mutating it
        msg.msg_iov = segments.as_ptr() as *mut IoSlice as *mut _;
        msg.msg_iovlen = segments.len() as _;

        unsafe { libc::sendmsg(fd, &msg, flags) }
    } as _)
}

#[inline]
pub fn shutdown<T>(fd: &T) -> io::Result<()>
where
    T: AsRawFd,
{
    libc_call(|| unsafe { libc::shutdown(fd.as_raw_fd(), libc::SHUT_WR) as _ })?;
    Ok(())
}

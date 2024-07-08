// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{libc_call, Flags};
use crate::msg::{
    addr::Addr,
    cmsg::{self, Encoder},
};
use s2n_quic_core::inet::ExplicitCongestionNotification;
use std::{
    io::{self, IoSlice, IoSliceMut},
    os::fd::AsRawFd,
};

pub use super::peek;

#[inline]
pub fn recv<T>(
    fd: &T,
    addr: &mut Addr,
    cmsg: &mut cmsg::Receiver,
    buffer: &mut [IoSliceMut],
    flags: Flags,
) -> io::Result<usize>
where
    T: AsRawFd,
{
    recv_msghdr(addr, cmsg, buffer, |msghdr| {
        libc_call(|| unsafe { libc::recvmsg(fd.as_raw_fd(), msghdr, flags) as _ })
    })
}

/// Constructs a msghdr for receiving
#[inline]
fn recv_msghdr(
    addr: &mut Addr,
    cmsg: &mut cmsg::Receiver,
    segments: &mut [IoSliceMut],
    exec: impl FnOnce(&mut libc::msghdr) -> io::Result<usize>,
) -> io::Result<usize> {
    debug_assert!(!segments.is_empty());

    let mut msg = unsafe { core::mem::zeroed::<libc::msghdr>() };

    addr.recv_with_msg(&mut msg);

    // setup cmsg info
    let mut cmsg_storage = cmsg::Storage::<{ cmsg::DECODER_LEN }>::default();

    msg.msg_control = cmsg_storage.as_mut_ptr() as *mut _;
    msg.msg_controllen = cmsg_storage.len() as _;

    msg.msg_iov = segments.as_ptr() as *mut IoSliceMut as *mut _;
    msg.msg_iovlen = segments.len() as _;

    let len = exec(&mut msg)?;

    cmsg.with_msg(&msg);

    Ok(len)
}

#[inline]
pub fn send<T>(
    fd: &T,
    addr: &Addr,
    ecn: ExplicitCongestionNotification,
    buffer: &[IoSlice],
) -> io::Result<usize>
where
    T: AsRawFd,
{
    send_msghdr(addr, ecn, buffer, |msghdr| {
        libc_call(|| unsafe { libc::sendmsg(fd.as_raw_fd(), msghdr, 0) as _ })
    })
}

/// Constructs a msghdr for sending
#[inline]
fn send_msghdr(
    addr: &Addr,
    ecn: ExplicitCongestionNotification,
    segments: &[IoSlice],
    exec: impl FnOnce(&libc::msghdr) -> io::Result<usize>,
) -> io::Result<usize> {
    debug_assert!(!segments.is_empty());

    let mut msg = unsafe { core::mem::zeroed::<libc::msghdr>() };

    addr.send_with_msg(&mut msg);

    // make sure we constructed a valid iovec
    #[cfg(debug_assertions)]
    check_send_iovec(segments);

    // setup cmsg info
    let mut cmsg_storage = cmsg::Storage::<{ cmsg::ENCODER_LEN }>::default();
    let mut cmsg = cmsg_storage.encoder();
    if ecn != ExplicitCongestionNotification::NotEct {
        // TODO enable this once we consolidate s2n-quic-core crates
        // let _ = cmsg.encode_ecn(ecn, &addr);
    }

    if segments.len() > 1 {
        let _ = cmsg.encode_gso(segments[0].len() as _);
    }

    if !cmsg.is_empty() {
        msg.msg_control = cmsg.as_mut_ptr() as *mut _;
        msg.msg_controllen = cmsg.len() as _;
    }

    msg.msg_iov = segments.as_ptr() as *mut IoSlice as *mut _;
    msg.msg_iovlen = segments.len() as _;

    exec(&mut msg)
}

#[cfg(debug_assertions)]
fn check_send_iovec<T>(segments: &[T])
where
    T: core::ops::Deref<Target = [u8]>,
{
    let mut total_len = 0;
    let mut segment_size = None;
    let mut can_accept_more = true;

    for segment in segments {
        assert!(can_accept_more);

        if let Some(expected_len) = segment_size {
            assert!(expected_len >= segment.len());
            // we can only have more segments if the current one matches the previous
            can_accept_more = expected_len == segment.len();
        } else {
            segment_size = Some(segment.len());
        }
        total_len += segment.len();
    }

    assert!(
        total_len <= u16::MAX as usize,
        "payloads should not exceed 2^16"
    );
}

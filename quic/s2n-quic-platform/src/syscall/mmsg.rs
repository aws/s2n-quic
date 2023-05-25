// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#![allow(dead_code)] // TODO remove once used

use super::{SocketEvents, SocketType};
use libc::mmsghdr;
use std::os::unix::io::AsRawFd;

#[inline]
pub fn send<Sock: AsRawFd, E: SocketEvents>(
    socket: &Sock,
    packets: &mut [mmsghdr],
    events: &mut E,
) {
    if packets.is_empty() {
        return;
    }

    // Safety: calling a libc function is inherently unsafe as rust cannot
    // make any invariant guarantees. This has to be reviewed by humans instead
    // so the [docs](https://linux.die.net/man/2/sendmmsg) are inlined here:

    // > The sockfd argument is the file descriptor of the socket on which data
    // > is to be transmitted.
    let sockfd = socket.as_raw_fd();

    // > The msgvec argument is a pointer to an array of mmsghdr structures.
    //
    // > The msg_hdr field is a msghdr structure, as described in sendmsg(2).
    // > The msg_len field is used to return the number of bytes sent from the
    // > message in msg_hdr.
    let msgvec = packets.as_mut_ptr();

    // > The size of this array is specified in vlen.
    //
    // > The value specified in vlen is capped to UIO_MAXIOV (1024).
    let vlen = packets.len().min(1024) as _;

    // > The flags argument contains flags ORed together.
    //
    // No flags are currently set
    let flags = Default::default();

    // > The sendmmsg() system call is an extension of sendmsg(2) that allows
    // > the caller to transmit multiple messages on a socket using a single
    // > system call. (This has performance benefits for some applications.)
    //
    // > A nonblocking call sends as many messages as possible (up to the limit
    // > specified by vlen) and returns immediately.
    //
    // > On return from sendmmsg(), the msg_len fields of successive elements
    // > of msgvec are updated to contain the number of bytes transmitted from
    // > the corresponding msg_hdr. The return value of the call indicates the
    // > number of elements of msgvec that have been updated.
    //
    // > On success, sendmmsg() returns the number of messages sent from msgvec;
    // > if this is less than vlen, the caller can retry with a further sendmmsg()
    // > call to send the remaining messages.
    //
    // > On error, -1 is returned, and errno is set to indicate the error.

    let res = libc!(sendmmsg(sockfd, msgvec, vlen, flags));

    let _ = match res {
        Ok(count) => events.on_complete(count as _),
        Err(error) => events.on_error(error),
    };
}

#[inline]
pub fn recv<Sock: AsRawFd, E: SocketEvents>(
    socket: &Sock,
    socket_type: SocketType,
    packets: &mut [mmsghdr],
    events: &mut E,
) {
    if packets.is_empty() {
        return;
    }

    // Safety: calling a libc function is inherently unsafe as rust cannot
    // make any invariant guarantees. This has to be reviewed by humans instead
    // so the [docs](https://linux.die.net/man/2/recvmmsg) are inlined here:

    // > The sockfd argument is the file descriptor of the socket to receive data from.
    let sockfd = socket.as_raw_fd();

    // > The msgvec argument is a pointer to an array of mmsghdr structures.
    //
    // > The msg_len field is the number of bytes returned for the message in the entry.
    let msgvec = packets.as_mut_ptr();

    // > The size of this array is specified in vlen.
    let vlen = packets.len() as _;

    // > The flags argument contains flags ORed together.
    //
    // If the socket is blocking, set the MSG_WAITFORONE flag so we don't hang until the entire
    // buffer is full.
    let flags = match socket_type {
        SocketType::Blocking => libc::MSG_WAITFORONE,
        SocketType::NonBlocking => libc::MSG_DONTWAIT,
    };

    // > The timeout argument points to a struct timespec defining a timeout
    // > (seconds plus nanoseconds) for the receive operation.
    //
    // Since we currently only use non-blocking sockets, this isn't needed.
    // If support is added for non-blocking sockets, this will need to be
    // updated.
    let timeout = core::ptr::null_mut();

    // > The recvmmsg() system call is an extension of recvmsg(2)
    // > that allows the caller to receive multiple messages from a
    // > socket using a single system call.
    //
    // > A nonblocking call reads as many messages as are available
    // > (up to the limit specified by vlen) and returns immediately.
    //
    // > On return from recvmmsg(), successive elements of msgvec are
    // > updated to contain information about each received message:
    // > msg_len contains the size of the received message;
    // > the subfields of msg_hdr are updated as described in recvmsg(2).
    // > The return value of the call indicates the number of elements of
    // > msgvec that have been updated.
    //
    // > On success, recvmmsg() returns the number of messages received in
    // > msgvec; on error, -1 is returned, and errno is set to indicate the error.
    let res = libc!(recvmmsg(sockfd, msgvec, vlen, flags, timeout));

    let _ = match res {
        Ok(count) => events.on_complete(count as _),
        Err(error) => events.on_error(error),
    };
}

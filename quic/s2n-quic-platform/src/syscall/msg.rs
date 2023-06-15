// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#![allow(dead_code)] // TODO remove once used

use super::{SocketEvents, SocketType};
use libc::msghdr;
use std::os::unix::io::AsRawFd;

#[inline]
pub fn send<'a, Sock: AsRawFd, P: IntoIterator<Item = &'a mut msghdr>, E: SocketEvents>(
    socket: &Sock,
    packets: P,
    events: &mut E,
) {
    for packet in packets {
        #[cfg(debug_assertions)]
        let prev_msg_control_ptr = packet.msg_control;

        // macOS doesn't like when msg_control have valid pointers but the len is 0
        //
        // If that's the case here, then set the `msg_control` to null and restore it after
        // calling sendmsg.
        #[cfg(any(target_os = "macos", target_os = "ios", test))]
        let msg_control = {
            let msg_control = packet.msg_control;

            if packet.msg_controllen == 0 {
                packet.msg_control = core::ptr::null_mut();
            }

            msg_control
        };

        // Safety: calling a libc function is inherently unsafe as rust cannot
        // make any invariant guarantees. This has to be reviewed by humans instead
        // so the [docs](https://linux.die.net/man/2/sendmsg) are inlined here:

        // > The argument sockfd is the file descriptor of the sending socket.
        let sockfd = socket.as_raw_fd();

        // > The address of the target is given by msg.msg_name, with msg.msg_namelen
        // > specifying its size.
        //
        // > The message is pointed to by the elements of the array msg.msg_iov.
        // > The sendmsg() call also allows sending ancillary data (also known as
        // > control information).
        let msg = packet;

        // > The flags argument is the bitwise OR of zero or more flags.
        //
        // No flags are currently set
        let flags = Default::default();

        // > On success, these calls return the number of characters sent.
        // > On error, -1 is returned, and errno is set appropriately.
        let result = libc!(sendmsg(sockfd, msg, flags));

        // restore the msg_control pointer if needed
        #[cfg(any(target_os = "macos", target_os = "ios", test))]
        {
            msg.msg_control = msg_control;
        }

        #[cfg(debug_assertions)]
        {
            assert_eq!(
                prev_msg_control_ptr, msg.msg_control,
                "msg_control pointer was modified by the OS"
            );
        }

        let cf = match result {
            Ok(_) => events.on_complete(1),
            Err(err) => events.on_error(err),
        };

        if cf.is_break() {
            return;
        }
    }
}

#[inline]
pub fn recv<'a, Sock: AsRawFd, P: IntoIterator<Item = &'a mut msghdr>, E: SocketEvents>(
    socket: &Sock,
    socket_type: SocketType,
    packets: P,
    events: &mut E,
) {
    let mut flags = match socket_type {
        SocketType::Blocking => Default::default(),
        SocketType::NonBlocking => libc::MSG_DONTWAIT,
    };

    for packet in packets {
        #[cfg(debug_assertions)]
        let prev_msg_control_ptr = packet.msg_control;

        // Safety: calling a libc function is inherently unsafe as rust cannot
        // make any invariant guarantees. This has to be reviewed by humans instead
        // so the [docs](https://linux.die.net/man/2/recmsg) are inlined here:

        // > The argument sockfd is the file descriptor of the receiving socket.
        let sockfd = socket.as_raw_fd();

        // > The recvmsg() call uses a msghdr structure to minimize the number of
        // > directly supplied arguments.
        //
        // > Here msg_name and msg_namelen specify the source address if the
        // > socket is unconnected.
        //
        // > The fields msg_iov and msg_iovlen describe scatter-gather locations
        //
        // > When recvmsg() is called, msg_controllen should contain the length
        // > of the available buffer in msg_control; upon return from a successful
        // > call it will contain the length of the control message sequence.
        let msg = packet;

        // > The flags argument to a recv() call is formed by ORing one or more flags
        //
        // We set MSG_DONTWAIT if it's nonblocking or there is more than one call

        // > recvmsg() calls are used to receive messages from a socket
        //
        // > All three routines return the length of the message on successful completion.
        // > If a message is too long to fit in the supplied buffer, excess bytes may be
        // > discarded depending on the type of socket the message is received from.
        //
        // > These calls return the number of bytes received, or -1 if an error occurred.
        let result = libc!(recvmsg(sockfd, msg, flags));

        #[cfg(debug_assertions)]
        {
            assert_eq!(
                prev_msg_control_ptr, msg.msg_control,
                "msg_control pointer was modified by the OS"
            );
        }

        let cf = match result {
            Ok(_) => events.on_complete(1),
            Err(err) => events.on_error(err),
        };

        if cf.is_break() {
            return;
        }

        // don't block the follow-up calls
        flags = libc::MSG_DONTWAIT;
    }
}

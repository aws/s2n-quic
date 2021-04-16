// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    buffer::Buffer,
    message::{
        msg::{Message, Ring},
        queue, Message as _,
    },
};
use libc::{recvmsg, sendmsg};
use s2n_quic_core::io::{rx, tx};
use std::{io, os::unix::io::AsRawFd};

#[derive(Debug, Default)]
pub struct Queue<B: Buffer>(queue::Queue<Ring<B>>);

impl<B: Buffer> Queue<B> {
    pub fn new(buffer: B) -> Self {
        let queue = queue::Queue::new(Ring::new(buffer));

        Self(queue)
    }

    pub fn free_len(&self) -> usize {
        self.0.free_len()
    }

    pub fn occupied_len(&self) -> usize {
        self.0.occupied_len()
    }

    pub fn tx<Socket: AsRawFd>(&mut self, socket: &Socket) -> io::Result<usize> {
        let mut count = 0;
        let mut entries = self.0.occupied_mut();

        for entry in entries.iter_mut() {
            unsafe {
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
                let msg = entry.as_mut_ptr() as _;

                // > The flags argument is the bitwise OR of zero or more flags.
                //
                // No flags are currently set
                let flags = Default::default();

                // > On success, these calls return the number of characters sent.
                // > On error, -1 is returned, and errno is set appropriately.
                match sendmsg(sockfd, msg, flags) {
                    status if status >= 0 => {
                        count += 1;
                    }
                    _ => {
                        let err = io::Error::last_os_error();

                        // TODO handle EMSGSIZE
                        // > By default, Linux UDP does path MTU (Maximum Transmission Unit)
                        // > discovery.  This means the kernel will keep track of the MTU to a
                        // > specific target IP address and return EMSGSIZE when a UDP packet
                        // > write exceeds it.

                        if count > 0 && err.kind() == io::ErrorKind::WouldBlock {
                            break;
                        } else {
                            entries.finish(count);
                            return Err(err);
                        }
                    }
                }
            }
        }

        entries.finish(count);

        Ok(count)
    }

    pub fn rx<Socket: AsRawFd>(&mut self, socket: &Socket) -> io::Result<usize> {
        let mut count = 0;
        let mut entries = self.0.free_mut();

        for entry in entries.as_mut() {
            unsafe {
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
                let msg = entry.as_mut_ptr() as _;

                // > The flags argument to a recv() call is formed by ORing one or more flags
                //
                // No flags are currently set
                let flags = Default::default();

                // > recvmsg() calls are used to receive messages from a socket
                //
                // > All three routines return the length of the message on successful completion.
                // > If a message is too long to fit in the supplied buffer, excess bytes may be
                // > discarded depending on the type of socket the message is received from.
                //
                // > These calls return the number of bytes received, or -1 if an error occurred.
                match recvmsg(sockfd, msg, flags) {
                    len if len >= 0 => {
                        let len = len as usize;

                        // ensure the returned length does not exceed what is
                        // allocated
                        debug_assert!(len <= entry.payload_len(), "cannot exceed payload_len");
                        let len = len.min(entry.payload_len());
                        entry.set_payload_len(len);

                        count += 1;
                    }
                    _ => {
                        let err = io::Error::last_os_error();

                        if count > 0 && err.kind() == io::ErrorKind::WouldBlock {
                            break;
                        } else {
                            entries.finish(count);
                            return Err(err);
                        }
                    }
                }
            }
        }

        entries.finish(count);

        Ok(count)
    }
}

impl<'a, B: Buffer> tx::Tx<'a> for Queue<B> {
    type Queue = queue::Free<'a, Message>;

    fn queue(&'a mut self) -> Self::Queue {
        self.0.free_mut()
    }

    fn len(&self) -> usize {
        self.0.free_len()
    }
}

impl<'a, B: Buffer> rx::Rx<'a> for Queue<B> {
    type Queue = queue::OccupiedWipe<'a, Message>;

    fn queue(&'a mut self) -> Self::Queue {
        self.0.occupied_wipe_mut()
    }

    fn len(&self) -> usize {
        self.0.occupied_len()
    }
}

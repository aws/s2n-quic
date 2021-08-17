// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    buffer::Buffer,
    message::{
        msg::{self, Message, Ring},
        queue, Message as _,
    },
};
use std::{io, os::unix::io::AsRawFd};

#[derive(Debug, Default)]
pub struct Queue<B: Buffer>(queue::Queue<Ring<B>>);

pub use msg::Handle;

impl<B: Buffer> Queue<B> {
    pub fn new(buffer: B, max_segments: usize) -> Self {
        let queue = queue::Queue::new(Ring::new(buffer, max_segments));

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
            match libc!(sendmsg(sockfd, msg, flags)) {
                Ok(_len) => {
                    count += 1;
                }
                Err(err) if count > 0 && err.kind() == io::ErrorKind::WouldBlock => {
                    break;
                }
                Err(err) if err.kind() == io::ErrorKind::WouldBlock => {
                    entries.finish(count);
                    return Err(err);
                }
                Err(err) if err.kind() == io::ErrorKind::Interrupted => {
                    break;
                }
                // check to see if we need to disable GSO
                #[cfg(s2n_quic_platform_gso)]
                Err(err) if unsafe { *libc::__errno_location() } == libc::EIO => {
                    // unfortunately we've already assembled GSO packets so just drop them
                    // and wait for a retransmission
                    let len = entries.len();
                    entries.finish(len);

                    if self.0.max_gso() > 1 {
                        self.0.disable_gso();
                        return Ok(count);
                    } else {
                        return Err(err);
                    }
                }
                Err(_) => {
                    // Ignore other transmission errors
                    // - Permissions issues are observed in case of unsuitable iptable
                    //   rules. Those can be changed while the application is running.
                    // - Network unreachable errors can be observed for certain
                    //   destination addresses.
                    //
                    // TODO: This error should potentially be logged - but in a debounced fashion
                    count += 1;
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
            match libc!(recvmsg(sockfd, msg, flags)) {
                Ok(len) => {
                    let len = len as usize;

                    // ensure the returned length does not exceed what is
                    // allocated
                    debug_assert!(len <= entry.payload_len(), "cannot exceed payload_len");
                    let len = len.min(entry.payload_len());
                    unsafe {
                        // Safety: len will always be <= the initially allocated size
                        entry.set_payload_len(len);
                    }

                    count += 1;
                }
                Err(err) if err.kind() == io::ErrorKind::Interrupted => {
                    break;
                }
                Err(err) => {
                    if count > 0 && err.kind() == io::ErrorKind::WouldBlock {
                        break;
                    } else {
                        entries.finish(count);
                        return Err(err);
                    }
                }
            }
        }

        entries.finish(count);

        Ok(count)
    }

    pub fn rx_queue(&mut self) -> queue::OccupiedWipe<Message> {
        self.0.occupied_wipe_mut()
    }

    pub fn tx_queue(&mut self) -> queue::Free<Message> {
        self.0.free_mut()
    }
}

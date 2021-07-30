// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    buffer::Buffer,
    message::{
        mmsg::{Message, Ring},
        queue,
    },
};
use std::{io, os::unix::io::AsRawFd};

#[derive(Debug, Default)]
pub struct Queue<B: Buffer>(queue::Queue<Ring<B>>);

impl<B: Buffer> Queue<B> {
    pub fn new(buffer: B, max_gso: usize) -> Self {
        let queue = queue::Queue::new(Ring::new(buffer, max_gso));

        Self(queue)
    }

    pub fn free_len(&self) -> usize {
        self.0.free_len()
    }

    pub fn occupied_len(&self) -> usize {
        self.0.occupied_len()
    }

    pub fn tx<Socket: AsRawFd>(&mut self, socket: &Socket) -> io::Result<usize> {
        let mut entries = self.0.occupied_mut();

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
        let msgvec = entries.as_mut_ptr() as _;

        // > The size of this array is specified in vlen.
        //
        // > The value specified in vlen is capped to UIO_MAXIOV (1024).
        let vlen = entries.len().min(1024) as _;

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
        match libc!(sendmmsg(sockfd, msgvec, vlen, flags)) {
            Ok(status) => {
                let count = status as usize;
                entries.finish(count);
                Ok(count)
            }
            Err(err) if err.kind() == io::ErrorKind::Interrupted => {
                entries.cancel(0);
                Ok(0)
            }
            Err(err) if err.kind() == io::ErrorKind::PermissionDenied => {
                // just drop the packets on permission errors - most likely a firewall issue
                let count = vlen as usize;
                entries.finish(count);
                Ok(count)
            }
            // check to see if we need to disable GSO
            #[cfg(target_os = "linux")]
            Err(err) if unsafe { *libc::__errno_location() } == libc::EIO => {
                let count = vlen as usize;
                entries.finish(count);

                if self.0.max_gso() > 1 {
                    self.0.disable_gso();
                    // unfortunately we've already assembled GSO packets so just drop them
                    // and wait for a retransmission
                    Ok(count)
                } else {
                    Err(err)
                }
            }
            Err(err) => {
                entries.cancel(0);
                Err(err)
            }
        }
    }

    pub fn rx<Socket: AsRawFd>(&mut self, socket: &Socket) -> io::Result<usize> {
        let mut entries = self.0.free_mut();

        if entries.is_empty() {
            return Ok(0);
        }

        // Safety: calling a libc function is inherently unsafe as rust cannot
        // make any invariant guarantees. This has to be reviewed by humans instead
        // so the [docs](https://linux.die.net/man/2/recvmmsg) are inlined here:

        // > The sockfd argument is the file descriptor of the socket to receive data from.
        let sockfd = socket.as_raw_fd();

        // > The msgvec argument is a pointer to an array of mmsghdr structures.
        //
        // > The msg_len field is the number of bytes returned for the message in the entry.
        let msgvec = entries.as_mut_ptr() as _;

        // > The size of this array is specified in vlen.
        let vlen = entries.len() as _;

        // > The flags argument contains flags ORed together.
        //
        // No flags are currently set
        let flags = Default::default();

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
        match libc!(recvmmsg(sockfd, msgvec, vlen, flags, timeout)) {
            Ok(status) => {
                let count = status as usize;
                entries.finish(count);
                Ok(count)
            }
            Err(err) if err.kind() == io::ErrorKind::Interrupted => {
                entries.cancel(0);
                Ok(0)
            }
            Err(err) => {
                entries.cancel(0);
                Err(err)
            }
        }
    }

    pub fn rx_queue(&mut self) -> queue::OccupiedWipe<Message> {
        self.0.occupied_wipe_mut()
    }

    pub fn tx_queue(&mut self) -> queue::Free<Message> {
        self.0.free_mut()
    }
}

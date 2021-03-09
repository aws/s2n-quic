// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    buffer,
    message::{
        msg::{Message, Ring},
        queue, Message as MessageTrait,
    },
    socket::{self, raw::AsRaw},
};
use libc::sendmsg;
use s2n_quic_core::io::tx;
use std::io;

impl_io!(Tx);
impl_socket_raw_delegate!(
    impl[Buffer: buffer::Buffer, Socket: AsRaw] Tx<Buffer, Socket>,
    |self| &self.socket
);
impl_socket_mio_delegate!(
    impl[Buffer: buffer::Buffer, Socket: mio::Evented] Tx<Buffer, Socket>,
    |self| &self.socket
);

impl<'a, Buffer: buffer::Buffer, Socket: AsRaw + socket::Socket<Error = io::Error>> tx::Tx<'a>
    for Tx<Buffer, Socket>
{
    type Queue = queue::Free<'a, Message>;
    type Error = io::Error;

    fn queue(&'a mut self) -> Self::Queue {
        self.queue.free_mut()
    }

    fn len(&self) -> usize {
        self.queue.occupied_len()
    }

    fn poll_transmit(
        &mut self,
        cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<io::Result<usize>> {
        let Self { socket, queue } = self;

        socket.poll_transmit(cx, |socket| {
            let mut count = 0;
            let mut occupied = queue.occupied_mut();

            for entry in occupied.iter_mut() {
                unsafe {
                    // Safety: calling a libc function is inherently unsafe as rust cannot
                    // make any invariant guarantees. This has to be reviewed by humans instead
                    // so the [docs](https://linux.die.net/man/2/sendmsg) are inlined here:

                    // > The argument sockfd is the file descriptor of the sending socket.
                    let sockfd = socket.as_raw();

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
                                occupied.finish(count);
                                return Err(err);
                            }
                        }
                    }
                }
            }

            occupied.finish(count);

            Ok(count)
        })
    }
}

#[cfg(test)]
mod tests {
    impl_io_tx_tests!();
}

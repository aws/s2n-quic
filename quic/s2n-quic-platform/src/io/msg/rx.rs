use crate::{
    buffer,
    message::{
        msg::{Message, Ring},
        queue, Message as MessageTrait,
    },
    socket::{self, raw::AsRaw},
};
use libc::recvmsg;
use s2n_quic_core::io::rx;
use std::io;

impl_io!(Rx);
impl_socket_raw_delegate!(
    impl[Buffer: buffer::Buffer, Socket: AsRaw] Rx<Buffer, Socket>,
    |self| &self.socket
);
impl_socket_mio_delegate!(
    impl[Buffer: buffer::Buffer, Socket: mio::Evented] Rx<Buffer, Socket>,
    |self| &self.socket
);

impl<'a, Buffer: buffer::Buffer, Socket: AsRaw + socket::Socket<Error = io::Error>> rx::Rx<'a>
    for Rx<Buffer, Socket>
{
    type Queue = queue::OccupiedWipe<'a, Message>;
    type Error = io::Error;

    fn queue(&'a mut self) -> Self::Queue {
        self.queue.occupied_wipe_mut()
    }

    fn len(&self) -> usize {
        self.queue.free_len()
    }

    fn poll_receive(
        &mut self,
        cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<io::Result<usize>> {
        let Self { socket, queue } = self;

        socket.poll_receive(cx, |socket| {
            let mut count = 0;
            let mut free = queue.free_mut();

            for entry in free.as_mut() {
                unsafe {
                    // Safety: calling a libc function is inherently unsafe as rust cannot
                    // make any invariant guarantees. This has to be reviewed by humans instead
                    // so the [docs](https://linux.die.net/man/2/recmsg) are inlined here:

                    // > The argument sockfd is the file descriptor of the receiving socket.
                    let sockfd = socket.as_raw();

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
                                free.finish(count);
                                return Err(err);
                            }
                        }
                    }
                }
            }

            free.finish(count);

            Ok(count)
        })
    }
}

#[cfg(test)]
mod tests {
    impl_io_rx_tests!();
}

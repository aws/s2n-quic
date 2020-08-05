use crate::{
    buffer,
    message::{
        mmsg::{Message, Ring},
        queue,
    },
    socket::raw::AsRaw,
};
use libc::recvmmsg;
use s2n_quic_core::io::rx;
use std::io;

impl_io!(Rx);
impl_io_tokio!(Rx, receive);
impl_socket_raw_delegate!(
    impl[Buffer: buffer::Buffer, Socket: AsRaw] Rx<Buffer, Socket>,
    |self| &self.socket
);
impl_socket_mio_delegate!(
    impl[Buffer: buffer::Buffer, Socket: mio::Evented] Rx<Buffer, Socket>,
    |self| &self.socket
);

impl<'a, Buffer: buffer::Buffer, Socket: AsRaw> rx::Rx<'a> for Rx<Buffer, Socket> {
    type Queue = queue::Occupied<'a, Message>;
    type Error = io::Error;

    fn queue(&'a mut self) -> Self::Queue {
        self.queue.occupied_mut()
    }

    fn len(&self) -> usize {
        self.queue.free_len()
    }

    fn receive(&mut self) -> io::Result<usize> {
        let mut entries = self.queue.free_mut();

        if entries.is_empty() {
            return Ok(0);
        }

        unsafe {
            // Safety: calling a libc function is inherently unsafe as rust cannot
            // make any invariant guarantees. This has to be reviewed by humans instead
            // so the [docs](https://linux.die.net/man/2/recvmmsg) are inlined here:

            // > The sockfd argument is the file descriptor of the socket to receive data from.
            let sockfd = self.socket.as_raw();

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
            match recvmmsg(sockfd, msgvec, vlen, flags, timeout) {
                status if status >= 0 => {
                    let count = status as usize;
                    entries.finish(count);
                    Ok(count)
                }
                _ => {
                    entries.cancel(0);
                    Err(io::Error::last_os_error())
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    impl_io_rx_tests!();
}

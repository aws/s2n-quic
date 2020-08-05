use crate::{
    buffer,
    message::{
        mmsg::{Message, Ring},
        queue,
    },
    socket::raw::AsRaw,
};
use libc::sendmmsg;
use s2n_quic_core::io::tx;
use std::io;

impl_io!(Tx);
impl_io_tokio!(Tx, transmit);
impl_socket_raw_delegate!(
    impl[Buffer: buffer::Buffer, Socket: AsRaw] Tx<Buffer, Socket>,
    |self| &self.socket
);
impl_socket_mio_delegate!(
    impl[Buffer: buffer::Buffer, Socket: mio::Evented] Tx<Buffer, Socket>,
    |self| &self.socket
);

impl<'a, Buffer: buffer::Buffer, Socket: AsRaw> tx::Tx<'a> for Tx<Buffer, Socket> {
    type Queue = queue::Free<'a, Message>;
    type Error = io::Error;

    fn queue(&'a mut self) -> Self::Queue {
        self.queue.free_mut()
    }

    fn len(&self) -> usize {
        self.queue.occupied_len()
    }

    fn transmit(&mut self) -> io::Result<usize> {
        let mut entries = self.queue.occupied_mut();

        unsafe {
            // Safety: calling a libc function is inherently unsafe as rust cannot
            // make any invariant guarantees. This has to be reviewed by humans instead
            // so the [docs](https://linux.die.net/man/2/sendmmsg) are inlined here:

            // > The sockfd argument is the file descriptor of the socket on which data
            // > is to be transmitted.
            let sockfd = self.socket.as_raw();

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
            match sendmmsg(sockfd, msgvec, vlen, flags) {
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
    impl_io_tx_tests!();
}

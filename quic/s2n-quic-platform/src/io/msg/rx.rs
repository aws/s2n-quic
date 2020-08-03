use crate::{
    buffer,
    message::{
        msg::{Message, Ring},
        queue, Message as MessageTrait,
    },
    socket::raw::AsRaw,
};
use libc::recvmsg;
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
        let mut count = 0;
        let mut free = self.queue.free_mut();

        while let Some(entry) = free.get_mut(count) {
            unsafe {
                match recvmsg(self.socket.as_raw(), entry.as_mut_ptr() as _, 0) {
                    len if len >= 0 => {
                        entry.set_payload_len(len as usize);
                        count += 1;
                    }
                    _ => {
                        let err = io::Error::last_os_error();

                        if count > 0 && err.kind() == io::ErrorKind::WouldBlock {
                            break;
                        } else {
                            return Err(err);
                        }
                    }
                }
            }
        }

        free.finish(count);

        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    impl_io_rx_tests!();
}

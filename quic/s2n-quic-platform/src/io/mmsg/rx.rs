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
            match recvmmsg(
                self.socket.as_raw(),
                entries.as_mut_ptr() as _,
                entries.len() as _,
                0,
                core::ptr::null_mut(),
            ) {
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

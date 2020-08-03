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
            match sendmmsg(
                self.socket.as_raw(),
                entries.as_mut_ptr() as _,
                entries.len() as _,
                0,
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
    impl_io_tx_tests!();
}

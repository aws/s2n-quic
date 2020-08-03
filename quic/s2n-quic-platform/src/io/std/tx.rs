use crate::{
    buffer,
    message::{
        queue,
        simple::{Message, Ring},
        Message as MessageTrait,
    },
    socket,
};
use s2n_quic_core::io::tx;
use std::io;

impl_io!(Tx);
impl_io_tokio!(Tx, transmit);
impl_socket_raw_delegate!(
    impl[Buffer: buffer::Buffer, Socket: socket::raw::AsRaw] Tx<Buffer, Socket>,
    |self| &self.socket
);
impl_socket_mio_delegate!(
    impl[Buffer: buffer::Buffer, Socket: mio::Evented] Tx<Buffer, Socket>,
    |self| &self.socket
);

impl<'a, Buffer: buffer::Buffer, Socket: socket::Simple<Error = io::Error>> tx::Tx<'a>
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

    fn transmit(&mut self) -> io::Result<usize> {
        let mut count = 0;
        let mut occupied = self.queue.occupied_mut();

        while let Some(entry) = occupied.get_mut(count) {
            if let Some(remote_address) = entry.remote_address() {
                match self.socket.send_to(entry.payload_mut(), &remote_address) {
                    Ok(_) => {
                        count += 1;
                    }
                    Err(err) => {
                        if count > 0 && err.kind() == io::ErrorKind::WouldBlock {
                            break;
                        } else {
                            return Err(err);
                        }
                    }
                }
            }
        }

        occupied.finish(count);

        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    impl_io_tx_tests!();
}

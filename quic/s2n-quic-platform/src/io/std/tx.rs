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
impl_socket_raw_delegate!(
    impl[Buffer: buffer::Buffer, Socket: socket::raw::AsRaw] Tx<Buffer, Socket>,
    |self| &self.socket
);
impl_socket_mio_delegate!(
    impl[Buffer: buffer::Buffer, Socket: mio::Evented] Tx<Buffer, Socket>,
    |self| &self.socket
);

impl<
        'a,
        Buffer: buffer::Buffer,
        Socket: socket::Simple<Error = io::Error> + socket::Socket<Error = io::Error>,
    > tx::Tx<'a> for Tx<Buffer, Socket>
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

            for entry in occupied.as_mut() {
                if let Some(remote_address) = entry.remote_address() {
                    match socket.send_to(entry.payload_mut(), &remote_address) {
                        Ok(_) => {
                            count += 1;
                        }
                        Err(err) => {
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

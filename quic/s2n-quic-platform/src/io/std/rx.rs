// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    buffer,
    message::{
        queue,
        simple::{Message, Ring},
        Message as MessageTrait,
    },
    socket,
};
use s2n_quic_core::io::rx;
use std::io;

impl_io!(Rx);
impl_socket_raw_delegate!(
    impl[Buffer: buffer::Buffer, Socket: socket::raw::AsRaw] Rx<Buffer, Socket>,
    |self| &self.socket
);
impl_socket_mio_delegate!(
    impl[Buffer: buffer::Buffer, Socket: mio::Evented] Rx<Buffer, Socket>,
    |self| &self.socket
);

impl<
        'a,
        Buffer: buffer::Buffer,
        Socket: socket::Simple<Error = io::Error> + socket::Socket<Error = io::Error>,
    > rx::Rx<'a> for Rx<Buffer, Socket>
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

            while let Some(entry) = free.get_mut(count) {
                match socket.recv_from(entry.payload_mut()) {
                    Ok((payload_len, Some(remote_address))) => {
                        entry.set_remote_address(&remote_address);
                        unsafe {
                            // Safety: The payload_len should not be bigger than the number of
                            // allocated bytes.

                            debug_assert!(payload_len < entry.payload_len());
                            let payload_len = payload_len.min(entry.payload_len());

                            entry.set_payload_len(payload_len);
                        }
                        count += 1;
                    }
                    Ok((_payload_len, None)) => {}
                    Err(err) => {
                        if count > 0 && err.kind() == io::ErrorKind::WouldBlock {
                            break;
                        } else {
                            free.finish(count);
                            return Err(err);
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

use crate::{
    buffer::Buffer as MessageBuffer,
    io::{
        rx::RxQueue,
        socket::unix::{
            queue::{new as new_queue, MessageQueue},
            udp::UdpSocket,
        },
    },
};
use s2n_quic_core::{inet::DatagramInfo, time::Timestamp};
use std::io;

#[derive(Debug)]
pub struct RxBuffer<Buffer: MessageBuffer>(MessageQueue<Buffer>);

impl<Buffer: MessageBuffer> RxBuffer<Buffer> {
    pub fn new(buffer: Buffer) -> Self {
        RxBuffer(new_queue(buffer))
    }

    #[cfg(s2n_quic_platform_socket_mmsg)]
    pub(crate) fn sync(&mut self, _socket: &mut UdpSocket) -> io::Result<usize> {
        unimplemented!("this implementation is deprecated")
    }

    #[cfg(not(s2n_quic_platform_socket_mmsg))]
    pub(crate) fn sync(&mut self, _socket: &mut UdpSocket) -> io::Result<usize> {
        unimplemented!("this implementation is deprecated")
    }
}

impl<Buffer: MessageBuffer> RxQueue for RxBuffer<Buffer> {
    fn pop(&mut self, _timestamp: Timestamp) -> Option<(DatagramInfo, &mut [u8])> {
        unimplemented!("this trait is deprecated")
    }

    fn len(&self) -> usize {
        self.0.occupied_len()
    }
}

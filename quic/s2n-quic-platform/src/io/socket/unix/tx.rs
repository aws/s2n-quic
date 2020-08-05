use crate::{
    buffer::Buffer as MessageBuffer,
    io::{
        socket::unix::{
            queue::{new as new_queue, MessageQueue},
            udp::UdpSocket,
        },
        tx::{TxError, TxPayload, TxQueue},
    },
};
use s2n_quic_core::inet::{ExplicitCongestionNotification, SocketAddress};
use std::io;

#[derive(Debug)]
pub struct TxBuffer<Buffer: MessageBuffer>(MessageQueue<Buffer>);

impl<Buffer: MessageBuffer> TxBuffer<Buffer> {
    pub fn new(buffer: Buffer) -> Self {
        TxBuffer(new_queue(buffer))
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

impl<Buffer: MessageBuffer> TxQueue for TxBuffer<Buffer> {
    fn push<Payload: TxPayload>(
        &mut self,
        _remote_address: &SocketAddress,
        _ecn: ExplicitCongestionNotification,
        _payload: Payload,
    ) -> Result<usize, TxError> {
        unimplemented!("this trait is deprecated")
    }

    fn capacity(&self) -> usize {
        self.0.free_len()
    }

    fn len(&self) -> usize {
        self.0.occupied_len()
    }
}

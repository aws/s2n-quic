use crate::{
    buffer::Buffer as MessageBuffer,
    io::{
        rx::RxQueue as RxQueueTrait,
        socket::unix::{rx::RxBuffer, tx::TxBuffer},
        tx::{TxError, TxPayload, TxQueue as TxQueueTrait},
    },
};
use core::ops::{Deref, DerefMut};
use s2n_quic_core::{
    inet::{DatagramInfo, ExplicitCongestionNotification, SocketAddress},
    time::Timestamp,
};
use std::{io::Result as IOResult, net::ToSocketAddrs};

mod builder;
mod queue;
mod rx;
mod tx;
mod udp;

pub type RxQueue<Buffer> = RxBuffer<Buffer>;
pub type TxQueue<Buffer> = TxBuffer<Buffer>;
pub use builder::SocketBuilder;
pub use udp::UdpSocket;

#[derive(Debug)]
pub struct Socket<Tx: MessageBuffer, Rx: MessageBuffer> {
    tx_buffer: TxQueue<Tx>,
    rx_buffer: RxQueue<Rx>,
    socket: UdpSocket,
}

impl<Tx: MessageBuffer, Rx: MessageBuffer> Deref for Socket<Tx, Rx> {
    type Target = UdpSocket;

    fn deref(&self) -> &Self::Target {
        &self.socket
    }
}

impl<Tx: MessageBuffer, Rx: MessageBuffer> DerefMut for Socket<Tx, Rx> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.socket
    }
}

impl<Tx: MessageBuffer, Rx: MessageBuffer> Socket<Tx, Rx> {
    pub fn build<Addrs: ToSocketAddrs>(addr: Addrs) -> IOResult<SocketBuilder<(), ()>> {
        SocketBuilder::new(addr)
    }

    pub fn bind<Addrs: ToSocketAddrs>(addr: Addrs, tx_buffer: Tx, rx_buffer: Rx) -> IOResult<Self> {
        SocketBuilder::new(addr)?
            .tx_buffer(tx_buffer)
            .rx_buffer(rx_buffer)
            .build()
    }

    pub fn split_mut(&mut self) -> (&mut TxQueue<Tx>, &mut RxQueue<Rx>) {
        (&mut self.tx_buffer, &mut self.rx_buffer)
    }

    pub fn sync(&mut self) -> IOResult<(usize, usize)> {
        let transmissions = self.sync_tx()?;
        let receptions = self.sync_rx()?;
        Ok((transmissions, receptions))
    }

    pub fn sync_tx(&mut self) -> IOResult<usize> {
        self.tx_buffer.sync(&mut self.socket)
    }

    pub fn sync_rx(&mut self) -> IOResult<usize> {
        self.rx_buffer.sync(&mut self.socket)
    }

    #[cfg(not(feature = "mio"))]
    pub fn set_nonblocking(&mut self, nonblocking: bool) -> IOResult<()> {
        self.socket.set_nonblocking(nonblocking)
    }
}

impl<Tx: MessageBuffer, Rx: MessageBuffer> TxQueueTrait for Socket<Tx, Rx> {
    fn push<Payload: TxPayload>(
        &mut self,
        remote_address: &SocketAddress,
        ecn: ExplicitCongestionNotification,
        payload: Payload,
    ) -> Result<usize, TxError> {
        self.tx_buffer.push(remote_address, ecn, payload)
    }

    fn capacity(&self) -> usize {
        self.tx_buffer.capacity()
    }

    fn len(&self) -> usize {
        self.tx_buffer.len()
    }
}

impl<Tx: MessageBuffer, Rx: MessageBuffer> RxQueueTrait for Socket<Tx, Rx> {
    fn pop(&mut self, timestamp: Timestamp) -> Option<(DatagramInfo, &mut [u8])> {
        self.rx_buffer.pop(timestamp)
    }

    fn len(&self) -> usize {
        self.rx_buffer.len()
    }
}

#[cfg(feature = "mio")]
impl<Tx: MessageBuffer, Rx: MessageBuffer> mio::Evented for Socket<Tx, Rx> {
    fn register(
        &self,
        poll: &mio::Poll,
        token: mio::Token,
        interest: mio::Ready,
        opts: mio::PollOpt,
    ) -> IOResult<()> {
        self.socket.register(poll, token, interest, opts)
    }

    fn reregister(
        &self,
        poll: &mio::Poll,
        token: mio::Token,
        interest: mio::Ready,
        opts: mio::PollOpt,
    ) -> IOResult<()> {
        self.socket.reregister(poll, token, interest, opts)
    }

    fn deregister(&self, poll: &mio::Poll) -> IOResult<()> {
        self.socket.deregister(poll)
    }
}

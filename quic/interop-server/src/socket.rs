use core::task::{Context, Poll};
use futures::{future::poll_fn, ready};
use mio::Ready;
use s2n_quic_core::{inet::DatagramInfo, time::Timestamp};
use s2n_quic_platform::{
    buffer::VecBuffer,
    io::{
        rx::RxQueue as RxQueueTrait,
        socket::{
            RxQueue as PlatformRxQueue, Socket as PlatformSocket, TxQueue as PlatformTxQueue,
        },
        tx::{
            ExplicitCongestionNotification, SocketAddress, TxError, TxPayload,
            TxQueue as TxQueueTrait,
        },
    },
};
use std::{
    io::{Error as IOError, ErrorKind as IOErrorKind},
    net::ToSocketAddrs,
};
use tokio::io::PollEvented;

pub type TxQueue = PlatformTxQueue<VecBuffer>;
pub type RxQueue = PlatformRxQueue<VecBuffer>;

pub struct Socket(PollEvented<PlatformSocket<VecBuffer, VecBuffer>>);

impl Socket {
    pub fn bind<Addr: ToSocketAddrs>(
        addr: Addr,
        slot_count: usize,
        mtu: usize,
    ) -> Result<Self, IOError> {
        let rx_queue = VecBuffer::new(slot_count, mtu);
        let tx_queue = VecBuffer::new(slot_count, mtu);
        let socket = PlatformSocket::bind(addr, tx_queue, rx_queue)?;
        Ok(Self(PollEvented::new(socket)?))
    }

    #[allow(dead_code)]
    pub fn split_mut(&mut self) -> (&mut TxQueue, &mut RxQueue) {
        self.0.get_mut().split_mut()
    }

    pub async fn sync_rx(&mut self) -> Result<usize, IOError> {
        poll_fn(|cx| self.poll_rx(cx)).await
    }

    fn poll_rx(&mut self, cx: &mut Context<'_>) -> Poll<Result<usize, IOError>> {
        let ready = Ready::readable();

        ready!(self.0.poll_read_ready(cx, ready))?;

        match self.0.get_mut().sync_rx() {
            Ok(count) => Poll::Ready(Ok(count)),
            Err(ref e) if e.kind() == IOErrorKind::WouldBlock => {
                self.0.clear_read_ready(cx, ready)?;
                Poll::Pending
            }
            Err(e) => Poll::Ready(Err(e)),
        }
    }

    pub async fn sync_tx(&mut self) -> Result<usize, IOError> {
        poll_fn(|cx| self.poll_tx(cx)).await
    }

    fn poll_tx(&mut self, cx: &mut Context<'_>) -> Poll<Result<usize, IOError>> {
        ready!(self.0.poll_write_ready(cx))?;

        match self.0.get_mut().sync_tx() {
            Ok(count) => Poll::Ready(Ok(count)),
            Err(ref e) if e.kind() == IOErrorKind::WouldBlock => {
                self.0.clear_write_ready(cx)?;
                Poll::Pending
            }
            Err(e) => Poll::Ready(Err(e)),
        }
    }
}

impl TxQueueTrait for Socket {
    fn push<Payload: TxPayload>(
        &mut self,
        remote_address: &SocketAddress,
        ecn: ExplicitCongestionNotification,
        payload: Payload,
    ) -> Result<usize, TxError> {
        self.0.get_mut().push(remote_address, ecn, payload)
    }

    fn capacity(&self) -> usize {
        self.0.get_ref().capacity()
    }

    fn len(&self) -> usize {
        TxQueueTrait::len(self.0.get_ref())
    }
}

impl RxQueueTrait for Socket {
    fn pop(&mut self, timestamp: Timestamp) -> Option<(DatagramInfo, &mut [u8])> {
        self.0.get_mut().pop(timestamp)
    }

    fn len(&self) -> usize {
        RxQueueTrait::len(self.0.get_ref())
    }
}

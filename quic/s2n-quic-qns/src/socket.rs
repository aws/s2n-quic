use s2n_quic_platform::{
    buffer::default::Buffer,
    io::default::{rx::Rx, tx::Tx},
    socket::default as socket,
};
use std::{io, net::ToSocketAddrs};

pub struct Socket {
    pub rx: Rx<Buffer, socket::Socket>,
    pub tx: Tx<Buffer, socket::Socket>,
}

impl Socket {
    pub fn bind<Addr: ToSocketAddrs>(
        addr: Addr,
        slot_count: usize,
        mtu: usize,
    ) -> io::Result<Self> {
        let rx_buffer = Buffer::new(slot_count, mtu);
        let tx_buffer = Buffer::new(slot_count, mtu);
        let socket = socket::Socket::builder()?.with_address(addr)?.build()?;
        let rx = Rx::new(rx_buffer, socket.try_clone()?);
        let tx = Tx::new(tx_buffer, socket);
        Ok(Self { rx, tx })
    }

    pub async fn sync_rx(&mut self) -> io::Result<usize> {
        self.rx.sync().await
    }

    pub async fn sync_tx(&mut self) -> io::Result<usize> {
        self.tx.sync().await
    }
}

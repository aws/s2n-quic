use cfg_if::cfg_if;
use s2n_quic_core::inet::SocketAddress;
use std::{io, net::UdpSocket};

type Inner = UdpSocket;

impl_socket!(Inner, Builder);
impl_socket_raw_delegate!(impl[] Socket, |self| &self.0);
impl_socket_debug!(impl[] Socket, |self| &self.0.local_addr().ok());

impl Socket {
    pub fn try_clone(&self) -> io::Result<Self> {
        let socket = self.0.try_clone()?;
        Ok(Self(socket))
    }
}

impl crate::socket::Simple for Socket {
    type Error = io::Error;

    fn recv_from(&self, buf: &mut [u8]) -> io::Result<(usize, Option<SocketAddress>)> {
        debug_assert!(!buf.is_empty());
        let (len, addr) = self.0.recv_from(buf)?;
        Ok((len, Some(addr.into())))
    }

    fn send_to(&self, buf: &[u8], addr: &SocketAddress) -> io::Result<usize> {
        debug_assert!(!buf.is_empty());
        let addr: std::net::SocketAddr = (*addr).into();
        self.0.send_to(buf, &addr)
    }
}

impl_socket2_builder!(Builder);

impl Builder {
    pub fn build(self) -> io::Result<Socket> {
        let socket = self.socket.into_udp_socket();
        Ok(Socket(socket))
    }
}

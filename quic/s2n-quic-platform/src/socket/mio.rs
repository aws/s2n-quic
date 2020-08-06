use core::convert::TryFrom;
use s2n_quic_core::inet::SocketAddress;
use socket2::Socket as Socket2;
use std::io;

type Inner = mio::net::UdpSocket;

impl_socket!(Inner, Builder);
impl_socket_deref!(Inner, |self| &self.0, |self| &mut self.0);
impl_socket_raw_delegate!(impl[] Socket, |self| &self.0);
impl_socket_mio_delegate!(impl[] Socket, |self| &self.0);
impl_socket_debug!(impl[] Socket, |self| &self.0.local_addr().ok());

impl Socket {
    pub fn try_clone(&self) -> io::Result<Self> {
        let socket = self.0.try_clone()?;
        Ok(Self(socket))
    }
}

impl TryFrom<Socket2> for Socket {
    type Error = io::Error;

    fn try_from(socket: Socket2) -> io::Result<Self> {
        let socket = socket.into_udp_socket();
        let socket = Inner::from_socket(socket)?;

        Ok(Self(socket))
    }
}

impl crate::socket::Simple for Socket {
    type Error = io::Error;

    fn recv_from(&self, buf: &mut [u8]) -> io::Result<(usize, Option<SocketAddress>)> {
        debug_assert!(!buf.is_empty());

        let (len, addr) = self.0.recv_from(buf)?;
        let addr = Some(addr.into());
        Ok((len, addr))
    }

    fn send_to(&self, buf: &[u8], addr: &SocketAddress) -> io::Result<usize> {
        debug_assert!(!buf.is_empty());

        let addr = (*addr).into();

        self.0.send_to(buf, &addr)
    }
}

impl_socket2_builder!(Builder);

impl Builder {
    pub fn build(self) -> io::Result<Socket> {
        Socket::try_from(self.socket)
    }
}

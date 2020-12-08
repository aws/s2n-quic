use core::{
    convert::{TryFrom, TryInto},
    task::{Context, Poll},
};
use futures_core::ready;
use mio::net::UdpSocket as MioSocket;
use s2n_quic_core::inet::SocketAddress;
use socket2::Socket as Socket2;
use std::io;
use tokio::io::PollEvented;

type Inner = PollEvented<MioSocket>;

impl_socket!(Inner, Builder);
impl_socket_deref!(MioSocket, |self| self.0.get_ref(), |self| self.0.get_mut());
impl_socket_raw_delegate!(impl[] Socket, |self| self.0.get_ref());
impl_socket_mio_delegate!(impl[] Socket, |self| self.0.get_ref());
impl_socket_debug!(impl[] Socket, |self| &self.0.get_ref().local_addr().ok());

impl Socket {
    pub fn try_clone(&self) -> io::Result<Self> {
        let socket = self.0.get_ref().try_clone()?;
        Ok(Self(PollEvented::new(socket)?))
    }
}

impl TryFrom<Socket2> for Socket {
    type Error = io::Error;

    fn try_from(socket: Socket2) -> io::Result<Self> {
        let socket: super::mio::Socket = socket.try_into()?;
        let socket = PollEvented::new(socket.0)?;
        Ok(Self(socket))
    }
}

impl crate::socket::Socket for Socket {
    type Error = io::Error;

    fn poll_receive<F: FnOnce(&mut Self) -> Result<V, Self::Error>, V>(
        &mut self,
        cx: &mut Context<'_>,
        f: F,
    ) -> Poll<Result<V, Self::Error>> {
        let ready = mio::Ready::readable();

        ready!(self.0.poll_read_ready(cx, ready))?;

        match f(self) {
            Ok(count) => Poll::Ready(Ok(count)),
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                self.0.clear_read_ready(cx, ready)?;
                Poll::Pending
            }
            Err(e) => Poll::Ready(Err(e)),
        }
    }

    fn poll_transmit<F: FnOnce(&mut Self) -> Result<V, Self::Error>, V>(
        &mut self,
        cx: &mut Context<'_>,
        f: F,
    ) -> Poll<Result<V, Self::Error>> {
        ready!(self.0.poll_write_ready(cx))?;

        match f(self) {
            Ok(count) => Poll::Ready(Ok(count)),
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                self.0.clear_write_ready(cx)?;
                Poll::Pending
            }
            Err(e) => Poll::Ready(Err(e)),
        }
    }
}

impl crate::socket::Simple for Socket {
    type Error = io::Error;

    fn recv_from(&self, buf: &mut [u8]) -> io::Result<(usize, Option<SocketAddress>)> {
        debug_assert!(!buf.is_empty());
        let (len, addr) = self.0.get_ref().recv_from(buf)?;
        let addr = Some(addr.into());
        Ok((len, addr))
    }

    fn send_to(&self, buf: &[u8], addr: &SocketAddress) -> io::Result<usize> {
        debug_assert!(!buf.is_empty());
        let addr = (*addr).into();
        self.0.get_ref().send_to(buf, &addr)
    }
}

impl_socket2_builder!(Builder);

impl Builder {
    pub fn build(self) -> io::Result<Socket> {
        Socket::try_from(self.socket)
    }
}

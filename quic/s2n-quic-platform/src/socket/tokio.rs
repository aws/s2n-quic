use core::convert::{TryFrom, TryInto};
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

#[cfg(feature = "futures")]
pub(crate) mod sync {
    use super::Socket;
    use core::ops::Deref;
    use futures::future::poll_fn;
    use s2n_quic_core::io::{rx::Rx, tx::Tx};
    use std::io;

    pub async fn receive<'a, R: Rx<'a, Error = io::Error> + Deref<Target = Socket>>(
        rx: &mut R,
    ) -> io::Result<usize> {
        poll_fn(|cx| super::poll::receive(rx, cx)).await
    }

    pub async fn transmit<'a, T: Tx<'a, Error = io::Error> + Deref<Target = Socket>>(
        tx: &mut T,
    ) -> io::Result<usize> {
        poll_fn(|cx| super::poll::transmit(tx, cx)).await
    }
}

#[cfg(feature = "futures")]
pub(crate) mod poll {
    use super::Socket;
    use core::{
        ops::Deref,
        task::{Context, Poll},
    };
    use futures::ready;
    use mio::Ready;
    use s2n_quic_core::io::{rx::Rx, tx::Tx};
    use std::io;

    pub fn receive<'a, R: Rx<'a, Error = io::Error> + Deref<Target = Socket>>(
        rx: &mut R,
        cx: &mut Context<'_>,
    ) -> Poll<io::Result<usize>> {
        let ready = Ready::readable();

        ready!(rx.deref().0.poll_read_ready(cx, ready))?;

        match rx.receive() {
            Ok(count) => Poll::Ready(Ok(count)),
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                rx.deref().0.clear_read_ready(cx, ready)?;
                Poll::Pending
            }
            Err(e) => Poll::Ready(Err(e)),
        }
    }

    pub fn transmit<'a, T: Tx<'a, Error = io::Error> + Deref<Target = Socket>>(
        tx: &mut T,
        cx: &mut Context<'_>,
    ) -> Poll<io::Result<usize>> {
        if tx.is_empty() {
            return Poll::Ready(Ok(0));
        }

        ready!(tx.deref().0.poll_write_ready(cx))?;

        match tx.transmit() {
            Ok(count) => Poll::Ready(Ok(count)),
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                tx.deref().0.clear_write_ready(cx)?;
                Poll::Pending
            }
            Err(e) => Poll::Ready(Err(e)),
        }
    }
}

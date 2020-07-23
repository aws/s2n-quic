use super::{udp::UdpSocket, RxQueue, Socket, TxQueue};
use crate::buffer::Buffer as MessageBuffer;
use net2::{unix::UnixUdpBuilderExt, UdpBuilder};
use std::{
    io::Result as IOResult,
    net::{SocketAddr, ToSocketAddrs},
};

/// Builder for creating a buffered UDP socket
#[derive(Debug)]
pub struct SocketBuilder<Tx, Rx> {
    tx_buffer: Tx,
    rx_buffer: Rx,
    addr: SocketAddr,
    socket: UdpBuilder,
}

impl SocketBuilder<(), ()> {
    /// Creates a new SocketBuilder
    ///
    /// # Note
    /// A `tx_buffer` and `rx_buffer` must be specified before calling
    /// `build`.
    pub fn new<Addrs: ToSocketAddrs>(addr: Addrs) -> IOResult<Self> {
        let mut addr = addr.to_socket_addrs()?;
        let addr = addr.next().expect("Invalid address");

        let socket = match addr {
            SocketAddr::V4(_) => UdpBuilder::new_v4()?,
            SocketAddr::V6(_) if cfg!(feature = "ipv6") => {
                let socket = UdpBuilder::new_v6()?;

                // normalize this setting across platforms - default to
                // accept both v4 and v6 on the same socket
                socket.only_v6(false)?;

                socket
            }
            SocketAddr::V6(_) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "IPv6 support not enabled",
                ))
            }
        };

        Ok(Self {
            tx_buffer: (),
            rx_buffer: (),
            addr,
            socket,
        })
    }
}

impl<Tx, Rx> SocketBuilder<Tx, Rx> {
    /// Set the `tx_buffer` for the socket
    pub fn tx_buffer<NewTx: MessageBuffer>(self, tx_buffer: NewTx) -> SocketBuilder<NewTx, Rx> {
        SocketBuilder {
            tx_buffer,
            rx_buffer: self.rx_buffer,
            addr: self.addr,
            socket: self.socket,
        }
    }

    /// Set the `rx_buffer` for the socket
    pub fn rx_buffer<NewRx: MessageBuffer>(self, rx_buffer: NewRx) -> SocketBuilder<Tx, NewRx> {
        SocketBuilder {
            rx_buffer,
            tx_buffer: self.tx_buffer,
            addr: self.addr,
            socket: self.socket,
        }
    }

    /// Enable port reuse for the socket
    ///
    /// The use of this option can provide better
    /// distribution of incoming datagrams to multiple processes (or
    /// threads) as compared to the traditional technique of having
    /// multiple processes compete to receive datagrams on the same
    /// socket.
    pub fn reuse_port(self) -> IOResult<Self> {
        self.socket.reuse_port(true)?;
        Ok(self)
    }

    /// Enable address reuse for the socket
    ///
    /// Indicates that the rules used in validating addresses supplied
    /// in a bind(2) call should allow reuse of local addresses.  For
    /// AF_INET sockets this means that a socket may bind, except when
    /// there is an active listening socket bound to the address.
    /// When the listening socket is bound to INADDR_ANY with a spe‐
    /// cific port then it is not possible to bind to this port for
    /// any local address.
    pub fn reuse_address(self) -> IOResult<Self> {
        self.socket.reuse_address(true)?;
        Ok(self)
    }

    /// Sets the value for the `IP_TTL` option on this socket.
    ///
    /// This value sets the time-to-live field that is used in every packet sent
    /// from this socket.
    pub fn ttl(self, ttl: u32) -> IOResult<Self> {
        self.socket.ttl(ttl)?;
        Ok(self)
    }

    /// Sets the value for the `IPV6_V6ONLY` option on this socket.
    ///
    /// If this is set the socket is restricted to sending and
    /// receiving IPv6 packets only. In this case two IPv4 and IPv6 applications
    /// can bind the same port at the same time.
    ///
    /// By default, IPv6 sockets will recieve IPv4 and IPv6 packets.
    #[cfg(feature = "ipv6")]
    pub fn only_v6(self) -> IOResult<Self> {
        self.socket.only_v6(true)?;
        Ok(self)
    }
}

impl<Tx: MessageBuffer, Rx: MessageBuffer> SocketBuilder<Tx, Rx> {
    /// Create a Socket for the given builder
    pub fn build(self) -> IOResult<Socket<Tx, Rx>> {
        let socket = self.socket.bind(self.addr)?;
        let socket = UdpSocket::from_socket(socket)?;

        let tx_buffer = TxQueue::new(self.tx_buffer);
        let rx_buffer = RxQueue::new(self.rx_buffer);

        Ok(Socket {
            tx_buffer,
            rx_buffer,
            socket,
        })
    }
}

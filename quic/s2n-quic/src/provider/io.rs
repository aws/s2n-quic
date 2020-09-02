use s2n_quic_core::io::{rx, tx};
pub use s2n_quic_core::{inet, io::Duplex};
use std::io;

/// Provides IO support for an endpoint
pub trait Provider
where
    for<'a> Self::Rx: 'static + Send + rx::Rx<'a>,
    for<'a> Self::Tx: 'static + Send + tx::Tx<'a>,
{
    type Rx;
    type Tx;
    type Error: core::fmt::Display;

    fn start(self) -> Result<Duplex<Self::Rx, Self::Tx>, Self::Error>;
}

impl<Rx, Tx> Provider for Duplex<Rx, Tx>
where
    for<'a> Rx: 'static + Send + rx::Rx<'a>,
    for<'a> Tx: 'static + Send + tx::Tx<'a>,
{
    type Rx = Rx;
    type Tx = Tx;
    type Error = io::Error;

    fn start(self) -> io::Result<Self> {
        Ok(self)
    }
}

pub use platform::Provider as Default;

impl TryInto for u16 {
    type Error = io::Error;
    type Provider = Default;

    fn try_into(self) -> io::Result<Self::Provider> {
        Default::new(("0.0.0.0", self))
    }
}

macro_rules! impl_socket_addrs {
    ($ty:ty) => {
        impl TryInto for $ty {
            type Error = io::Error;
            type Provider = Default;

            fn try_into(self) -> io::Result<Self::Provider> {
                Default::new(self)
            }
        }
    };
}

impl_socket_addrs!((&str, u16));
impl_socket_addrs!(&str);
impl_socket_addrs!(std::net::SocketAddr);
impl_socket_addrs!(std::net::SocketAddrV4);
impl_socket_addrs!(std::net::SocketAddrV6);
impl_socket_addrs!(inet::SocketAddress);
impl_socket_addrs!(inet::SocketAddressV4);
impl_socket_addrs!(inet::SocketAddressV6);

impl_provider_utils!();

pub mod platform {
    pub use s2n_quic_platform::default::*;
    use std::{
        io,
        net::{SocketAddr, ToSocketAddrs},
    };

    #[derive(Debug)]
    pub struct Provider {
        addresses: Vec<SocketAddr>,
    }

    impl Provider {
        pub fn new<A: ToSocketAddrs>(addr: A) -> io::Result<Self> {
            let addresses = addr.to_socket_addrs()?.collect();
            Ok(Self { addresses })
        }
    }

    impl Default for Provider {
        fn default() -> Self {
            Self {
                addresses: ("0.0.0.0", 443u16).to_socket_addrs().unwrap().collect(),
            }
        }
    }

    impl super::Provider for Provider {
        type Rx = Rx;
        type Tx = Tx;
        type Error = io::Error;

        fn start(self) -> io::Result<Duplex> {
            let mut builder = Socket::builder()?;
            for addr in self.addresses {
                builder = builder.with_address(addr)?;
            }

            let socket = builder.build()?;

            let rx = Rx::new(Buffer::default(), socket.try_clone()?);
            let tx = Tx::new(Buffer::default(), socket);

            let socket = Duplex { rx, tx };
            Ok(socket)
        }
    }
}

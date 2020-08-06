use s2n_quic_core::io::{rx, tx};
pub use s2n_quic_core::{inet, io::Pair};
pub use s2n_quic_platform::default;
use std::{
    io,
    net::{SocketAddr, ToSocketAddrs},
};

/// Provides IO support for an endpoint
pub trait Provider
where
    for<'a> Self::Pair: rx::Rx<'a> + tx::Tx<'a>,
{
    type Pair;

    fn finish(self) -> io::Result<Self::Pair>;
}

impl<Rx, Tx> Provider for Pair<Rx, Tx>
where
    for<'a> Rx: rx::Rx<'a>,
    for<'a> Tx: tx::Tx<'a>,
{
    type Pair = Self;

    fn finish(self) -> io::Result<Self> {
        Ok(self)
    }
}

#[derive(Debug)]
pub struct Default {
    addresses: Vec<SocketAddr>,
}

impl Default {
    fn new<A: ToSocketAddrs>(addr: A) -> io::Result<Self> {
        let addresses = addr.to_socket_addrs()?.collect();
        Ok(Self { addresses })
    }
}

impl core::default::Default for Default {
    fn default() -> Self {
        Self {
            addresses: ("0.0.0.0", 443u16).to_socket_addrs().unwrap().collect(),
        }
    }
}

impl Provider for Default {
    type Pair = default::Pair;

    fn finish(self) -> io::Result<Self::Pair> {
        let mut builder = default::Socket::builder()?;
        for addr in self.addresses {
            builder = builder.with_address(addr)?;
        }

        let socket = builder.build()?;

        let rx = default::Rx::new(default::Buffer::default(), socket.try_clone()?);
        let tx = default::Tx::new(default::Buffer::default(), socket);

        let socket = default::Pair { rx, tx };
        Ok(socket)
    }
}

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

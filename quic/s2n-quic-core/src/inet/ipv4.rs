use crate::inet::unspecified::Unspecified;
use core::fmt;
use s2n_codec::zerocopy::U16;

/// Length defined at https://tools.ietf.org/html/rfc791#section-2.3
/// > Addresses are fixed length of four octets (32 bits).
const IPV4_LEN: usize = 32 / 8;

define_inet_type!(
    pub struct IPv4Address {
        octets: [u8; IPV4_LEN],
    }
);

impl fmt::Debug for IPv4Address {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "IPv4Address({})", self)
    }
}

impl fmt::Display for IPv4Address {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        let octets = &self.octets;
        write!(
            fmt,
            "{}.{}.{}.{}",
            octets[0], octets[1], octets[2], octets[3]
        )
    }
}

impl Unspecified for IPv4Address {
    fn is_unspecified(&self) -> bool {
        <[u8; IPV4_LEN]>::default().eq(&self.octets)
    }
}

test_inet_snapshot!(ipv4, ipv4_snapshot_test, IPv4Address);

define_inet_type!(
    pub struct SocketAddressV4 {
        ip: IPv4Address,
        port: U16,
    }
);

impl SocketAddressV4 {
    pub fn ip(&self) -> &IPv4Address {
        &self.ip
    }

    #[inline(always)]
    #[allow(clippy::trivially_copy_pass_by_ref)]
    pub fn port(&self) -> u16 {
        self.port.into()
    }

    pub fn set_port(&mut self, port: u16) {
        self.port.set(port)
    }
}

impl fmt::Debug for SocketAddressV4 {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "SocketAddressV4({})", self)
    }
}

impl fmt::Display for SocketAddressV4 {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "{}:{}", self.ip, self.port)
    }
}

impl Unspecified for SocketAddressV4 {
    fn is_unspecified(&self) -> bool {
        self.ip.is_unspecified() && self.port.is_unspecified()
    }
}

test_inet_snapshot!(socket_v4, socket_v4_snapshot_test, SocketAddressV4);

impl From<[u8; IPV4_LEN]> for IPv4Address {
    fn from(octets: [u8; IPV4_LEN]) -> Self {
        Self { octets }
    }
}

impl Into<[u8; IPV4_LEN]> for IPv4Address {
    fn into(self) -> [u8; IPV4_LEN] {
        self.octets
    }
}

#[cfg(any(test, feature = "std"))]
mod std_conversion {
    use super::*;
    use std::net;

    impl From<net::Ipv4Addr> for IPv4Address {
        fn from(address: net::Ipv4Addr) -> Self {
            (&address).into()
        }
    }

    impl From<&net::Ipv4Addr> for IPv4Address {
        fn from(address: &net::Ipv4Addr) -> Self {
            address.octets().into()
        }
    }

    impl Into<net::Ipv4Addr> for IPv4Address {
        fn into(self) -> net::Ipv4Addr {
            self.octets.into()
        }
    }

    impl From<net::SocketAddrV4> for SocketAddressV4 {
        fn from(address: net::SocketAddrV4) -> Self {
            let ip = address.ip().into();
            let port = address.port().into();
            Self { ip, port }
        }
    }

    impl Into<net::SocketAddrV4> for SocketAddressV4 {
        fn into(self) -> net::SocketAddrV4 {
            let ip = self.ip.into();
            let port = self.port.into();
            net::SocketAddrV4::new(ip, port)
        }
    }

    impl Into<net::SocketAddrV4> for &SocketAddressV4 {
        fn into(self) -> net::SocketAddrV4 {
            let ip = self.ip.into();
            let port = self.port.into();
            net::SocketAddrV4::new(ip, port)
        }
    }

    impl Into<net::SocketAddr> for SocketAddressV4 {
        fn into(self) -> net::SocketAddr {
            let addr: net::SocketAddrV4 = self.into();
            addr.into()
        }
    }

    impl Into<net::SocketAddr> for &SocketAddressV4 {
        fn into(self) -> net::SocketAddr {
            let addr: net::SocketAddrV4 = self.into();
            addr.into()
        }
    }

    impl net::ToSocketAddrs for SocketAddressV4 {
        type Iter = std::iter::Once<net::SocketAddr>;

        fn to_socket_addrs(&self) -> std::io::Result<Self::Iter> {
            let ip = self.ip.into();
            let port = self.port.into();
            let addr = net::SocketAddrV4::new(ip, port);
            Ok(std::iter::once(addr.into()))
        }
    }
}

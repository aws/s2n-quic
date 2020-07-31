use crate::inet::{
    ipv4::{IPv4Address, SocketAddressV4},
    ipv6::{IPv6Address, SocketAddressV6},
    unspecified::Unspecified,
};
use core::fmt;

#[cfg(feature = "generator")]
use bolero_generator::*;

/// An IP address, either IPv4 or IPv6.
///
/// Instead of using `std::net::IPAddr`, this implementation
/// is geared towards `no_std` environments and zerocopy decoding.
///
/// The size is also consistent across target operating systems.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "generator", derive(TypeGenerator))]
pub enum IPAddress {
    IPv4(IPv4Address),
    IPv6(IPv6Address),
}

impl From<IPv4Address> for IPAddress {
    fn from(ip: IPv4Address) -> Self {
        Self::IPv4(ip)
    }
}

impl From<IPv6Address> for IPAddress {
    fn from(ip: IPv6Address) -> Self {
        Self::IPv6(ip)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum IPAddressRef<'a> {
    IPv4(&'a IPv4Address),
    IPv6(&'a IPv6Address),
}

impl<'a> IPAddressRef<'a> {
    pub fn to_owned(self) -> IPAddress {
        match self {
            Self::IPv4(addr) => IPAddress::IPv4(*addr),
            Self::IPv6(addr) => IPAddress::IPv6(*addr),
        }
    }
}

impl<'a> From<&'a IPv4Address> for IPAddressRef<'a> {
    fn from(ip: &'a IPv4Address) -> Self {
        Self::IPv4(ip)
    }
}

impl<'a> From<&'a IPv6Address> for IPAddressRef<'a> {
    fn from(ip: &'a IPv6Address) -> Self {
        Self::IPv6(ip)
    }
}

/// An IP socket address, either IPv4 or IPv6, with a specific port.
///
/// Instead of using `std::net::SocketAddr`, this implementation
/// is geared towards `no_std` environments and zerocopy decoding.
///
/// The size is also consistent across target operating systems.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "generator", derive(TypeGenerator))]
pub enum SocketAddress {
    IPv4(SocketAddressV4),
    IPv6(SocketAddressV6),
}

impl SocketAddress {
    pub fn ip(&self) -> IPAddress {
        match self {
            SocketAddress::IPv4(addr) => IPAddress::IPv4(*addr.ip()),
            SocketAddress::IPv6(addr) => IPAddress::IPv6(*addr.ip()),
        }
    }

    pub fn port(&self) -> u16 {
        match self {
            SocketAddress::IPv4(addr) => addr.port(),
            SocketAddress::IPv6(addr) => addr.port(),
        }
    }

    /// Converts the IP address into a IPv6 mapped address
    pub fn to_v6_mapped(&self) -> SocketAddressV6 {
        match self {
            Self::IPv4(addr) => addr.to_v6_mapped(),
            Self::IPv6(addr) => *addr,
        }
    }
}

impl Default for SocketAddress {
    fn default() -> Self {
        SocketAddress::IPv4(Default::default())
    }
}

impl fmt::Display for SocketAddress {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        match self {
            SocketAddress::IPv4(addr) => write!(fmt, "{}", addr),
            SocketAddress::IPv6(addr) => write!(fmt, "{}", addr),
        }
    }
}

impl Unspecified for SocketAddress {
    fn is_unspecified(&self) -> bool {
        match self {
            SocketAddress::IPv4(addr) => addr.is_unspecified(),
            SocketAddress::IPv6(addr) => addr.is_unspecified(),
        }
    }
}

impl From<SocketAddressV4> for SocketAddress {
    fn from(addr: SocketAddressV4) -> Self {
        SocketAddress::IPv4(addr)
    }
}

impl From<SocketAddressV6> for SocketAddress {
    fn from(addr: SocketAddressV6) -> Self {
        SocketAddress::IPv6(addr)
    }
}

/// An IP socket address, either IPv4 or IPv6, with a specific port.
///
/// This is the borrowed version of `SocketAddress`, aimed at zerocopy
/// use cases.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SocketAddressRef<'a> {
    IPv4(&'a SocketAddressV4),
    IPv6(&'a SocketAddressV6),
}

impl<'a> SocketAddressRef<'a> {
    pub fn to_owned(self) -> SocketAddress {
        match self {
            Self::IPv4(addr) => SocketAddress::IPv4(*addr),
            Self::IPv6(addr) => SocketAddress::IPv6(*addr),
        }
    }
}

#[cfg(any(test, feature = "std"))]
mod std_conversion {
    use super::*;
    use std::net;

    impl net::ToSocketAddrs for SocketAddress {
        type Iter = std::iter::Once<net::SocketAddr>;

        fn to_socket_addrs(&self) -> std::io::Result<Self::Iter> {
            match self {
                Self::IPv4(addr) => addr.to_socket_addrs(),
                Self::IPv6(addr) => addr.to_socket_addrs(),
            }
        }
    }

    impl<'a> net::ToSocketAddrs for SocketAddressRef<'a> {
        type Iter = std::iter::Once<net::SocketAddr>;

        fn to_socket_addrs(&self) -> std::io::Result<Self::Iter> {
            match self {
                Self::IPv4(addr) => addr.to_socket_addrs(),
                Self::IPv6(addr) => addr.to_socket_addrs(),
            }
        }
    }

    impl Into<net::SocketAddr> for SocketAddress {
        fn into(self) -> net::SocketAddr {
            match self {
                Self::IPv4(addr) => addr.into(),
                Self::IPv6(addr) => addr.into(),
            }
        }
    }

    impl<'a> Into<net::SocketAddr> for SocketAddressRef<'a> {
        fn into(self) -> net::SocketAddr {
            match self {
                Self::IPv4(addr) => addr.into(),
                Self::IPv6(addr) => addr.into(),
            }
        }
    }

    impl From<net::SocketAddr> for SocketAddress {
        fn from(addr: net::SocketAddr) -> Self {
            match addr {
                net::SocketAddr::V4(addr) => Self::IPv4(addr.into()),
                net::SocketAddr::V6(addr) => Self::IPv6(addr.into()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{SocketAddr, ToSocketAddrs};

    const TESTS: &[&str] = &[
        "127.0.0.1:80",
        "192.168.1.1:40",
        "255.255.255.255:12345",
        "[::]:443",
        "[::1]:123",
        "[2001:0db8:85a3:0001:0002:8a2e:0370:7334]:9000",
    ];

    #[test]
    #[compliance::tests(
        /// ::FFFF:0:0/96 are the IPv4-mapped addresses [RFC4291].
        "https://tools.ietf.org/rfc/rfc5156.txt#2.2"
    )]
    fn to_ipv6_mapped_test() {
        for test in TESTS.iter() {
            // assert that this implementation matches the standard library
            let addr: SocketAddr = test.parse().unwrap();
            let address: SocketAddress = addr.into();
            let addr = match addr {
                SocketAddr::V4(addr) => {
                    let ip = addr.ip().to_ipv6_mapped();
                    (ip, addr.port()).into()
                }
                _ => addr,
            };
            let address = address.to_v6_mapped().into();
            assert_eq!(addr, address);
        }
    }

    #[test]
    fn display_test() {
        for test in TESTS.iter() {
            // assert that this implementation matches the standard library
            let addr: SocketAddr = test.parse().unwrap();
            let address: SocketAddress = addr.into();
            assert_eq!(addr.to_string(), address.to_string());
        }
    }

    #[test]
    fn to_socket_addrs_test() {
        for test in TESTS.iter() {
            let addr: SocketAddr = test.parse().unwrap();
            let address: SocketAddress = addr.into();
            for address in address.to_socket_addrs().unwrap() {
                assert_eq!(addr, address);
            }
        }
    }
}

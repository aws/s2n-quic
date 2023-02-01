// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::inet::{
    ipv4::{IpV4Address, SocketAddressV4},
    ipv6::{IpV6Address, SocketAddressV6},
    unspecified::Unspecified,
};
use core::fmt;

#[cfg(any(test, feature = "generator"))]
use bolero_generator::*;

/// An IP address, either IPv4 or IPv6.
///
/// Instead of using `std::net::IPAddr`, this implementation
/// is geared towards `no_std` environments and zerocopy decoding.
///
/// The size is also consistent across target operating systems.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(any(test, feature = "generator"), derive(TypeGenerator))]
pub enum IpAddress {
    Ipv4(IpV4Address),
    Ipv6(IpV6Address),
}

impl IpAddress {
    /// Converts the IP address into IPv4 if it is mapped, otherwise the address is unchanged
    #[inline]
    #[must_use]
    pub fn unmap(self) -> Self {
        match self {
            Self::Ipv4(_) => self,
            Self::Ipv6(addr) => addr.unmap(),
        }
    }
}

impl From<IpV4Address> for IpAddress {
    fn from(ip: IpV4Address) -> Self {
        Self::Ipv4(ip)
    }
}

impl From<IpV6Address> for IpAddress {
    fn from(ip: IpV6Address) -> Self {
        Self::Ipv6(ip)
    }
}

impl Unspecified for IpAddress {
    fn is_unspecified(&self) -> bool {
        match self {
            Self::Ipv4(addr) => addr.is_unspecified(),
            Self::Ipv6(addr) => addr.is_unspecified(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum IpAddressRef<'a> {
    IPv4(&'a IpV4Address),
    IPv6(&'a IpV6Address),
}

impl<'a> IpAddressRef<'a> {
    pub fn to_owned(self) -> IpAddress {
        match self {
            Self::IPv4(addr) => IpAddress::Ipv4(*addr),
            Self::IPv6(addr) => IpAddress::Ipv6(*addr),
        }
    }
}

impl<'a> From<&'a IpV4Address> for IpAddressRef<'a> {
    fn from(ip: &'a IpV4Address) -> Self {
        Self::IPv4(ip)
    }
}

impl<'a> From<&'a IpV6Address> for IpAddressRef<'a> {
    fn from(ip: &'a IpV6Address) -> Self {
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
#[cfg_attr(any(test, feature = "generator"), derive(TypeGenerator))]
pub enum SocketAddress {
    IpV4(SocketAddressV4),
    IpV6(SocketAddressV6),
}

impl SocketAddress {
    #[inline]
    pub fn ip(&self) -> IpAddress {
        match self {
            SocketAddress::IpV4(addr) => IpAddress::Ipv4(*addr.ip()),
            SocketAddress::IpV6(addr) => IpAddress::Ipv6(*addr.ip()),
        }
    }

    #[inline]
    pub fn port(&self) -> u16 {
        match self {
            SocketAddress::IpV4(addr) => addr.port(),
            SocketAddress::IpV6(addr) => addr.port(),
        }
    }

    #[inline]
    pub fn set_port(&mut self, port: u16) {
        match self {
            SocketAddress::IpV4(addr) => addr.set_port(port),
            SocketAddress::IpV6(addr) => addr.set_port(port),
        }
    }

    #[inline]
    pub const fn unicast_scope(&self) -> Option<UnicastScope> {
        match self {
            Self::IpV4(addr) => addr.unicast_scope(),
            Self::IpV6(addr) => addr.unicast_scope(),
        }
    }

    /// Converts the IP address into a IPv6 mapped address
    pub fn to_ipv6_mapped(self) -> SocketAddressV6 {
        match self {
            Self::IpV4(addr) => addr.to_ipv6_mapped(),
            Self::IpV6(addr) => addr,
        }
    }

    /// Converts the IP address into IPv4 if it is mapped, otherwise the address is unchanged
    #[inline]
    #[must_use]
    pub fn unmap(self) -> Self {
        match self {
            Self::IpV4(_) => self,
            Self::IpV6(addr) => addr.unmap(),
        }
    }
}

impl Default for SocketAddress {
    fn default() -> Self {
        SocketAddress::IpV4(Default::default())
    }
}

impl fmt::Display for SocketAddress {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        match self {
            SocketAddress::IpV4(addr) => write!(fmt, "{addr}"),
            SocketAddress::IpV6(addr) => write!(fmt, "{addr}"),
        }
    }
}

impl Unspecified for SocketAddress {
    fn is_unspecified(&self) -> bool {
        match self {
            SocketAddress::IpV4(addr) => addr.is_unspecified(),
            SocketAddress::IpV6(addr) => addr.is_unspecified(),
        }
    }
}

impl From<SocketAddressV4> for SocketAddress {
    fn from(addr: SocketAddressV4) -> Self {
        SocketAddress::IpV4(addr)
    }
}

impl From<SocketAddressV6> for SocketAddress {
    fn from(addr: SocketAddressV6) -> Self {
        SocketAddress::IpV6(addr)
    }
}

/// An IP socket address, either IPv4 or IPv6, with a specific port.
///
/// This is the borrowed version of `SocketAddress`, aimed at zerocopy
/// use cases.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SocketAddressRef<'a> {
    IpV4(&'a SocketAddressV4),
    IpV6(&'a SocketAddressV6),
}

impl<'a> SocketAddressRef<'a> {
    pub fn to_owned(self) -> SocketAddress {
        match self {
            Self::IpV4(addr) => SocketAddress::IpV4(*addr),
            Self::IpV6(addr) => SocketAddress::IpV6(*addr),
        }
    }
}

//= https://www.rfc-editor.org/rfc/rfc9000#section-21.5.6
//# Similarly, endpoints could regard a change in address to a link-local
//# address [RFC4291] or an address in a private-use range [RFC1918] from
//# a global, unique-local [RFC4193], or non-private address as a
//# potential attempt at request forgery.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnicastScope {
    Loopback,
    LinkLocal,
    Private,
    Global,
}

#[cfg(any(test, feature = "std"))]
mod std_conversion {
    use super::*;
    use std::net;

    impl net::ToSocketAddrs for SocketAddress {
        type Iter = std::iter::Once<net::SocketAddr>;

        fn to_socket_addrs(&self) -> std::io::Result<Self::Iter> {
            match self {
                Self::IpV4(addr) => addr.to_socket_addrs(),
                Self::IpV6(addr) => addr.to_socket_addrs(),
            }
        }
    }

    impl<'a> net::ToSocketAddrs for SocketAddressRef<'a> {
        type Iter = std::iter::Once<net::SocketAddr>;

        fn to_socket_addrs(&self) -> std::io::Result<Self::Iter> {
            match self {
                Self::IpV4(addr) => addr.to_socket_addrs(),
                Self::IpV6(addr) => addr.to_socket_addrs(),
            }
        }
    }

    impl From<SocketAddress> for net::SocketAddr {
        fn from(address: SocketAddress) -> Self {
            match address {
                SocketAddress::IpV4(addr) => addr.into(),
                SocketAddress::IpV6(addr) => addr.into(),
            }
        }
    }

    impl From<(net::IpAddr, u16)> for SocketAddress {
        fn from((ip, port): (net::IpAddr, u16)) -> Self {
            match ip {
                net::IpAddr::V4(ip) => Self::IpV4((ip, port).into()),
                net::IpAddr::V6(ip) => Self::IpV6((ip, port).into()),
            }
        }
    }

    impl<'a> From<SocketAddressRef<'a>> for net::SocketAddr {
        fn from(address: SocketAddressRef<'a>) -> Self {
            match address {
                SocketAddressRef::IpV4(addr) => addr.into(),
                SocketAddressRef::IpV6(addr) => addr.into(),
            }
        }
    }

    impl From<net::SocketAddr> for SocketAddress {
        fn from(addr: net::SocketAddr) -> Self {
            match addr {
                net::SocketAddr::V4(addr) => Self::IpV4(addr.into()),
                net::SocketAddr::V6(addr) => Self::IpV6(addr.into()),
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
    //= https://www.rfc-editor.org/rfc/rfc5156#section-2.2
    //= type=test
    //# ::FFFF:0:0/96 are the IPv4-mapped addresses [RFC4291].
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
            let address = address.to_ipv6_mapped().into();
            assert_eq!(addr, address);
        }
    }

    #[test]
    fn unmap_test() {
        for test in TESTS.iter() {
            // assert that unmap correctly converts IPv4 mapped addresses
            let addr: SocketAddr = test.parse().unwrap();
            let address: SocketAddress = addr.into();
            let actual: SocketAddress = address.to_ipv6_mapped().into();
            let actual = actual.unmap();
            assert_eq!(address, actual);
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

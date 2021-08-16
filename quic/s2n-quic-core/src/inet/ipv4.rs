// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::inet::{
    ipv6::{IpV6Address, SocketAddressV6},
    unspecified::Unspecified,
};
use core::{fmt, mem::size_of};
use s2n_codec::zerocopy::U16;

//= https://tools.ietf.org/rfc/rfc791.txt#2.3
//# Addresses are fixed length of four octets (32 bits).
const IPV4_LEN: usize = 32 / 8;

define_inet_type!(
    pub struct IpV4Address {
        octets: [u8; IPV4_LEN],
    }
);

impl IpV4Address {
    /// An unspecified IpV4Address
    pub const UNSPECIFIED: Self = Self {
        octets: [0; IPV4_LEN],
    };

    /// Converts the IP address into a IPv6 mapped address
    #[inline]
    pub const fn to_ipv6_mapped(self) -> IpV6Address {
        //= https://tools.ietf.org/rfc/rfc5156.txt#2.2
        //# ::FFFF:0:0/96 are the IPv4-mapped addresses [RFC4291].
        let mut addr = [0; size_of::<IpV6Address>()];
        let [a, b, c, d] = self.octets;
        addr[10] = 0xFF;
        addr[11] = 0xFF;
        addr[12] = a;
        addr[13] = b;
        addr[14] = c;
        addr[15] = d;
        IpV6Address { octets: addr }
    }
}

impl fmt::Debug for IpV4Address {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "IPv4Address({})", self)
    }
}

impl fmt::Display for IpV4Address {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        let octets = &self.octets;
        write!(
            fmt,
            "{}.{}.{}.{}",
            octets[0], octets[1], octets[2], octets[3]
        )
    }
}

impl Unspecified for IpV4Address {
    #[inline]
    fn is_unspecified(&self) -> bool {
        Self::UNSPECIFIED.eq(self)
    }
}

test_inet_snapshot!(ipv4, ipv4_snapshot_test, IpV4Address);

define_inet_type!(
    pub struct SocketAddressV4 {
        ip: IpV4Address,
        port: U16,
    }
);

impl SocketAddressV4 {
    pub const UNSPECIFIED: Self = Self {
        ip: IpV4Address::UNSPECIFIED,
        port: U16::ZERO,
    };

    #[inline]
    pub const fn ip(&self) -> &IpV4Address {
        &self.ip
    }

    #[inline]
    pub fn port(self) -> u16 {
        self.port.into()
    }

    #[inline]
    pub fn set_port(&mut self, port: u16) {
        self.port.set(port)
    }

    /// Converts the IP address into a IPv6 mapped address
    #[inline]
    pub const fn to_ipv6_mapped(self) -> SocketAddressV6 {
        let ip = self.ip().to_ipv6_mapped();
        let port = self.port;
        SocketAddressV6 { ip, port }
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
    #[inline]
    fn is_unspecified(&self) -> bool {
        Self::UNSPECIFIED.eq(self)
    }
}

test_inet_snapshot!(socket_v4, socket_v4_snapshot_test, SocketAddressV4);

impl From<[u8; IPV4_LEN]> for IpV4Address {
    #[inline]
    fn from(octets: [u8; IPV4_LEN]) -> Self {
        Self { octets }
    }
}

impl From<IpV4Address> for [u8; IPV4_LEN] {
    #[inline]
    fn from(address: IpV4Address) -> Self {
        address.octets
    }
}

#[cfg(any(test, feature = "std"))]
mod std_conversion {
    use super::*;
    use std::net;

    impl From<net::Ipv4Addr> for IpV4Address {
        fn from(address: net::Ipv4Addr) -> Self {
            (&address).into()
        }
    }

    impl From<&net::Ipv4Addr> for IpV4Address {
        fn from(address: &net::Ipv4Addr) -> Self {
            address.octets().into()
        }
    }

    impl From<IpV4Address> for net::Ipv4Addr {
        fn from(address: IpV4Address) -> Self {
            address.octets.into()
        }
    }

    impl From<net::SocketAddrV4> for SocketAddressV4 {
        fn from(address: net::SocketAddrV4) -> Self {
            let ip = address.ip().into();
            let port = address.port().into();
            Self { ip, port }
        }
    }

    impl From<SocketAddressV4> for net::SocketAddrV4 {
        fn from(address: SocketAddressV4) -> Self {
            let ip = address.ip.into();
            let port = address.port.into();
            Self::new(ip, port)
        }
    }

    impl From<&SocketAddressV4> for net::SocketAddrV4 {
        fn from(address: &SocketAddressV4) -> Self {
            let ip = address.ip.into();
            let port = address.port.into();
            Self::new(ip, port)
        }
    }

    impl From<SocketAddressV4> for net::SocketAddr {
        fn from(address: SocketAddressV4) -> Self {
            let addr: net::SocketAddrV4 = address.into();
            addr.into()
        }
    }

    impl From<&SocketAddressV4> for net::SocketAddr {
        fn from(address: &SocketAddressV4) -> Self {
            let addr: net::SocketAddrV4 = address.into();
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

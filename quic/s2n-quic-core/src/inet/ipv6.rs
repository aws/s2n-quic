// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::inet::unspecified::Unspecified;
use core::fmt;
use s2n_codec::zerocopy::U16;

//= https://tools.ietf.org/rfc/rfc2373.txt#2.0
//# IPv6 addresses are 128-bit identifiers for interfaces and sets of interfaces.
const IPV6_LEN: usize = 128 / 8;

define_inet_type!(
    pub struct IpV6Address {
        octets: [u8; IPV6_LEN],
    }
);

impl IpV6Address {
    /// An unspecified IpV6Address
    pub const UNSPECIFIED: Self = Self {
        octets: [0; IPV6_LEN],
    };

    #[inline]
    pub fn segments(&self) -> [u16; 8] {
        let octets = &self.octets;
        [
            u16::from_be_bytes([octets[0], octets[1]]),
            u16::from_be_bytes([octets[2], octets[3]]),
            u16::from_be_bytes([octets[4], octets[5]]),
            u16::from_be_bytes([octets[6], octets[7]]),
            u16::from_be_bytes([octets[8], octets[9]]),
            u16::from_be_bytes([octets[10], octets[11]]),
            u16::from_be_bytes([octets[12], octets[13]]),
            u16::from_be_bytes([octets[14], octets[15]]),
        ]
    }
}

impl fmt::Debug for IpV6Address {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "IPv6Address({})", self)
    }
}

impl fmt::Display for IpV6Address {
    #[allow(clippy::many_single_char_names)]
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        match self.segments() {
            [0, 0, 0, 0, 0, 0, 0, 0] => write!(fmt, "::"),
            [0, 0, 0, 0, 0, 0, 0, 1] => write!(fmt, "::1"),
            // Ipv4 Compatible address
            [0, 0, 0, 0, 0, 0, g, h] => write!(
                fmt,
                "::{}.{}.{}.{}",
                (g >> 8) as u8,
                g as u8,
                (h >> 8) as u8,
                h as u8
            ),
            // Ipv4-Mapped address
            [0, 0, 0, 0, 0, 0xffff, g, h] => write!(
                fmt,
                "::ffff:{}.{}.{}.{}",
                (g >> 8) as u8,
                g as u8,
                (h >> 8) as u8,
                h as u8
            ),
            // TODO better formatting
            [a, b, c, d, e, f, g, h] => write!(
                fmt,
                "{:x}:{:x}:{:x}:{:x}:{:x}:{:x}:{:x}:{:x}",
                a, b, c, d, e, f, g, h
            ),
        }
    }
}

impl Unspecified for IpV6Address {
    #[inline]
    fn is_unspecified(&self) -> bool {
        Self::UNSPECIFIED.eq(self)
    }
}

test_inet_snapshot!(ipv6, ipv6_snapshot_test, IpV6Address);

define_inet_type!(
    pub struct SocketAddressV6 {
        ip: IpV6Address,
        port: U16,
    }
);

impl SocketAddressV6 {
    /// An unspecified SocketAddressV6
    pub const UNSPECIFIED: Self = Self {
        ip: IpV6Address::UNSPECIFIED,
        port: U16::ZERO,
    };

    #[inline]
    pub const fn ip(&self) -> &IpV6Address {
        &self.ip
    }

    #[inline]
    pub fn port(&self) -> u16 {
        self.port.into()
    }

    #[inline]
    pub fn set_port(&mut self, port: u16) {
        self.port.set(port)
    }
}

impl fmt::Debug for SocketAddressV6 {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "SocketAddressV6({})", self)
    }
}

impl fmt::Display for SocketAddressV6 {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "[{}]:{:?}", self.ip, self.port)
    }
}

impl Unspecified for SocketAddressV6 {
    #[inline]
    fn is_unspecified(&self) -> bool {
        Self::UNSPECIFIED.eq(self)
    }
}

test_inet_snapshot!(socket_v6, socket_v6_snapshot_test, SocketAddressV6);

impl From<[u8; IPV6_LEN]> for IpV6Address {
    #[inline]
    fn from(octets: [u8; IPV6_LEN]) -> Self {
        Self { octets }
    }
}

impl From<IpV6Address> for [u8; IPV6_LEN] {
    #[inline]
    fn from(v: IpV6Address) -> Self {
        v.octets
    }
}

#[cfg(any(test, feature = "std"))]
mod std_conversion {
    use super::*;
    use std::net;

    impl From<net::Ipv6Addr> for IpV6Address {
        fn from(address: net::Ipv6Addr) -> Self {
            (&address).into()
        }
    }

    impl From<&net::Ipv6Addr> for IpV6Address {
        fn from(address: &net::Ipv6Addr) -> Self {
            address.octets().into()
        }
    }

    impl From<IpV6Address> for net::Ipv6Addr {
        fn from(address: IpV6Address) -> Self {
            address.octets.into()
        }
    }

    impl From<net::SocketAddrV6> for SocketAddressV6 {
        fn from(address: net::SocketAddrV6) -> Self {
            let ip = address.ip().into();
            let port = address.port().into();
            Self { ip, port }
        }
    }

    impl From<SocketAddressV6> for net::SocketAddrV6 {
        fn from(address: SocketAddressV6) -> Self {
            let ip = address.ip.into();
            let port = address.port.into();
            Self::new(ip, port, 0, 0)
        }
    }

    impl From<&SocketAddressV6> for net::SocketAddrV6 {
        fn from(address: &SocketAddressV6) -> Self {
            let ip = address.ip.into();
            let port = address.port.into();
            Self::new(ip, port, 0, 0)
        }
    }

    impl From<SocketAddressV6> for net::SocketAddr {
        fn from(address: SocketAddressV6) -> Self {
            let addr: net::SocketAddrV6 = address.into();
            addr.into()
        }
    }

    impl From<&SocketAddressV6> for net::SocketAddr {
        fn from(address: &SocketAddressV6) -> Self {
            let addr: net::SocketAddrV6 = address.into();
            addr.into()
        }
    }

    impl net::ToSocketAddrs for SocketAddressV6 {
        type Iter = std::iter::Once<net::SocketAddr>;

        fn to_socket_addrs(&self) -> std::io::Result<Self::Iter> {
            let ip = self.ip.into();
            let port = self.port.into();
            let addr = net::SocketAddrV6::new(ip, port, 0, 0);
            Ok(std::iter::once(addr.into()))
        }
    }
}

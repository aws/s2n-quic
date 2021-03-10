// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::inet::unspecified::Unspecified;
use core::fmt;
use s2n_codec::zerocopy::U16;

//= https://tools.ietf.org/rfc/rfc2373.txt#2.0
//# IPv6 addresses are 128-bit identifiers for interfaces and sets of interfaces.
const IPV6_LEN: usize = 128 / 8;

define_inet_type!(
    pub struct IPv6Address {
        octets: [u8; IPV6_LEN],
    }
);

impl IPv6Address {
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

impl fmt::Debug for IPv6Address {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "IPv6Address({})", self)
    }
}

impl fmt::Display for IPv6Address {
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

impl Unspecified for IPv6Address {
    fn is_unspecified(&self) -> bool {
        <[u8; IPV6_LEN]>::default().eq(&self.octets)
    }
}

test_inet_snapshot!(ipv6, ipv6_snapshot_test, IPv6Address);

define_inet_type!(
    pub struct SocketAddressV6 {
        ip: IPv6Address,
        port: U16,
    }
);

impl SocketAddressV6 {
    pub const fn ip(&self) -> &IPv6Address {
        &self.ip
    }

    pub fn port(&self) -> u16 {
        self.port.into()
    }

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
    fn is_unspecified(&self) -> bool {
        self.ip.is_unspecified() && self.port.is_unspecified()
    }
}

test_inet_snapshot!(socket_v6, socket_v6_snapshot_test, SocketAddressV6);

impl From<[u8; IPV6_LEN]> for IPv6Address {
    fn from(octets: [u8; IPV6_LEN]) -> Self {
        Self { octets }
    }
}

impl Into<[u8; IPV6_LEN]> for IPv6Address {
    fn into(self) -> [u8; IPV6_LEN] {
        self.octets
    }
}

#[cfg(any(test, feature = "std"))]
mod std_conversion {
    use super::*;
    use std::net;

    impl From<net::Ipv6Addr> for IPv6Address {
        fn from(address: net::Ipv6Addr) -> Self {
            (&address).into()
        }
    }

    impl From<&net::Ipv6Addr> for IPv6Address {
        fn from(address: &net::Ipv6Addr) -> Self {
            address.octets().into()
        }
    }

    impl Into<net::Ipv6Addr> for IPv6Address {
        fn into(self) -> net::Ipv6Addr {
            self.octets.into()
        }
    }

    impl From<net::SocketAddrV6> for SocketAddressV6 {
        fn from(address: net::SocketAddrV6) -> Self {
            let ip = address.ip().into();
            let port = address.port().into();
            Self { ip, port }
        }
    }

    impl Into<net::SocketAddrV6> for SocketAddressV6 {
        fn into(self) -> net::SocketAddrV6 {
            let ip = self.ip.into();
            let port = self.port.into();
            net::SocketAddrV6::new(ip, port, 0, 0)
        }
    }

    impl Into<net::SocketAddrV6> for &SocketAddressV6 {
        fn into(self) -> net::SocketAddrV6 {
            let ip = self.ip.into();
            let port = self.port.into();
            net::SocketAddrV6::new(ip, port, 0, 0)
        }
    }

    impl Into<net::SocketAddr> for SocketAddressV6 {
        fn into(self) -> net::SocketAddr {
            let addr: net::SocketAddrV6 = self.into();
            addr.into()
        }
    }

    impl Into<net::SocketAddr> for &SocketAddressV6 {
        fn into(self) -> net::SocketAddr {
            let addr: net::SocketAddrV6 = self.into();
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

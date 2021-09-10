// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::inet::{
    ip, ipv4::IpV4Address, unspecified::Unspecified, IpAddress, IpV4Address, SocketAddress,
    SocketAddressV4,
};
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
    pub const fn segments(&self) -> [u16; 8] {
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

    /// Converts the IP address into IPv4 if it is mapped, otherwise the address is unchanged
    #[inline]
    pub fn unmap(self) -> IpAddress {
        match self.octets {
            [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xff, 0xff, a, b, c, d] => {
                IpV4Address::new([a, b, c, d]).into()
            }
            _ => self.into(),
        }
    }

    /// Returns the [`ip::RangeType`] for the given address
    ///
    /// See the [IANA Registry](https://www.iana.org/assignments/ipv6-address-space/ipv6-address-space.xhtml)
    /// for more details.
    ///
    /// ```
    /// use s2n_quic_core::inet::{IpV6Address, ip::RangeType::*};
    ///
    /// assert_eq!(IpV6Address::from([0, 0, 0, 0, 0, 0, 0, 0]).range_type(), Unspecified);
    /// assert_eq!(IpV6Address::from([0, 0, 0, 0, 0, 0, 0, 1]).range_type(), Loopback);
    /// assert_eq!(IpV6Address::from([0xff0e, 0, 0, 0, 0, 0, 0, 0]).range_type(), Broadcast);
    /// assert_eq!(IpV6Address::from([0xfe80, 0, 0, 0, 0, 0, 0, 0]).range_type(), LinkLocal);
    /// assert_eq!(IpV6Address::from([0xfc02, 0, 0, 0, 0, 0, 0, 0]).range_type(), Private);
    /// assert_eq!(IpV6Address::from([0x2001, 0xdb8, 0, 0, 0, 0, 0, 0]).range_type(), Documentation);
    /// // IPv4-mapped address
    /// assert_eq!(IpV6Address::from([0, 0, 0, 0, 0, 0xffff, 0xc00a, 0x2ff]).range_type(), Global);
    /// ```
    #[inline]
    pub const fn range_type(self) -> ip::RangeType {
        use ip::RangeType::*;

        // https://www.iana.org/assignments/ipv6-address-space/ipv6-address-space.xhtml
        match self.segments() {
            //= https://www.rfc-editor.org/rfc/rfc4291.txt#2.5.2
            //# The address 0:0:0:0:0:0:0:0 is called the unspecified address.
            [0, 0, 0, 0, 0, 0, 0, 0] => Unspecified,

            //= https://www.rfc-editor.org/rfc/rfc4291.txt#2.5.3
            //# The unicast address 0:0:0:0:0:0:0:1 is called the loopback address.
            [0, 0, 0, 0, 0, 0, 0, 1] => Loopback,

            //= https://www.rfc-editor.org/rfc/rfc4291.txt#2.5.5.1
            //# The format of the "IPv4-Compatible IPv6 address" is as
            //# follows:
            //#
            //# |                80 bits               | 16 |      32 bits        |
            //# +--------------------------------------+--------------------------+
            //# |0000..............................0000|0000|    IPv4 address     |
            //# +--------------------------------------+----+---------------------+

            //= https://www.rfc-editor.org/rfc/rfc4291.txt#2.5.5.2
            //# The format of the "IPv4-mapped IPv6
            //# address" is as follows:
            //#
            //# |                80 bits               | 16 |      32 bits        |
            //# +--------------------------------------+--------------------------+
            //# |0000..............................0000|FFFF|    IPv4 address     |
            //# +--------------------------------------+----+---------------------+
            [0, 0, 0, 0, 0, 0, a, b] | [0, 0, 0, 0, 0, 0xffff, a, b] => {
                let [c, d] = u16::to_be_bytes(b);
                let [a, b] = u16::to_be_bytes(a);
                IpV4Address {
                    octets: [a, b, c, d],
                }
                .range_type()
            }

            //= https://www.rfc-editor.org/rfc/rfc4291.txt#2.7
            //# binary 11111111 at the start of the address identifies the address
            //# as being a multicast address.
            [a, ..] if a & 0xff00 == 0xff00 => Broadcast,

            //= https://www.rfc-editor.org/rfc/rfc4291.txt#2.5.6
            //# Link-Local addresses have the following format:
            //# |   10     |
            //# |  bits    |         54 bits         |          64 bits           |
            //# +----------+-------------------------+----------------------------+
            //# |1111111010|           0             |       interface ID         |
            //# +----------+-------------------------+----------------------------+
            [a, ..] if a & 0xffc0 == 0xfe80 => LinkLocal,

            //= https://www.rfc-editor.org/rfc/rfc4193.txt#8
            //# The IANA has assigned the FC00::/7 prefix to "Unique Local Unicast".
            [a, ..] if a & 0xfe00 == 0xfc00 => Private,

            //= https://www.rfc-editor.org/rfc/rfc3849.txt#4
            //# IANA is to record the allocation of the IPv6 global unicast address
            //# prefix  2001:DB8::/32 as a documentation-only prefix  in the IPv6
            //# address registry.
            [0x2001, 0xdb8, ..] => Documentation,

            // Everything else is considered globally-reachable
            _ => Global,
        }
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

    /// Converts the IP address into IPv4 if it is mapped, otherwise the address is unchanged
    #[inline]
    pub fn unmap(self) -> SocketAddress {
        match self.ip.unmap() {
            IpAddress::Ipv4(addr) => SocketAddressV4::new(addr, self.port).into(),
            IpAddress::Ipv6(_) => self.into(),
        }
    }

    #[inline]
    pub const fn range_type(&self) -> ip::RangeType {
        self.ip.range_type()
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

impl From<[u16; IPV6_LEN / 2]> for IpV6Address {
    #[inline]
    fn from(octets: [u16; IPV6_LEN / 2]) -> Self {
        macro_rules! convert {
            ($($segment:ident),*) => {{
                let [$($segment),*] = octets;
                $(
                    let $segment = u16::to_be_bytes($segment);
                )*
                Self {
                    octets: [
                        $(
                            $segment[0],
                            $segment[1],
                        )*
                    ]
                }
            }}
        }
        convert!(a, b, c, d, e, f, g, h)
    }
}

impl From<IpV6Address> for [u8; IPV6_LEN] {
    #[inline]
    fn from(v: IpV6Address) -> Self {
        v.octets
    }
}

impl From<IpV6Address> for [u16; IPV6_LEN / 2] {
    #[inline]
    fn from(v: IpV6Address) -> Self {
        v.segments()
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

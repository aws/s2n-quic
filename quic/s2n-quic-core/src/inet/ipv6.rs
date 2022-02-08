// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::inet::{
    ip, ipv4::IpV4Address, unspecified::Unspecified, IpAddress, SocketAddress, SocketAddressV4,
};
use core::fmt;
use s2n_codec::zerocopy::U16;

//= https://www.rfc-editor.org/rfc/rfc2373#section-2.0
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
    pub const fn unmap(self) -> IpAddress {
        match self.segments() {
            // special-case unspecified and loopback
            [0, 0, 0, 0, 0, 0, 0, 0] => IpAddress::Ipv6(self),
            [0, 0, 0, 0, 0, 0, 0, 1] => IpAddress::Ipv6(self),

            //= https://www.rfc-editor.org/rfc/rfc4291#section-2.5.5.1
            //# The format of the "IPv4-Compatible IPv6 address" is as
            //# follows:
            //#
            //# |                80 bits               | 16 |      32 bits        |
            //# +--------------------------------------+--------------------------+
            //# |0000..............................0000|0000|    IPv4 address     |
            //# +--------------------------------------+----+---------------------+

            //= https://www.rfc-editor.org/rfc/rfc4291#section-2.5.5.2
            //# The format of the "IPv4-mapped IPv6
            //# address" is as follows:
            //#
            //# |                80 bits               | 16 |      32 bits        |
            //# +--------------------------------------+--------------------------+
            //# |0000..............................0000|FFFF|    IPv4 address     |
            //# +--------------------------------------+----+---------------------+

            //= https://www.rfc-editor.org/rfc/rfc6052#section-2.1
            //# This document reserves a "Well-Known Prefix" for use in an
            //# algorithmic mapping.  The value of this IPv6 prefix is:
            //#
            //#   64:ff9b::/96
            [0, 0, 0, 0, 0, 0, ab, cd]
            | [0, 0, 0, 0, 0, 0xffff, ab, cd]
            | [0x64, 0xff9b, 0, 0, 0, 0, ab, cd] => {
                let [a, b] = u16::to_be_bytes(ab);
                let [c, d] = u16::to_be_bytes(cd);
                IpAddress::Ipv4(IpV4Address {
                    octets: [a, b, c, d],
                })
            }
            _ => IpAddress::Ipv6(self),
        }
    }

    /// Returns the [`ip::UnicastScope`] for the given address
    ///
    /// See the [IANA Registry](https://www.iana.org/assignments/ipv6-address-space/ipv6-address-space.xhtml)
    /// for more details.
    ///
    /// ```
    /// use s2n_quic_core::inet::{IpV4Address, IpV6Address, ip::UnicastScope::*};
    ///
    /// assert_eq!(IpV6Address::from([0, 0, 0, 0, 0, 0, 0, 0]).unicast_scope(), None);
    /// assert_eq!(IpV6Address::from([0, 0, 0, 0, 0, 0, 0, 1]).unicast_scope(), Some(Loopback));
    /// assert_eq!(IpV6Address::from([0xff0e, 0, 0, 0, 0, 0, 0, 0]).unicast_scope(), None);
    /// assert_eq!(IpV6Address::from([0xfe80, 0, 0, 0, 0, 0, 0, 0]).unicast_scope(), Some(LinkLocal));
    /// assert_eq!(IpV6Address::from([0xfc02, 0, 0, 0, 0, 0, 0, 0]).unicast_scope(), Some(Private));
    /// // documentation
    /// assert_eq!(IpV6Address::from([0x2001, 0xdb8, 0, 0, 0, 0, 0, 0]).unicast_scope(), None);
    /// // benchmarking
    /// assert_eq!(IpV6Address::from([0x2001, 0x0200, 0, 0, 0, 0, 0, 0]).unicast_scope(), None);
    /// // IPv4-mapped address
    /// assert_eq!(IpV4Address::from([92, 88, 99, 123]).to_ipv6_mapped().unicast_scope(), Some(Global));
    /// ```
    #[inline]
    pub const fn unicast_scope(self) -> Option<ip::UnicastScope> {
        use ip::UnicastScope::*;

        // If this is an IpV4 ip, delegate to that implementation
        if let IpAddress::Ipv4(ip) = self.unmap() {
            return ip.unicast_scope();
        }

        // https://www.iana.org/assignments/ipv6-address-space/ipv6-address-space.xhtml
        match self.segments() {
            //= https://www.rfc-editor.org/rfc/rfc4291#section-2.5.2
            //# The address 0:0:0:0:0:0:0:0 is called the unspecified address.
            [0, 0, 0, 0, 0, 0, 0, 0] => None,

            //= https://www.rfc-editor.org/rfc/rfc4291#section-2.5.3
            //# The unicast address 0:0:0:0:0:0:0:1 is called the loopback address.
            [0, 0, 0, 0, 0, 0, 0, 1] => Some(Loopback),

            //= https://www.rfc-editor.org/rfc/rfc6666.html#section-4
            //# Per this document, IANA has recorded the allocation of the IPv6
            //# address prefix 0100::/64 as a Discard-Only Prefix in the "Internet
            //# Protocol Version 6 Address Space" and added the prefix to the "IANA
            //# IPv6 Special Purpose Address Registry" [IANA-IPV6REG].
            [0x0100, 0, 0, 0, ..] => None,

            //= https://www.rfc-editor.org/rfc/rfc7723.html#section-4.2
            //# +----------------------+-------------------------------------------+
            //# | Attribute            | Value                                     |
            //# +----------------------+-------------------------------------------+
            //# | Address Block        | 2001:1::1/128                             |
            //# | Name                 | Port Control Protocol Anycast             |
            //# | RFC                  | RFC 7723 (this document)                  |
            //# | Allocation Date      | October 2015                              |
            //# | Termination Date     | N/A                                       |
            //# | Source               | True                                      |
            //# | Destination          | True                                      |
            //# | Forwardable          | True                                      |
            //# | Global               | True                                      |
            //# | Reserved-by-Protocol | False                                     |
            //# +----------------------+-------------------------------------------+
            [0x2001, 0x1, 0, 0, 0, 0, 0, 0x1] => Some(Global),

            //= https://www.rfc-editor.org/rfc/rfc8155.html#section-8.2
            //# +----------------------+-------------------------------------------+
            //# | Attribute            | Value                                     |
            //# +----------------------+-------------------------------------------+
            //# | Address Block        | 2001:1::2/128                             |
            //# | Name                 | Traversal Using Relays around NAT Anycast |
            //# | RFC                  | RFC 8155                                  |
            //# | Allocation Date      | 2017-02                                   |
            //# | Termination Date     | N/A                                       |
            //# | Source               | True                                      |
            //# | Destination          | True                                      |
            //# | Forwardable          | True                                      |
            //# | Global               | True                                      |
            //# | Reserved-by-Protocol | False                                     |
            //# +----------------------+-------------------------------------------+
            [0x2001, 0x1, 0, 0, 0, 0, 0, 0x2] => Some(Global),

            //= https://www.rfc-editor.org/rfc/rfc6890#section-2.2.3
            //# +----------------------+---------------------------+
            //# | Attribute            | Value                     |
            //# +----------------------+---------------------------+
            //# | Address Block        | 2001::/23                 |
            //# | Name                 | IETF Protocol Assignments |
            //# | RFC                  | [RFC2928]                 |
            //# | Allocation Date      | September 2000            |
            //# | Termination Date     | N/A                       |
            //# | Source               | False[1]                  |
            //# | Destination          | False[1]                  |
            //# | Forwardable          | False[1]                  |
            //# | Global               | False[1]                  |
            //# | Reserved-by-Protocol | False                     |
            //# +----------------------+---------------------------+
            [0x2001, 0x0..=0x01ff, ..] => None,

            //= https://www.rfc-editor.org/rfc/rfc5180#section-8
            //# The IANA has allocated 2001:0200::/48 for IPv6 benchmarking, which is
            //# a 48-bit prefix from the RFC 4773 pool.
            [0x2001, 0x0200, 0, ..] => None,

            //= https://www.rfc-editor.org/rfc/rfc3849#section-4
            //# IANA is to record the allocation of the IPv6 global unicast address
            //# prefix  2001:DB8::/32 as a documentation-only prefix  in the IPv6
            //# address registry.
            [0x2001, 0xdb8, ..] => None,

            //= https://www.rfc-editor.org/rfc/rfc4193#section-8
            //# The IANA has assigned the FC00::/7 prefix to "Unique Local Unicast".
            [0xfc00..=0xfdff, ..] => {
                //= https://www.rfc-editor.org/rfc/rfc4193#section-1
                //# They are not
                //# expected to be routable on the global Internet.  They are routable
                //# inside of a more limited area such as a site.  They may also be
                //# routed between a limited set of sites.
                Some(Private)
            }

            //= https://www.rfc-editor.org/rfc/rfc4291#section-2.5.6
            //# Link-Local addresses have the following format:
            //# |   10     |
            //# |  bits    |         54 bits         |          64 bits           |
            //# +----------+-------------------------+----------------------------+
            //# |1111111010|           0             |       interface ID         |
            //# +----------+-------------------------+----------------------------+
            [0xfe80..=0xfebf, ..] => Some(LinkLocal),

            //= https://www.rfc-editor.org/rfc/rfc4291#section-2.7
            //# binary 11111111 at the start of the address identifies the address
            //# as being a multicast address.
            [0xff00..=0xffff, ..] => None,

            // Everything else is considered globally-reachable
            _ => Some(Global),
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
    pub const fn unicast_scope(&self) -> Option<ip::UnicastScope> {
        self.ip.unicast_scope()
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

    impl From<(net::Ipv6Addr, u16)> for SocketAddressV6 {
        fn from((ip, port): (net::Ipv6Addr, u16)) -> Self {
            Self::new(ip, port)
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

#[cfg(test)]
mod tests {
    use super::*;
    use bolero::{check, generator::*};

    /// Asserts the UnicastScope returned matches a known implementation
    #[test]
    fn scope_test() {
        let g = gen::<[u8; 16]>().map_gen(IpV6Address::from);
        check!().with_generator(g).cloned().for_each(|subject| {
            use ip::UnicastScope::*;

            // the ipv4 scopes are tested elsewhere there so we just make sure the scopes match
            if let IpAddress::Ipv4(ipv4) = subject.unmap() {
                assert_eq!(ipv4.unicast_scope(), subject.unicast_scope());
                return;
            }

            let expected = std::net::Ipv6Addr::from(subject);
            let network = ip_network::Ipv6Network::from(expected);

            match subject.unicast_scope() {
                Some(Global) => {
                    // Site-local addresses are deprecated but the `ip_network` still partitions
                    // them out
                    // See: https://datatracker.ietf.org/doc/html/rfc3879

                    assert!(network.is_global() || network.is_unicast_site_local());
                }
                Some(Private) => {
                    assert!(network.is_unique_local());
                }
                Some(Loopback) => {
                    assert!(expected.is_loopback());
                }
                Some(LinkLocal) => {
                    assert!(network.is_unicast_link_local());
                }
                None => {
                    assert!(
                        expected.is_multicast()
                            || expected.is_unspecified()
                            // Discard space
                            || subject.segments()[0] == 0x0100
                            // IETF Reserved
                            || subject.segments()[0] == 0x2001
                    );
                }
            }
        })
    }
}

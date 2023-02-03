// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::inet::{
    ip,
    ipv6::{IpV6Address, SocketAddressV6},
    unspecified::Unspecified,
};
use core::{fmt, mem::size_of};
use s2n_codec::zerocopy::U16;

//= https://www.rfc-editor.org/rfc/rfc791#section-2.3
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

    /// Returns the [`ip::UnicastScope`] for the given address
    ///
    /// See the [IANA Registry](https://www.iana.org/assignments/iana-ipv4-special-registry/iana-ipv4-special-registry.xhtml)
    /// for more details.
    ///
    /// ```
    /// use s2n_quic_core::inet::{IpV4Address, ip::UnicastScope::*};
    ///
    /// assert_eq!(IpV4Address::from([0, 0, 0, 0]).unicast_scope(), None);
    /// assert_eq!(IpV4Address::from([127, 0, 0, 1]).unicast_scope(), Some(Loopback));
    /// assert_eq!(IpV4Address::from([127, 1, 1, 1]).unicast_scope(), Some(Loopback));
    /// assert_eq!(IpV4Address::from([10, 0, 0, 1]).unicast_scope(), Some(Private));
    /// assert_eq!(IpV4Address::from([100, 64, 0, 1]).unicast_scope(), Some(Private));
    /// assert_eq!(IpV4Address::from([169, 254, 1, 2]).unicast_scope(), Some(LinkLocal));
    /// assert_eq!(IpV4Address::from([192, 0, 0, 1]).unicast_scope(), None);
    /// assert_eq!(IpV4Address::from([192, 0, 0, 9]).unicast_scope(), Some(Global));
    /// assert_eq!(IpV4Address::from([192, 0, 0, 10]).unicast_scope(), Some(Global));
    /// assert_eq!(IpV4Address::from([192, 0, 2, 1]).unicast_scope(), None);
    /// assert_eq!(IpV4Address::from([198, 18, 0, 0]).unicast_scope(), None);
    /// assert_eq!(IpV4Address::from([198, 19, 1, 1]).unicast_scope(), None);
    /// assert_eq!(IpV4Address::from([233, 252, 0, 1]).unicast_scope(), None);
    /// assert_eq!(IpV4Address::from([240, 255, 255, 255]).unicast_scope(), None);
    /// assert_eq!(IpV4Address::from([255, 255, 255, 255]).unicast_scope(), None);
    /// assert_eq!(IpV4Address::from([92, 88, 99, 123]).unicast_scope(), Some(Global));
    /// assert_eq!(IpV4Address::from([168, 254, 169, 253]).unicast_scope(), Some(Global));
    /// assert_eq!(IpV4Address::from([224, 0, 0, 1]).unicast_scope(), Some(Global));
    /// ```
    #[inline]
    pub const fn unicast_scope(self) -> Option<ip::UnicastScope> {
        use ip::UnicastScope::*;

        // https://www.iana.org/assignments/iana-ipv4-special-registry/iana-ipv4-special-registry.xhtml
        //
        // NOTE: Even though 192.88.99.0/24 is reserved for "6to4 Relay Anycast", it has been
        //       deprecated so this code considers it as `Global`.
        match self.octets {
            // NOTE: this RFC doesn't quite follow modern formatting so it doesn't parse with the
            // compliance tool
            // https://www.rfc-editor.org/rfc/rfc1122#section-3.2.1.3
            // (a)  { 0, 0 }
            //
            //     This host on this network.  MUST NOT be sent, except as
            //     a source address as part of an initialization procedure
            //     by which the host learns its own IP address.
            //
            //     See also Section 3.3.6 for a non-standard use of {0,0}.
            [0, 0, 0, 0] => None,

            // (b)  { 0, <Host-number> }
            //
            //     Specified host on this network.  It MUST NOT be sent,
            //     except as a source address as part of an initialization
            //     procedure by which the host learns its full IP address.
            [0, _, _, _] => None,

            // NOTE: this RFC doesn't quite follow modern formatting so it doesn't parse with the
            // compliance tool
            // https://www.rfc-editor.org/rfc/rfc1122#section-3.2.1.3
            // (g)  { 127, <any> }
            //
            //   Internal host loopback address.  Addresses of this form
            //   MUST NOT appear outside a host.
            [127, _, _, _] => Some(Loopback),

            //= https://www.rfc-editor.org/rfc/rfc1918#section-3
            //# The Internet Assigned Numbers Authority (IANA) has reserved the
            //# following three blocks of the IP address space for private internets:
            //#
            //# 10.0.0.0        -   10.255.255.255  (10/8 prefix)
            //# 172.16.0.0      -   172.31.255.255  (172.16/12 prefix)
            //# 192.168.0.0     -   192.168.255.255 (192.168/16 prefix)
            [10, _, _, _] => Some(Private),
            [172, 16..=31, _, _] => Some(Private),
            [192, 168, _, _] => Some(Private),

            //= https://www.rfc-editor.org/rfc/rfc6598#section-7
            //# The Shared Address Space address range is 100.64.0.0/10.
            [100, 64..=127, _, _] => {
                //= https://www.rfc-editor.org/rfc/rfc6598#section-1
                //# Shared Address Space is similar to [RFC1918] private address space in
                //# that it is not globally routable address space and can be used by
                //# multiple pieces of equipment.
                Some(Private)
            }

            //= https://www.rfc-editor.org/rfc/rfc3927#section-8
            //# The IANA has allocated the prefix 169.254/16 for the use described in
            //# this document.
            [169, 254, _, _] => Some(LinkLocal),

            //= https://www.rfc-editor.org/rfc/rfc7723#section-4.1
            //# +----------------------+-------------------------------------------+
            //# | Attribute            | Value                                     |
            //# +----------------------+-------------------------------------------+
            //# | Address Block        | 192.0.0.9/32                              |
            //# | Name                 | Port Control Protocol Anycast             |
            //# | RFC                  | RFC 7723 (this document)                  |
            //# | Allocation Date      | October 2015                              |
            //# | Termination Date     | N/A                                       |
            //# | Source               | True                                      |
            //# | Destination          | True                                      |
            //# | Forwardable          | True                                      |
            //# | Global               | True                                      |
            [192, 0, 0, 9] => Some(Global),

            //= https://www.rfc-editor.org/rfc/rfc8155#section-8.1
            //# +----------------------+-------------------------------------------+
            //# | Attribute            | Value                                     |
            //# +----------------------+-------------------------------------------+
            //# | Address Block        | 192.0.0.10/32                             |
            //# | Name                 | Traversal Using Relays around NAT Anycast |
            //# | RFC                  | RFC 8155                                  |
            //# | Allocation Date      | 2017-02                                   |
            //# | Termination Date     | N/A                                       |
            //# | Source               | True                                      |
            //# | Destination          | True                                      |
            //# | Forwardable          | True                                      |
            //# | Global               | True                                      |
            [192, 0, 0, 10] => Some(Global),

            //= https://www.rfc-editor.org/rfc/rfc6890#section-2.1
            //# Table 7 of this document records the assignment of an IPv4 address
            //# block (192.0.0.0/24) to IANA for IETF protocol assignments.
            [192, 0, 0, _] => None,

            //= https://www.rfc-editor.org/rfc/rfc2544#C.2.2
            //# The network addresses 192.18.0.0 through 198.19.255.255 are have been
            //# assigned to the BMWG by the IANA for this purpose.
            // NOTE: this range should be 198.18.0.0/15 as corrected by https://www.rfc-editor.org/errata/eid423
            [198, 18..=19, _, _] => None,

            //= https://www.rfc-editor.org/rfc/rfc5737#section-3
            //# The blocks 192.0.2.0/24 (TEST-NET-1), 198.51.100.0/24 (TEST-NET-2),
            //# and 203.0.113.0/24 (TEST-NET-3) are provided for use in
            //# documentation.
            [192, 0, 2, _] => None,
            [198, 51, 100, _] => None,
            [203, 0, 113, _] => None,

            //= https://www.rfc-editor.org/rfc/rfc6676#section-2
            //# For Any-Source Multicast (ASM), the IPv4 multicast addresses
            //# allocated for documentation purposes are 233.252.0.0 - 233.252.0.255
            //# (233.252.0.0/24).
            [233, 252, 0, _] => None,

            //= https://www.rfc-editor.org/rfc/rfc1112#section-4
            //# Class E IP addresses, i.e.,
            //# those with "1111" as their high-order four bits, are reserved for
            //# future addressing modes.

            //= https://www.rfc-editor.org/rfc/rfc919#section-7
            //# The address 255.255.255.255 denotes a broadcast on a local hardware
            //# network, which must not be forwarded.
            [240..=255, _, _, _] => None,

            // everything else is considered global
            _ => Some(Global),
        }
    }

    /// Converts the IP address into a IPv6 mapped address
    #[inline]
    pub const fn to_ipv6_mapped(self) -> IpV6Address {
        //= https://www.rfc-editor.org/rfc/rfc5156#section-2.2
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
        write!(fmt, "IPv4Address({self})")
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

    #[inline]
    pub const fn unicast_scope(&self) -> Option<ip::UnicastScope> {
        self.ip.unicast_scope()
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
        write!(fmt, "SocketAddressV4({self})")
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

    impl From<(net::Ipv4Addr, u16)> for SocketAddressV4 {
        fn from((ip, port): (net::Ipv4Addr, u16)) -> Self {
            Self::new(ip, port)
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

#[cfg(test)]
mod tests {
    use super::*;
    use bolero::{check, generator::*};

    /// Asserts the Scope returned matches a known implementation
    #[test]
    fn scope_test() {
        let g = gen::<[u8; 4]>().map_gen(IpV4Address::from);
        check!().with_generator(g).cloned().for_each(|subject| {
            use ip::UnicastScope::*;

            let expected = std::net::Ipv4Addr::from(subject);

            // Several IP methods are blocked behind `feature(ip)`: https://github.com/rust-lang/rust/issues/27709
            //
            // Use the `ip_network` crate to fill any gaps
            let network = ip_network::Ipv4Network::from(expected);

            match subject.unicast_scope() {
                Some(Global) => {
                    // ip_network has a bug in the `is_global` logic. Remove this once its fixed
                    // and published
                    // https://github.com/JakubOnderka/ip_network/pull/7
                    if subject.octets == [192, 0, 0, 9] || subject.octets == [192, 0, 0, 10] {
                        return;
                    }

                    assert!(network.is_global());
                }
                Some(Private) => {
                    assert!(expected.is_private() || network.is_shared_address_space());
                }
                Some(Loopback) => {
                    assert!(expected.is_loopback());
                }
                Some(LinkLocal) => {
                    assert!(expected.is_link_local());
                }
                None => {
                    assert!(
                        expected.is_broadcast()
                            || expected.is_multicast()
                            || expected.is_documentation()
                            || network.is_benchmarking()
                            || network.is_ietf_protocol_assignments()
                            || network.is_reserved()
                            || network.is_local_identification()
                            || network.is_unspecified()
                    );
                }
            }
        })
    }
}

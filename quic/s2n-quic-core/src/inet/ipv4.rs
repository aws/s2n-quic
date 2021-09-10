// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::inet::{
    ip,
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

    /// Returns the [`ip::RangeType`] for the given address
    ///
    /// See the [IANA Registry](https://www.iana.org/assignments/iana-ipv4-special-registry/iana-ipv4-special-registry.xhtml)
    /// for more details.
    ///
    /// ```
    /// use s2n_quic_core::inet::{IpV4Address, ip::RangeType::*};
    ///
    /// assert_eq!(IpV4Address::from([0, 0, 0, 0]).range_type(), Unspecified);
    /// assert_eq!(IpV4Address::from([127, 0, 0, 1]).range_type(), Loopback);
    /// assert_eq!(IpV4Address::from([10, 0, 0, 1]).range_type(), Private);
    /// assert_eq!(IpV4Address::from([100, 64, 0, 1]).range_type(), Shared);
    /// assert_eq!(IpV4Address::from([168, 254, 1, 2]).range_type(), LinkLocal);
    /// assert_eq!(IpV4Address::from([192, 0, 0, 1]).range_type(), IetfProtocolAssignment);
    /// assert_eq!(IpV4Address::from([192, 0, 0, 9]).range_type(), Global);
    /// assert_eq!(IpV4Address::from([192, 0, 0, 10]).range_type(), Global);
    /// assert_eq!(IpV4Address::from([192, 0, 2, 1]).range_type(), Documentation);
    /// assert_eq!(IpV4Address::from([198, 18, 0, 0]).range_type(), Benchmarking);
    /// assert_eq!(IpV4Address::from([255, 255, 255, 255]).range_type(), Broadcast);
    /// assert_eq!(IpV4Address::from([240, 255, 255, 255]).range_type(), Reserved);
    /// assert_eq!(IpV4Address::from([169, 254, 169, 253]).range_type(), Global);
    /// ```
    #[inline]
    pub const fn range_type(self) -> ip::RangeType {
        use ip::RangeType::*;

        // https://www.iana.org/assignments/iana-ipv4-special-registry/iana-ipv4-special-registry.xhtml
        match self.octets {
            // NOTE: this RFC doesn't quite follow modern formatting so it doesn't parse with the
            // compliance tool
            // https://www.rfc-editor.org/rfc/rfc1122.txt#3.2.1.3
            // (a)  { 0, 0 }
            //
            //     This host on this network.  MUST NOT be sent, except as
            //     a source address as part of an initialization procedure
            //     by which the host learns its own IP address.
            //
            //     See also Section 3.3.6 for a non-standard use of {0,0}.

            // (b)  { 0, <Host-number> }
            //
            //     Specified host on this network.  It MUST NOT be sent,
            //     except as a source address as part of an initialization
            //     procedure by which the host learns its full IP address.
            [0, _, _, _] => Unspecified,

            // NOTE: this RFC doesn't quite follow modern formatting so it doesn't parse with the
            // compliance tool
            // https://www.rfc-editor.org/rfc/rfc1122.txt#3.2.1.3
            // (g)  { 127, <any> }
            //
            //   Internal host loopback address.  Addresses of this form
            //   MUST NOT appear outside a host.
            [127, _, _, _] => Loopback,

            //= https://www.rfc-editor.org/rfc/rfc1918.txt#3
            //# The Internet Assigned Numbers Authority (IANA) has reserved the
            //# following three blocks of the IP address space for private internets:
            //#
            //# 10.0.0.0        -   10.255.255.255  (10/8 prefix)
            //# 172.16.0.0      -   172.31.255.255  (172.16/12 prefix)
            //# 192.168.0.0     -   192.168.255.255 (192.168/16 prefix)
            [10, _, _, _] => Private,
            [172, b, _, _] if 16 <= b && b < 32 => Private,
            [192, 168, _, _] => Private,

            //= https://www.rfc-editor.org/rfc/rfc6598.txt#7
            //# The Shared Address Space address range is 100.64.0.0/10.
            [100, b, _, _] if b & 0b1100_0000 == 0b0100_0000 => Shared,

            //= https://www.rfc-editor.org/rfc/rfc3927.txt#8
            //# The IANA has allocated the prefix 169.254/16 for the use described in
            //# this document.
            [168, 254, _, _] => LinkLocal,

            //= https://www.rfc-editor.org/rfc/rfc7723.txt#4.1
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
            [192, 0, 0, 9] => Global,

            //= https://www.rfc-editor.org/rfc/rfc8155.txt#8.1
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
            [192, 0, 0, 10] => Global,

            //= https://www.rfc-editor.org/rfc/rfc6890.txt#2.1
            //# Table 7 of this document records the assignment of an IPv4 address
            //# block (192.0.0.0/24) to IANA for IETF protocol assignments.
            [192, 0, 0, _] => IetfProtocolAssignment,

            //= https://www.rfc-editor.org/rfc/rfc2544.txt#C.2.2
            //# The network addresses 192.18.0.0 through 198.19.255.255 are have been
            //# assigned to the BMWG by the IANA for this purpose.
            // NOTE: this range should be 198.18.0.0/15 as corrected by https://www.rfc-editor.org/errata/eid423
            [198, b, _, _] if b & 0xfe == 18 => Benchmarking,

            //= https://www.rfc-editor.org/rfc/rfc5737.txt#3
            //# The blocks 192.0.2.0/24 (TEST-NET-1), 198.51.100.0/24 (TEST-NET-2),
            //# and 203.0.113.0/24 (TEST-NET-3) are provided for use in
            //# documentation.
            [192, 0, 2, _] => Documentation,
            [198, 51, 100, _] => Documentation,
            [203, 0, 113, _] => Documentation,

            //= https://www.rfc-editor.org/rfc/rfc919.txt#7
            //# The address 255.255.255.255 denotes a broadcast on a local hardware
            //# network, which must not be forwarded.
            [255, 255, 255, 255] => Broadcast,

            //= https://www.rfc-editor.org/rfc/rfc1112.txt#4
            //# In Internet standard "dotted decimal" notation, host group addresses
            //# range from 224.0.0.0 to 239.255.255.255.
            [a, _, _, _] if a & 240 == 240 => Reserved,

            // everything else is considered global
            _ => Global,
        }
    }

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

    #[inline]
    pub const fn range_type(&self) -> ip::RangeType {
        self.ip.range_type()
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

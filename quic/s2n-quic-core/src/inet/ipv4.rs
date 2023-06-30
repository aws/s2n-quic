// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::inet::{
    ip,
    ipv6::{IpV6Address, SocketAddressV6},
    unspecified::Unspecified,
    ExplicitCongestionNotification,
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

    #[inline]
    pub fn with_port(self, port: u16) -> SocketAddressV4 {
        SocketAddressV4 {
            ip: self,
            port: port.into(),
        }
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

//= https://www.rfc-editor.org/rfc/rfc791.html#section-3.1
//#  A summary of the contents of the internet header follows:
//#
//#
//#    0                   1                   2                   3
//#    0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
//#   +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//#   |Version|  IHL  |Type of Service|          Total Length         |
//#   +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//#   |         Identification        |Flags|      Fragment Offset    |
//#   +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//#   |  Time to Live |    Protocol   |         Header Checksum       |
//#   +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//#   |                       Source Address                          |
//#   +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//#   |                    Destination Address                        |
//#   +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//#   |                    Options                    |    Padding    |
//#   +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+

// Note that Options and Padding are variable length depending on the IHL field
// so they can't be included directly in the fixed-sized struct.
define_inet_type!(
    pub struct Header {
        vihl: Vihl,
        tos: Tos,
        total_len: U16,
        id: U16,
        flag_fragment: FlagFragment,
        ttl: u8,
        protocol: ip::Protocol,
        checksum: U16,
        source: IpV4Address,
        destination: IpV4Address,
    }
);

impl fmt::Debug for Header {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("ipv4::Header")
            .field("version", &self.vihl.version())
            .field("header_len", &self.vihl.header_len())
            .field("dscp", &self.tos.dscp())
            .field("ecn", &self.tos.ecn())
            .field("total_len", &self.total_len)
            .field("id", &format_args!("0x{:04x}", self.id.get()))
            .field("flags (reserved)", &self.flag_fragment.reserved())
            .field(
                "flags (don't fragment)",
                &self.flag_fragment.dont_fragment(),
            )
            .field(
                "flags (more fragments)",
                &self.flag_fragment.more_fragments(),
            )
            .field("fragment_offset", &self.flag_fragment.fragment_offset())
            .field("ttl", &self.ttl)
            .field("protocol", &self.protocol)
            .field("checksum", &format_args!("0x{:04x}", self.checksum.get()))
            .field("source", &self.source)
            .field("destination", &self.destination)
            .finish()
    }
}

impl Header {
    /// Swaps the direction of the header
    #[inline]
    pub fn swap(&mut self) {
        core::mem::swap(&mut self.source, &mut self.destination)
    }

    #[inline]
    pub const fn vihl(&self) -> &Vihl {
        &self.vihl
    }

    #[inline]
    pub fn vihl_mut(&mut self) -> &mut Vihl {
        &mut self.vihl
    }

    #[inline]
    pub const fn tos(&self) -> &Tos {
        &self.tos
    }

    #[inline]
    pub fn tos_mut(&mut self) -> &mut Tos {
        &mut self.tos
    }

    #[inline]
    pub const fn total_len(&self) -> &U16 {
        &self.total_len
    }

    #[inline]
    pub fn total_len_mut(&mut self) -> &mut U16 {
        &mut self.total_len
    }

    #[inline]
    pub const fn id(&self) -> &U16 {
        &self.id
    }

    #[inline]
    pub fn id_mut(&mut self) -> &mut U16 {
        &mut self.id
    }

    #[inline]
    pub const fn flag_fragment(&self) -> &FlagFragment {
        &self.flag_fragment
    }

    #[inline]
    pub fn flag_fragment_mut(&mut self) -> &mut FlagFragment {
        &mut self.flag_fragment
    }

    #[inline]
    pub const fn ttl(&self) -> &u8 {
        &self.ttl
    }

    #[inline]
    pub fn ttl_mut(&mut self) -> &mut u8 {
        &mut self.ttl
    }

    #[inline]
    pub const fn protocol(&self) -> &ip::Protocol {
        &self.protocol
    }

    #[inline]
    pub fn protocol_mut(&mut self) -> &mut ip::Protocol {
        &mut self.protocol
    }

    #[inline]
    pub const fn checksum(&self) -> &U16 {
        &self.checksum
    }

    #[inline]
    pub fn checksum_mut(&mut self) -> &mut U16 {
        &mut self.checksum
    }

    #[inline]
    pub const fn source(&self) -> &IpV4Address {
        &self.source
    }

    #[inline]
    pub fn source_mut(&mut self) -> &mut IpV4Address {
        &mut self.source
    }

    #[inline]
    pub const fn destination(&self) -> &IpV4Address {
        &self.destination
    }

    #[inline]
    pub fn destination_mut(&mut self) -> &mut IpV4Address {
        &mut self.destination
    }

    #[inline]
    pub fn update_checksum(&mut self) {
        use core::hash::Hasher;

        self.checksum.set(0);

        let bytes = self.as_bytes();

        let mut checksum = crate::inet::checksum::Checksum::generic();

        checksum.write(bytes);

        self.checksum.set_be(checksum.finish_be());
    }
}

// This struct covers the bits for version and IHL (header len).
//
// Rust doesn't have the ability to do arbitrary bit sized values so we have to round up to the
// nearest byte.
define_inet_type!(
    pub struct Vihl {
        value: u8,
    }
);

impl fmt::Debug for Vihl {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Vihl")
            .field("version", &self.version())
            .field("header_len", &self.header_len())
            .finish()
    }
}

impl Vihl {
    #[inline]
    pub fn version(&self) -> u8 {
        self.value >> 4
    }

    #[inline]
    pub fn set_version(&mut self, value: u8) -> &mut Self {
        self.value = value << 4 | (self.value & 0x0F);
        self
    }

    #[inline]
    pub fn header_len(&self) -> u8 {
        self.value & 0x0F
    }

    #[inline]
    pub fn set_header_len(&mut self, value: u8) -> &mut Self {
        self.value = (self.value & 0xF0) | (value & 0x0F);
        self
    }
}

// This struct covers the bits for DSCP and ECN.
//
// Rust doesn't have the ability to do arbitrary bit sized values so we have to round up to the
// nearest byte.
define_inet_type!(
    pub struct Tos {
        value: u8,
    }
);

impl Tos {
    /// Differentiated Services Code Point
    #[inline]
    pub fn dscp(&self) -> u8 {
        self.value >> 2
    }

    #[inline]
    pub fn set_dscp(&mut self, value: u8) -> &mut Self {
        self.value = (value << 2) | (self.value & 0b11);
        self
    }

    #[inline]
    pub fn ecn(&self) -> ExplicitCongestionNotification {
        ExplicitCongestionNotification::new(self.value & 0b11)
    }

    #[inline]
    pub fn set_ecn(&mut self, ecn: ExplicitCongestionNotification) -> &mut Self {
        self.value = (self.value & !0b11) | ecn as u8;
        self
    }
}

impl fmt::Debug for Tos {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("ipv4::Tos")
            .field("dscp", &self.dscp())
            .field("ecn", &self.ecn())
            .finish()
    }
}

// This struct covers the bits for Flags and Fragment Offset.
//
// Rust doesn't have the ability to do arbitrary bit sized values so we have to round up to the
// nearest byte.
define_inet_type!(
    pub struct FlagFragment {
        value: U16,
    }
);

impl FlagFragment {
    const FRAGMENT_MASK: u16 = 0b0001_1111_1111_1111;

    #[inline]
    pub fn reserved(&self) -> bool {
        self.get(1 << 15)
    }

    pub fn set_reserved(&mut self, enabled: bool) -> &mut Self {
        self.set(1 << 15, enabled)
    }

    #[inline]
    pub fn dont_fragment(&self) -> bool {
        self.get(1 << 14)
    }

    #[inline]
    pub fn set_dont_fragment(&mut self, enabled: bool) -> &mut Self {
        self.set(1 << 14, enabled)
    }

    #[inline]
    pub fn more_fragments(&self) -> bool {
        self.get(1 << 13)
    }

    #[inline]
    pub fn set_more_fragments(&mut self, enabled: bool) -> &mut Self {
        self.set(1 << 13, enabled)
    }

    #[inline]
    pub fn fragment_offset(&self) -> u16 {
        self.value.get() & Self::FRAGMENT_MASK
    }

    #[inline]
    pub fn set_fragment_offset(&mut self, offset: u16) -> &mut Self {
        self.value
            .set(self.value.get() & !Self::FRAGMENT_MASK | offset & Self::FRAGMENT_MASK);
        self
    }

    #[inline]
    fn get(&self, mask: u16) -> bool {
        self.value.get() & mask == mask
    }

    #[inline]
    fn set(&mut self, mask: u16, enabled: bool) -> &mut Self {
        let value = self.value.get();
        let value = if enabled { value | mask } else { value & !mask };
        self.value.set(value);
        self
    }
}

impl fmt::Debug for FlagFragment {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("ipv4::FlagFragment")
            .field("reserved", &self.reserved())
            .field("dont_fragment", &self.dont_fragment())
            .field("more_fragments", &self.more_fragments())
            .field("fragment_offset", &self.fragment_offset())
            .finish()
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
    use s2n_codec::{DecoderBuffer, DecoderBufferMut};

    /// Asserts the Scope returned matches a known implementation
    #[test]
    #[cfg_attr(kani, kani::proof, kani::unwind(5), kani::solver(kissat))]
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

    #[test]
    #[cfg_attr(miri, ignore)]
    fn snapshot_test() {
        let mut buffer = vec![0u8; core::mem::size_of::<Header>()];
        for (idx, byte) in buffer.iter_mut().enumerate() {
            *byte = idx as u8;
        }
        let decoder = DecoderBuffer::new(&buffer);
        let (header, _) = decoder.decode::<&Header>().unwrap();
        insta::assert_debug_snapshot!("snapshot_test", header);

        for byte in &mut buffer {
            *byte = 255;
        }
        let decoder = DecoderBuffer::new(&buffer);
        let (header, _) = decoder.decode::<&Header>().unwrap();
        insta::assert_debug_snapshot!("snapshot_filled_test", header);
    }

    #[test]
    #[cfg_attr(kani, kani::proof, kani::unwind(5), kani::solver(kissat))]
    fn header_getter_setter_test() {
        check!().with_type::<Header>().for_each(|expected| {
            let mut buffer = [255u8; core::mem::size_of::<Header>()];
            let decoder = DecoderBufferMut::new(&mut buffer);
            let (header, _) = decoder.decode::<&mut Header>().unwrap();
            {
                // use all of the getters and setters to copy over each field
                header
                    .vihl_mut()
                    .set_version(expected.vihl().version())
                    .set_header_len(expected.vihl().header_len());
                header
                    .tos_mut()
                    .set_dscp(expected.tos().dscp())
                    .set_ecn(expected.tos().ecn());
                header.id_mut().set(expected.id().get());
                header.total_len_mut().set(expected.total_len().get());
                header
                    .flag_fragment_mut()
                    .set_reserved(expected.flag_fragment().reserved())
                    .set_dont_fragment(expected.flag_fragment().dont_fragment())
                    .set_more_fragments(expected.flag_fragment().more_fragments())
                    .set_fragment_offset(expected.flag_fragment().fragment_offset());
                *header.ttl_mut() = *expected.ttl();
                *header.protocol_mut() = *expected.protocol();
                header.checksum_mut().set(expected.checksum().get());
                *header.source_mut() = *expected.source();
                *header.destination_mut() = *expected.destination();
            }

            let decoder = DecoderBuffer::new(&buffer);
            let (actual, _) = decoder.decode::<&Header>().unwrap();
            {
                // make sure all of the values match
                assert_eq!(expected, actual);
                assert_eq!(expected.vihl(), actual.vihl());
                assert_eq!(expected.vihl().version(), actual.vihl().version());
                assert_eq!(expected.vihl().header_len(), actual.vihl().header_len());
                assert_eq!(expected.tos(), actual.tos());
                assert_eq!(expected.tos().dscp(), actual.tos().dscp());
                assert_eq!(expected.tos().ecn(), actual.tos().ecn());
                assert_eq!(expected.id(), actual.id());
                assert_eq!(expected.total_len(), actual.total_len());
                assert_eq!(expected.flag_fragment(), actual.flag_fragment());
                assert_eq!(
                    expected.flag_fragment().reserved(),
                    actual.flag_fragment().reserved()
                );
                assert_eq!(
                    expected.flag_fragment().dont_fragment(),
                    actual.flag_fragment().dont_fragment()
                );
                assert_eq!(
                    expected.flag_fragment().more_fragments(),
                    actual.flag_fragment().more_fragments()
                );
                assert_eq!(expected.ttl(), actual.ttl());
                assert_eq!(expected.protocol(), actual.protocol());
                assert_eq!(expected.checksum(), actual.checksum());
                assert_eq!(expected.source(), actual.source());
                assert_eq!(expected.destination(), actual.destination());
            }
        })
    }

    #[test]
    fn header_round_trip_test() {
        check!().for_each(|buffer| {
            s2n_codec::assert_codec_round_trip_bytes!(Header, buffer);
        });
    }
}

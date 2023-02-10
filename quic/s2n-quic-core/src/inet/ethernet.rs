// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::inet::Unspecified;
use core::fmt;

const MAC_LEN: usize = 48 / 8;

define_inet_type!(
    pub struct MacAddress {
        octets: [u8; MAC_LEN],
    }
);

impl fmt::Debug for MacAddress {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_tuple("MacAddress")
            .field(&format_args!("{self}"))
            .finish()
    }
}

impl fmt::Display for MacAddress {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        let [a, b, c, d, e, f] = self.octets;
        write!(fmt, "{a:02x}:{b:02x}:{c:02x}:{d:02x}:{e:02x}:{f:02x}")
    }
}

impl Unspecified for MacAddress {
    #[inline]
    fn is_unspecified(&self) -> bool {
        self.octets == [0; MAC_LEN]
    }
}

define_inet_type!(
    pub struct EtherType {
        id: [u8; 2],
    }
);

impl EtherType {
    // https://www.iana.org/assignments/ieee-802-numbers/ieee-802-numbers.xhtml#ieee-802-numbers-1
    // NOTE: these variants were added as the ones we think we'll need. feel free to add more as
    //       needed.
    pub const IPV4: Self = Self { id: [0x08, 0x00] };
    pub const ARP: Self = Self { id: [0x08, 0x06] };
    pub const IPV6: Self = Self { id: [0x86, 0xDD] };
    pub const VLAN: Self = Self { id: [0x81, 0x00] };
    pub const PPP: Self = Self { id: [0x88, 0xA8] };
}

impl fmt::Debug for EtherType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_tuple("EtherType")
            .field(&format_args!("{self}"))
            .finish()
    }
}

impl fmt::Display for EtherType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            v if v == Self::IPV4 => "IPv4",
            v if v == Self::ARP => "ARP",
            v if v == Self::IPV6 => "IPv6",
            v if v == Self::VLAN => "VLAN",
            v if v == Self::PPP => "PPP",
            Self { id: [a, b] } => return write!(f, "[unknown 0x{a:02x}{b:02x}]"),
        }
        .fmt(f)
    }
}

macro_rules! impl_is {
    ($fun:ident, $cap:ident) => {
        #[inline]
        pub const fn $fun(self) -> bool {
            Self::const_cmp(self, Self::$cap)
        }
    };
}

impl EtherType {
    impl_is!(is_ipv4, IPV4);
    impl_is!(is_arp, ARP);
    impl_is!(is_ipv6, IPV6);
    impl_is!(is_vlan, VLAN);
    impl_is!(is_ppp, PPP);

    #[inline]
    const fn const_cmp(a: Self, b: Self) -> bool {
        a.id[0] == b.id[0] && a.id[1] == b.id[1]
    }
}

// NOTE: duvet doesn't know how to parse this RFC since it doesn't follow more modern formatting
//# https://www.rfc-editor.org/rfc/rfc826
//# Packet format:
//# --------------
//#
//# To communicate mappings from <protocol, address> pairs to 48.bit
//# Ethernet addresses, a packet format that embodies the Address
//# Resolution protocol is needed.  The format of the packet follows.
//#
//#    Ethernet transmission layer (not necessarily accessible to
//#         the user):
//#        48.bit: Ethernet address of destination
//#        48.bit: Ethernet address of sender
//#        16.bit: Protocol type = ether_type$ADDRESS_RESOLUTION

define_inet_type!(
    pub struct Header {
        destination: MacAddress,
        source: MacAddress,
        ethertype: EtherType,
    }
);

impl fmt::Debug for Header {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("ethernet::Header")
            .field("destination", &self.destination)
            .field("source", &self.source)
            .field("ethertype", &self.ethertype)
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
    pub const fn destination(&self) -> &MacAddress {
        &self.destination
    }

    #[inline]
    pub fn destination_mut(&mut self) -> &mut MacAddress {
        &mut self.destination
    }

    #[inline]
    pub const fn source(&self) -> &MacAddress {
        &self.source
    }

    #[inline]
    pub fn source_mut(&mut self) -> &mut MacAddress {
        &mut self.source
    }

    #[inline]
    pub const fn ethertype(&self) -> &EtherType {
        &self.ethertype
    }

    #[inline]
    pub fn ethertype_mut(&mut self) -> &mut EtherType {
        &mut self.ethertype
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bolero::check;
    use s2n_codec::DecoderBuffer;

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
    fn header_round_trip_test() {
        check!().for_each(|buffer| {
            s2n_codec::assert_codec_round_trip_bytes!(Header, buffer);
        });
    }
}

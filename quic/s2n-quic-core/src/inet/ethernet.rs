// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::inet::Unspecified;
use core::fmt;

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

impl MacAddress {
    pub const UNSPECIFIED: Self = Self {
        octets: [0; MAC_LEN],
    };
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
            Self::IPV4 => "IPv4",
            Self::ARP => "ARP",
            Self::IPV6 => "IPv6",
            Self::VLAN => "VLAN",
            Self::PPP => "PPP",
            Self { id: [a, b] } => return write!(f, "[unknown 0x{a:02x}{b:02x}]"),
        }
        .fmt(f)
    }
}

macro_rules! impl_type {
    ($fun:ident, $cap:ident, $val:expr) => {
        pub const $cap: Self = Self { id: $val };

        #[inline]
        pub const fn $fun(self) -> bool {
            matches!(self, Self::$cap)
        }
    };
}

impl EtherType {
    // https://www.iana.org/assignments/ieee-802-numbers/ieee-802-numbers.xhtml#ieee-802-numbers-1
    // NOTE: these variants were added as the ones we think we'll need. feel free to add more as
    //       needed.
    impl_type!(is_ipv4, IPV4, [0x08, 0x00]);
    impl_type!(is_arp, ARP, [0x08, 0x06]);
    impl_type!(is_ipv6, IPV6, [0x86, 0xDD]);
    impl_type!(is_ppp, PPP, [0x88, 0x0B]);
    impl_type!(is_vlan, VLAN, [0x88, 0xA8]);
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

        buffer.fill(255);
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

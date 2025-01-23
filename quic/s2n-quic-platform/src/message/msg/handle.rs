// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::ext::Ext as _;
use crate::{features, message::cmsg::Encoder};
use libc::msghdr;
use s2n_quic_core::{
    inet::{AncillaryData, SocketAddressV4},
    path::{self, LocalAddress, RemoteAddress},
};

#[cfg(any(test, feature = "generator"))]
use bolero_generator::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(any(test, feature = "generator"), derive(TypeGenerator))]
pub struct Handle {
    pub remote_address: RemoteAddress,
    pub local_address: LocalAddress,
}

impl Handle {
    #[inline]
    pub(super) fn with_ancillary_data(&mut self, ancillary_data: AncillaryData) {
        self.local_address = ancillary_data.local_address;
    }

    #[inline]
    pub(super) fn update_msg_hdr(&self, msghdr: &mut msghdr) {
        // when sending a packet, we start out with no cmsg items
        msghdr.msg_controllen = 0;

        msghdr.set_remote_address(&self.remote_address.0);

        msghdr
            .cmsg_encoder()
            .encode_local_address(&self.local_address.0)
            .unwrap();
    }
}

impl path::Handle for Handle {
    #[inline]
    fn from_remote_address(remote_address: RemoteAddress) -> Self {
        Self {
            remote_address,
            local_address: SocketAddressV4::UNSPECIFIED.into(),
        }
    }

    #[inline]
    fn remote_address(&self) -> RemoteAddress {
        self.remote_address
    }

    #[inline]
    fn set_remote_address(&mut self, addr: RemoteAddress) {
        self.remote_address = addr;
    }

    #[inline]
    fn local_address(&self) -> LocalAddress {
        self.local_address
    }

    #[inline]
    fn set_local_address(&mut self, addr: LocalAddress) {
        self.local_address = addr;
    }

    #[inline]
    fn unmapped_eq(&self, other: &Self) -> bool {
        let mut eq = true;

        // only compare local addresses if the OS returns them
        if features::pktinfo::IS_SUPPORTED {
            eq &= self.local_address.unmapped_eq(&other.local_address);
        }

        eq && self.remote_address.unmapped_eq(&other.remote_address)
    }

    #[inline]
    fn strict_eq(&self, other: &Self) -> bool {
        PartialEq::eq(self, other)
    }

    #[inline]
    fn maybe_update(&mut self, other: &Self) {
        if other.local_address.port() == 0 {
            return;
        }

        // once we discover our path, or the port changes, update the address with the new information
        if self.local_address.port() != other.local_address.port() {
            self.local_address = other.local_address;
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::message::msg::Handle;
    use s2n_quic_core::{
        inet::{IpV4Address, IpV6Address, SocketAddressV4, SocketAddressV6},
        path::{Handle as _, LocalAddress},
    };

    #[test]
    //= https://www.rfc-editor.org/rfc/rfc5156#section-2.2
    //= type=test
    //# ::FFFF:0:0/96 are the IPv4-mapped addresses [RFC4291].
    fn to_ipv6_mapped_test() {
        let handle_ipv6 = Handle {
            remote_address: Default::default(),
            local_address: LocalAddress::from(SocketAddressV6::new(
                IpV6Address::new([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xff, 0xff, 1, 1, 1, 1]),
                4440,
            )),
        };
        let handle_ipv4 = Handle {
            remote_address: Default::default(),
            local_address: LocalAddress::from(SocketAddressV4::new(
                IpV4Address::new([1, 1, 1, 1]),
                4440,
            )),
        };

        assert!(handle_ipv6.unmapped_eq(&handle_ipv4));
    }
}

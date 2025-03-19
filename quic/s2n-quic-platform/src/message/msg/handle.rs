// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::ext::Ext as _;
use crate::{features, message::cmsg::Encoder};
use libc::msghdr;
use s2n_quic_core::{
    ensure,
    inet::{AncillaryData, SocketAddressV4, Unspecified},
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
        ensure!(
            self.remote_address.unmapped_eq(&other.remote_address),
            false
        );

        // only compare local addresses if the OS returns them
        ensure!(features::pktinfo::IS_SUPPORTED, true);

        // Make sure to only compare the fields if they're both set
        //
        // This avoids cases where we don't have the full context for the local address and find it
        // out with a later packet.
        if !self.local_address.ip().is_unspecified() && !other.local_address.ip().is_unspecified() {
            ensure!(
                self.local_address
                    .ip()
                    .unmapped_eq(&other.local_address.ip()),
                false
            );
        }

        if self.local_address.port() > 0 && other.local_address.port() > 0 {
            ensure!(
                self.local_address.port() == other.local_address.port(),
                false
            );
        }

        true
    }

    #[inline]
    fn strict_eq(&self, other: &Self) -> bool {
        PartialEq::eq(self, other)
    }

    #[inline]
    fn maybe_update(&mut self, other: &Self) {
        self.local_address.maybe_update(&other.local_address);
    }
}

#[cfg(test)]
mod tests {
    use crate::message::msg::Handle;
    use s2n_quic_core::{
        inet::{IpAddress, IpV4Address},
        path::{Handle as _, LocalAddress},
    };

    /// Checks that unmapped_eq is correct independent of argument ordering
    fn reflexive_check(a: Handle, b: Handle) {
        assert!(a.unmapped_eq(&b));
        assert!(b.unmapped_eq(&a));
    }

    #[test]
    fn unmapped_eq_test() {
        // All of these values should be considered equivalent for local addresses
        let ips: &[IpAddress] = &[
            // if we have an unspecified IP address then don't consider it for equality
            IpV4Address::new([0, 0, 0, 0]).into(),
            // a regular IPv4 IP should match the IPv4-mapped into IPv6
            IpV4Address::new([1, 1, 1, 1]).into(),
            IpV4Address::new([1, 1, 1, 1]).to_ipv6_mapped().into(),
        ];
        let ports = [0u16, 4440];

        for ip_a in ips {
            for ip_b in ips {
                for port_a in ports {
                    for port_b in ports {
                        reflexive_check(
                            Handle {
                                remote_address: Default::default(),
                                local_address: LocalAddress::from(ip_a.with_port(port_a)),
                            },
                            Handle {
                                remote_address: Default::default(),
                                local_address: LocalAddress::from(ip_b.with_port(port_b)),
                            },
                        );
                    }
                }
            }
        }
    }
}

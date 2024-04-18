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
    fn set_remote_port(&mut self, port: u16) {
        self.remote_address.0.set_port(port);
    }

    #[inline]
    fn local_address(&self) -> LocalAddress {
        self.local_address
    }

    #[inline]
    fn eq(&self, other: &Self) -> bool {
        let mut eq = true;

        // only compare local addresses if the OS returns them
        if features::pktinfo::IS_SUPPORTED {
            eq &= self.local_address.eq(&other.local_address);
        }

        eq && path::Handle::eq(&self.remote_address, &other.remote_address)
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

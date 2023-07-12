// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    inet::{ethernet::MacAddress, ipv4, IpAddress},
    path::{self, Handle},
};

#[cfg(any(test, feature = "generator"))]
use bolero_generator::prelude::*;

macro_rules! define_address {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        #[cfg_attr(any(test, feature = "generator"), derive(TypeGenerator))]
        pub struct $name {
            pub mac: MacAddress,
            pub ip: IpAddress,
            pub port: u16,
        }

        impl $name {
            pub const UNSPECIFIED: Self = Self {
                mac: MacAddress::UNSPECIFIED,
                ip: IpAddress::Ipv4(ipv4::IpV4Address::UNSPECIFIED),
                port: 0,
            };

            #[inline]
            pub fn unmap(self) -> Self {
                Self {
                    mac: self.mac,
                    ip: self.ip.unmap(),
                    port: self.port,
                }
            }
        }

        impl From<path::$name> for $name {
            #[inline]
            fn from(addr: path::$name) -> Self {
                Self {
                    mac: MacAddress::UNSPECIFIED,
                    ip: addr.ip(),
                    port: addr.port(),
                }
            }
        }

        impl From<$name> for path::$name {
            #[inline]
            fn from(addr: $name) -> Self {
                addr.ip.with_port(addr.port).into()
            }
        }
    };
}

define_address!(RemoteAddress);
define_address!(LocalAddress);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(any(test, feature = "generator"), derive(TypeGenerator))]
pub struct Tuple {
    pub remote_address: RemoteAddress,
    pub local_address: LocalAddress,
}

impl Tuple {
    pub const UNSPECIFIED: Self = Self {
        remote_address: RemoteAddress::UNSPECIFIED,
        local_address: LocalAddress::UNSPECIFIED,
    };

    #[inline]
    pub fn swap(&mut self) {
        core::mem::swap(&mut self.remote_address.mac, &mut self.local_address.mac);
        core::mem::swap(&mut self.remote_address.ip, &mut self.local_address.ip);
        core::mem::swap(&mut self.remote_address.port, &mut self.local_address.port);
    }
}

impl Handle for Tuple {
    #[inline]
    fn from_remote_address(remote_address: path::RemoteAddress) -> Self {
        let remote_address = remote_address.into();
        let local_address = LocalAddress::UNSPECIFIED;
        Self {
            remote_address,
            local_address,
        }
    }

    #[inline]
    fn remote_address(&self) -> path::RemoteAddress {
        self.remote_address.into()
    }

    #[inline]
    fn local_address(&self) -> path::LocalAddress {
        self.local_address.into()
    }

    #[inline]
    fn eq(&self, other: &Self) -> bool {
        // TODO only compare everything if the other is all filled out
        PartialEq::eq(&self.local_address.unmap(), &other.local_address.unmap())
            && PartialEq::eq(&self.remote_address.unmap(), &other.remote_address.unmap())
    }

    #[inline]
    fn strict_eq(&self, other: &Self) -> bool {
        PartialEq::eq(self, other)
    }

    #[inline]
    fn maybe_update(&mut self, other: &Self) {
        // once we discover our path, update the address full address
        if self.local_address.port == 0 {
            *self = *other;
        }
    }

    #[inline]
    fn update_local_address(&mut self, other: &Self) {
        self.local_address = other.local_address;
    }
}

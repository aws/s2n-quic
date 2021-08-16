// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::inet::{SocketAddress, SocketAddressV4, SocketAddressV6};
use core::{
    convert::{TryFrom, TryInto},
    fmt,
    fmt::{Display, Formatter},
    num::NonZeroU16,
};

//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#14
//# The maximum datagram size MUST be at least 1200 bytes.
pub const MINIMUM_MTU: u16 = 1200;

// TODO decide on better defaults
// Safety: 1500 is greater than zero
pub const DEFAULT_MAX_MTU: MaxMtu = MaxMtu(unsafe { NonZeroU16::new_unchecked(1500) });

// Length is the length in octets of this user datagram  including  this
// header and the data. (This means the minimum value of the length is
// eight.)
// See https://tools.ietf.org/rfc/rfc768.txt
pub const UDP_HEADER_LEN: u16 = 8;

// IPv4 header ranges from 20-60 bytes, depending on Options
pub const IPV4_MIN_HEADER_LEN: u16 = 20;
// IPv6 header is always 40 bytes, plus extensions
pub const IPV6_MIN_HEADER_LEN: u16 = 40;
#[cfg(feature = "ipv6")]
const IP_MIN_HEADER_LEN: u16 = IPV6_MIN_HEADER_LEN;
#[cfg(not(feature = "ipv6"))]
const IP_MIN_HEADER_LEN: u16 = IPV4_MIN_HEADER_LEN;

// The minimum allowed Max MTU is the minimum UDP datagram size of 1200 bytes plus
// the UDP header length and minimal IP header length
const MIN_ALLOWED_MAX_MTU: u16 = MINIMUM_MTU + UDP_HEADER_LEN + IP_MIN_HEADER_LEN;

// Initial PTO backoff multiplier is 1 indicating no additional increase to the backoff.
pub const INITIAL_PTO_BACKOFF: u32 = 1;

/// An interface for an object that represents a unique path between two endpoints
pub trait Handle: 'static + Copy + Send + fmt::Debug {
    /// Returns the remote address for the given handle
    fn remote_address(&self) -> RemoteAddress;

    /// Returns the local address for the given handle
    fn local_address(&self) -> LocalAddress;

    /// Returns `true` if the two handles are equal from a network perspective
    ///
    /// This function is used to determine if a connection has migrated to another
    /// path.
    fn eq(&self, other: &Self) -> bool;

    /// Returns `true` if the two handles are strictly equal to each other, i.e.
    /// byte-for-byte.
    fn strict_eq(&self, other: &Self) -> bool;
}

macro_rules! impl_addr {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, Default, Eq)]
        pub struct $name(pub SocketAddress);

        impl From<SocketAddress> for $name {
            #[inline]
            fn from(value: SocketAddress) -> Self {
                Self(value)
            }
        }

        impl From<SocketAddressV4> for $name {
            #[inline]
            fn from(value: SocketAddressV4) -> Self {
                Self(value.into())
            }
        }

        impl From<SocketAddressV6> for $name {
            #[inline]
            fn from(value: SocketAddressV6) -> Self {
                Self(value.into())
            }
        }

        impl PartialEq for $name {
            #[inline]
            fn eq(&self, other: &Self) -> bool {
                self.0.eq(&other.0)
            }
        }

        impl core::ops::Deref for $name {
            type Target = SocketAddress;

            fn deref(&self) -> &Self::Target {
                &self.0
            }
        }
    };
}

impl_addr!(LocalAddress);

impl_addr!(RemoteAddress);

impl RemoteAddress {
    #[inline]
    pub fn from_remote_address(remote_address: SocketAddress) -> Self {
        Self(remote_address)
    }
}

impl Handle for RemoteAddress {
    #[inline]
    fn remote_address(&self) -> RemoteAddress {
        *self
    }

    #[inline]
    fn local_address(&self) -> LocalAddress {
        SocketAddressV4::UNSPECIFIED.into()
    }

    #[inline]
    fn eq(&self, other: &Self) -> bool {
        PartialEq::eq(self, other)
    }

    #[inline]
    fn strict_eq(&self, other: &Self) -> bool {
        PartialEq::eq(self, other)
    }
}

#[derive(Clone, Copy, Debug, Eq)]
pub struct Tuple {
    pub remote_address: RemoteAddress,
    pub local_address: LocalAddress,
}

impl Tuple {
    #[inline]
    pub fn from_remote_address(remote_address: RemoteAddress) -> Self {
        let local_address = SocketAddressV4::UNSPECIFIED.into();
        Self {
            remote_address,
            local_address,
        }
    }
}

impl PartialEq for Tuple {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        PartialEq::eq(&self.remote_address, &other.remote_address)
            && PartialEq::eq(&self.local_address, &other.local_address)
    }
}

impl Handle for Tuple {
    #[inline]
    fn remote_address(&self) -> RemoteAddress {
        self.remote_address
    }

    #[inline]
    fn local_address(&self) -> LocalAddress {
        self.local_address
    }

    #[inline]
    fn eq(&self, other: &Self) -> bool {
        PartialEq::eq(self, other)
    }

    #[inline]
    fn strict_eq(&self, other: &Self) -> bool {
        PartialEq::eq(self, other)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct MaxMtu(NonZeroU16);

impl Default for MaxMtu {
    fn default() -> Self {
        DEFAULT_MAX_MTU
    }
}

impl TryFrom<u16> for MaxMtu {
    type Error = MaxMtuError;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        if value < MIN_ALLOWED_MAX_MTU {
            return Err(MaxMtuError(MIN_ALLOWED_MAX_MTU.try_into().unwrap()));
        }

        Ok(MaxMtu(value.try_into().expect(
            "Value must be greater than zero according to the check above",
        )))
    }
}

impl From<MaxMtu> for usize {
    #[inline]
    fn from(value: MaxMtu) -> Self {
        value.0.get() as usize
    }
}

impl From<MaxMtu> for u16 {
    #[inline]
    fn from(value: MaxMtu) -> Self {
        value.0.get()
    }
}

#[derive(Debug)]
pub struct MaxMtuError(NonZeroU16);

impl Display for MaxMtuError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "MaxMtu must be at least {}", self.0)
    }
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    event,
    inet::{IpV4Address, IpV6Address, SocketAddress, SocketAddressV4, SocketAddressV6},
};
use core::fmt;

#[cfg(any(test, feature = "generator"))]
use bolero_generator::*;

pub mod ecn;
pub mod migration;
pub mod mtu;

pub use mtu::*;

// Initial PTO backoff multiplier is 1 indicating no additional increase to the backoff.
pub const INITIAL_PTO_BACKOFF: u32 = 1;

/// Internal Id of a path in the manager
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Eq, Ord, Hash)]
pub struct Id(u8);

impl Id {
    /// Create a new path::Id
    ///
    /// # Safety
    /// This should only be used by the path::Manager
    pub unsafe fn new(id: u8) -> Self {
        Self(id)
    }

    pub fn as_u8(&self) -> u8 {
        self.0
    }
}

impl event::IntoEvent<u64> for Id {
    #[inline]
    fn into_event(self) -> u64 {
        self.0 as u64
    }
}

#[cfg(any(test, feature = "testing"))]
impl Id {
    pub fn test_id() -> Self {
        unsafe { Id::new(0) }
    }
}

/// An interface for an object that represents a unique path between two endpoints
pub trait Handle: 'static + Copy + Send + fmt::Debug {
    /// Creates a Handle from a RemoteAddress
    fn from_remote_address(remote_addr: RemoteAddress) -> Self;

    /// Returns the remote address for the given handle
    fn remote_address(&self) -> RemoteAddress;

    /// Updates the remote port to the given value
    fn set_remote_port(&mut self, port: u16);

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

    /// Depending on the current value of `self`, fields from `other` may be copied to increase the
    /// fidelity of the value.
    ///
    /// This is especially useful for clients that initiate a connection only based on the remote
    /// IP and port. They likely wouldn't know the IP address of the local socket. Once a response
    /// is received from the server, the IP information will be known at this point and the handle
    /// can be updated with the new information.
    ///
    /// Implementations should try to limit the cost of updating by checking the current value to
    /// see if it needs updating.
    fn maybe_update(&mut self, other: &Self);
}

macro_rules! impl_addr {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
        #[cfg_attr(any(test, feature = "generator"), derive(TypeGenerator))]
        #[cfg_attr(kani, derive(kani::Arbitrary))]
        pub struct $name(pub SocketAddress);

        impl From<event::api::SocketAddress<'_>> for $name {
            #[inline]
            fn from(value: event::api::SocketAddress<'_>) -> Self {
                match value {
                    event::api::SocketAddress::IpV4 { ip, port } => {
                        $name(IpV4Address::new(*ip).with_port(port).into())
                    }
                    event::api::SocketAddress::IpV6 { ip, port } => {
                        $name(IpV6Address::new(*ip).with_port(port).into())
                    }
                }
            }
        }

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

        impl core::ops::Deref for $name {
            type Target = SocketAddress;

            fn deref(&self) -> &Self::Target {
                &self.0
            }
        }

        impl core::ops::DerefMut for $name {
            fn deref_mut(&mut self) -> &mut Self::Target {
                &mut self.0
            }
        }
    };
}

impl_addr!(LocalAddress);

impl_addr!(RemoteAddress);

impl Handle for RemoteAddress {
    #[inline]
    fn from_remote_address(remote_address: RemoteAddress) -> Self {
        remote_address
    }

    #[inline]
    fn remote_address(&self) -> RemoteAddress {
        *self
    }

    #[inline]
    fn set_remote_port(&mut self, port: u16) {
        self.0.set_port(port)
    }

    #[inline]
    fn local_address(&self) -> LocalAddress {
        SocketAddressV4::UNSPECIFIED.into()
    }

    #[inline]
    fn eq(&self, other: &Self) -> bool {
        PartialEq::eq(&self.unmap(), &other.unmap())
    }

    #[inline]
    fn strict_eq(&self, other: &Self) -> bool {
        PartialEq::eq(self, other)
    }

    #[inline]
    fn maybe_update(&mut self, _other: &Self) {
        // nothing to update
    }
}

#[derive(Clone, Copy, Debug, Eq)]
#[cfg_attr(any(test, feature = "generator"), derive(TypeGenerator))]
pub struct Tuple {
    pub remote_address: RemoteAddress,
    pub local_address: LocalAddress,
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
    fn from_remote_address(remote_address: RemoteAddress) -> Self {
        let local_address = SocketAddressV4::UNSPECIFIED.into();
        Self {
            remote_address,
            local_address,
        }
    }

    #[inline]
    fn remote_address(&self) -> RemoteAddress {
        self.remote_address
    }

    #[inline]
    fn set_remote_port(&mut self, port: u16) {
        self.remote_address.set_port(port)
    }

    #[inline]
    fn local_address(&self) -> LocalAddress {
        self.local_address
    }

    #[inline]
    fn eq(&self, other: &Self) -> bool {
        PartialEq::eq(&self.local_address.unmap(), &other.local_address.unmap())
            && Handle::eq(&self.remote_address, &other.remote_address)
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

//= https://www.rfc-editor.org/rfc/rfc9308#section-8.1
//# Some UDP protocols are vulnerable to reflection attacks, where an
//# attacker is able to direct traffic to a third party as a denial of
//# service.  For example, these source ports are associated with
//# applications known to be vulnerable to reflection attacks, often due
//# to server misconfiguration:
//#
//# *  port 53 - DNS [RFC1034]
//#
//# *  port 123 - NTP [RFC5905]
//#
//# *  port 1900 - SSDP [SSDP]
//#
//# *  port 5353 - mDNS [RFC6762]
//#
//# *  port 11211 - memcache

/// List of ports to refuse connections from. This list must be sorted.
///
/// Based on https://quiche.googlesource.com/quiche/+/bac04054bccb2a249d4705ecc94a646404d41c1b/quiche/quic/core/quic_dispatcher.cc#498
const BLOCKED_PORTS: [u16; 11] = [
    0,   // We cannot send to port 0 so drop that source port.
    17,  // Quote of the Day, can loop with QUIC.
    19,  // Chargen, can loop with QUIC.
    53,  // DNS, vulnerable to reflection attacks.
    111, // Portmap.
    123, // NTP, vulnerable to reflection attacks.
    137, // NETBIOS Name Service,
    138, // NETBIOS Datagram Service
    161, // SNMP.
    389, // CLDAP.
    500, // IKE, can loop with QUIC.
];
const MAX_BLOCKED_PORT: u16 = BLOCKED_PORTS[BLOCKED_PORTS.len() - 1];

#[inline]
pub fn remote_port_blocked(port: u16) -> bool {
    if port > MAX_BLOCKED_PORT {
        // Early return to avoid iteration if possible
        return false;
    }

    for blocked in BLOCKED_PORTS {
        if port == blocked {
            return true;
        }
    }

    false
}

// The below ports are also vulnerable to reflection attacks, but are within
// the original ephemeral port range of 1024–65535, so there is a chance
// clients may be randomly assigned them. To address this, we can throttle
// connections using these ports instead of fully blocking them.

/// List of ports to throttle connections from. This list must be sorted.
///
/// Based on https://quiche.googlesource.com/quiche/+/bac04054bccb2a249d4705ecc94a646404d41c1b/quiche/quic/core/quic_dispatcher.cc#498
const THROTTLED_PORTS: [u16; 5] = [
    1900,  // SSDP, vulnerable to reflection attacks.
    3702,  // WS-Discovery, vulnerable to reflection attacks.
    5353,  // mDNS, vulnerable to reflection attacks.
    5355,  // LLMNR, vulnerable to reflection attacks.
    11211, // memcache, vulnerable to reflection attacks.
];
const MAX_THROTTLED_PORT: u16 = THROTTLED_PORTS[THROTTLED_PORTS.len() - 1];
pub const THROTTLED_PORTS_LEN: usize = THROTTLED_PORTS.len();

#[inline]
pub fn remote_port_throttled_index(port: u16) -> Option<usize> {
    for (idx, throttled_port) in THROTTLED_PORTS.iter().enumerate() {
        if *throttled_port == port {
            return Some(idx);
        }
    }
    None
}

#[inline]
pub fn remote_port_throttled(port: u16) -> bool {
    if port > MAX_THROTTLED_PORT {
        // Early return to avoid iteration if possible
        return false;
    }

    for throttled in THROTTLED_PORTS {
        if port == throttled {
            return true;
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use crate::path::{
        remote_port_blocked, remote_port_throttled, BLOCKED_PORTS, MAX_BLOCKED_PORT,
        MAX_THROTTLED_PORT, THROTTLED_PORTS,
    };

    #[test]
    fn blocked_ports_is_sorted() {
        assert_eq!(Some(MAX_BLOCKED_PORT), BLOCKED_PORTS.iter().max().copied());

        let mut sorted = BLOCKED_PORTS.to_vec();
        sorted.sort_unstable();

        for i in 0..BLOCKED_PORTS.len() {
            assert_eq!(sorted[i], BLOCKED_PORTS[i]);
        }
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn blocked_port() {
        for port in 0..u16::MAX {
            let blocked_expected = BLOCKED_PORTS.iter().copied().any(|blocked| blocked == port);
            assert_eq!(blocked_expected, remote_port_blocked(port));
        }
    }

    #[test]
    fn throttled_ports_is_sorted() {
        assert_eq!(
            Some(MAX_THROTTLED_PORT),
            THROTTLED_PORTS.iter().max().copied()
        );

        let mut sorted = THROTTLED_PORTS.to_vec();
        sorted.sort_unstable();

        for i in 0..THROTTLED_PORTS.len() {
            assert_eq!(sorted[i], THROTTLED_PORTS[i]);
        }
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn throttled_port() {
        for port in 0..u16::MAX {
            let throttled_expected = THROTTLED_PORTS
                .iter()
                .copied()
                .any(|throttled| throttled == port);
            assert_eq!(throttled_expected, remote_port_throttled(port));
        }
    }
}

#[cfg(any(test, feature = "testing"))]
pub mod testing {
    use crate::{
        connection, event,
        event::{builder::SocketAddress, IntoEvent},
    };

    impl<'a> event::builder::Path<'a> {
        pub fn test() -> Self {
            Self {
                local_addr: SocketAddress::IpV4 {
                    ip: &[127, 0, 0, 1],
                    port: 0,
                },
                local_cid: connection::LocalId::TEST_ID.into_event(),
                remote_addr: SocketAddress::IpV4 {
                    ip: &[127, 0, 0, 1],
                    port: 0,
                },
                remote_cid: connection::PeerId::TEST_ID.into_event(),
                id: 0,
                is_active: false,
            }
        }
    }
}

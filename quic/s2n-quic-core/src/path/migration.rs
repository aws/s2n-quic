// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    event,
    event::{
        api::{Path, SocketAddress},
        IntoEvent,
    },
    inet,
};

#[derive(Debug)]
#[non_exhaustive]
pub struct Attempt<'a> {
    /// The path that the connection is currently actively using
    pub active_path: Path<'a>,
    /// Information about the packet triggering the migration attempt
    pub packet: PacketInfo<'a>,
}

#[derive(Debug)]
pub struct AttemptBuilder<'a> {
    /// The path that the connection is currently actively using
    pub active_path: Path<'a>,
    /// Information about the packet triggering the migration attempt
    pub packet: PacketInfo<'a>,
}

impl<'a> From<AttemptBuilder<'a>> for Attempt<'a> {
    #[inline]
    fn from(builder: AttemptBuilder<'a>) -> Self {
        Self {
            active_path: builder.active_path,
            packet: builder.packet,
        }
    }
}

#[derive(Debug)]
#[non_exhaustive]
pub struct PacketInfo<'a> {
    pub remote_address: SocketAddress<'a>,
    pub local_address: SocketAddress<'a>,
}

#[derive(Debug)]
pub struct PacketInfoBuilder<'a> {
    pub remote_address: &'a inet::SocketAddress,
    pub local_address: &'a inet::SocketAddress,
}

impl<'a> From<PacketInfoBuilder<'a>> for PacketInfo<'a> {
    #[inline]
    fn from(builder: PacketInfoBuilder<'a>) -> Self {
        Self {
            remote_address: builder.remote_address.into_event(),
            local_address: builder.local_address.into_event(),
        }
    }
}

// TODO: Add an outcome that allows the connection to be closed/stateless reset https://github.com/aws/s2n-quic/issues/317

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Outcome {
    /// Allows the path migration attempt to continue
    ///
    /// Note that path validation will still be attempted as described in
    /// [Section 8.2](https://datatracker.ietf.org/doc/html/rfc9000#section-8.2).
    Allow,

    /// Rejects a path migration attempt
    ///
    /// The connection will drop the packet that attempted to migrate and not reserve any state
    /// for the new path.
    Deny(DenyReason),
    // Additional outcomes must be handled in the path::Manager
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DenyReason {
    // The new address uses a port that is blocked
    BlockedPort,
    // The new address uses a port in a different scope
    PortScopeChanged,
    // The new address uses an IP in a different scope
    IpScopeChanged,
    // All connection migrations are disabled
    ConnectionMigrationDisabled,
}

impl IntoEvent<event::builder::ConnectionMigrationDenied> for DenyReason {
    #[inline]
    fn into_event(self) -> event::builder::ConnectionMigrationDenied {
        let reason = match self {
            DenyReason::BlockedPort => event::builder::MigrationDenyReason::BlockedPort,
            DenyReason::PortScopeChanged => event::builder::MigrationDenyReason::PortScopeChanged,
            DenyReason::IpScopeChanged => event::builder::MigrationDenyReason::IpScopeChange,
            DenyReason::ConnectionMigrationDisabled => {
                event::builder::MigrationDenyReason::ConnectionMigrationDisabled
            }
        };
        event::builder::ConnectionMigrationDenied { reason }
    }
}

/// Validates a path migration attempt from an active path to another
pub trait Validator: 'static + Send {
    /// Called on each connection migration attempt for a connection
    fn on_migration_attempt(&mut self, attempt: &Attempt) -> Outcome;
}

pub mod default {
    use super::*;
    use crate::path::remote_port_blocked;

    #[derive(Debug, Default)]
    pub struct Validator;

    impl super::Validator for Validator {
        #[inline]
        fn on_migration_attempt(&mut self, attempt: &Attempt) -> Outcome {
            let active_addr = to_addr(&attempt.active_path.remote_addr);
            let packet_addr = to_addr(&attempt.packet.remote_address);

            // Block migrations to a port that is blocked
            if remote_port_blocked(packet_addr.port()) {
                return Outcome::Deny(DenyReason::BlockedPort);
            }

            //= https://www.rfc-editor.org/rfc/rfc9000#section-21.5.6
            //# it might be possible over time to identify
            //# specific UDP ports that are common targets of attacks or particular
            //# patterns in datagrams that are used for attacks.  Endpoints MAY
            //# choose to avoid sending datagrams to these ports or not send
            //# datagrams that match these patterns prior to validating the
            //# destination address.

            // NOTE: this may cause reachability issues if a peer or NAT use different
            //       port scopes for the same connection. Additional research may
            //       be required to determine if this countermeasure needs to be relaxed.
            if PortScope::new(active_addr.port()) != PortScope::new(packet_addr.port()) {
                return Outcome::Deny(DenyReason::PortScopeChanged);
            }

            //= https://www.rfc-editor.org/rfc/rfc9000#section-21.5.6
            //# Endpoints MAY prevent connection attempts or
            //# migration to a loopback address.  Endpoints SHOULD NOT allow
            //# connections or migration to a loopback address if the same service
            //# was previously available at a different interface or if the address
            //# was provided by a service at a non-loopback address.

            //= https://www.rfc-editor.org/rfc/rfc9000#section-21.5.6
            //# Similarly, endpoints could regard a change in address to a link-local
            //# address [RFC4291] or an address in a private-use range [RFC1918] from
            //# a global, unique-local [RFC4193], or non-private address as a
            //# potential attempt at request forgery.

            // Here, we ensure the ip scope match so peers are unable to change after
            // establishing a connection
            match (active_addr.unicast_scope(), packet_addr.unicast_scope()) {
                (Some(a), Some(b)) if a == b => Outcome::Allow,
                _ => Outcome::Deny(DenyReason::IpScopeChanged),
            }
        }
    }

    #[derive(Debug, PartialEq, Eq)]
    enum PortScope {
        System,
        User,
        Dynamic,
    }

    impl PortScope {
        #[inline]
        pub const fn new(value: u16) -> Self {
            //= https://www.rfc-editor.org/rfc/rfc6335#section-6
            //# o  the System Ports, also known as the Well Known Ports, from 0-1023
            //#    (assigned by IANA)
            //#
            //# o  the User Ports, also known as the Registered Ports, from 1024-
            //#    49151 (assigned by IANA)
            //#
            //# o  the Dynamic Ports, also known as the Private or Ephemeral Ports,
            //#    from 49152-65535 (never assigned)
            match value {
                0..=1023 => Self::System,
                1024..=49151 => Self::User,
                49152..=65535 => Self::Dynamic,
            }
        }
    }

    fn to_addr(addr: &SocketAddress) -> crate::inet::SocketAddress {
        match addr {
            SocketAddress::IpV4 { ip, port, .. } => {
                crate::inet::SocketAddressV4::new(**ip, *port).into()
            }
            SocketAddress::IpV6 { ip, port, .. } => {
                crate::inet::SocketAddressV6::new(**ip, *port).into()
            }
        }
    }
}

pub mod disabled {
    use super::*;

    #[derive(Debug, Default)]
    pub struct Validator;

    impl super::Validator for Validator {
        fn on_migration_attempt(&mut self, _attempt: &Attempt) -> Outcome {
            // deny all migration attempts
            Outcome::Deny(DenyReason::ConnectionMigrationDisabled)
        }
    }
}

#[cfg(any(test, feature = "testing"))]
pub mod allow_all {
    use super::*;

    #[derive(Debug, Default)]
    pub struct Validator;

    impl super::Validator for Validator {
        fn on_migration_attempt(&mut self, _attempt: &Attempt) -> Outcome {
            // allow all migration attempts
            Outcome::Allow
        }
    }
}

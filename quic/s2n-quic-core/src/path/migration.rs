// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::event::api::{Path, SocketAddress};

#[derive(Debug)]
#[non_exhaustive]
pub struct Attempt<'a> {
    /// The path that the connection is currently actively using
    pub active_path: Path<'a>,
    /// Information about the packet triggering the migration attempt
    pub packet: PacketInfo<'a>,
}

#[derive(Debug)]
#[non_exhaustive]
pub struct PacketInfo<'a> {
    pub remote_address: SocketAddress<'a>,
    pub local_address: SocketAddress<'a>,
}

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
    Deny,
}

/// Validates a path migration attempt from an active path to another
pub trait Validator: 'static + Send {
    /// Called on each connection migration attempt for a connection
    fn on_migration_attempt(&mut self, attempt: &Attempt) -> Outcome;
}

pub mod default {
    use super::*;

    #[derive(Debug, Default)]
    pub struct Validator;

    impl super::Validator for Validator {
        fn on_migration_attempt(&mut self, attempt: &Attempt) -> Outcome {
            let active_addr = to_addr(&attempt.active_path.remote_addr);
            let packet_addr = to_addr(&attempt.packet.remote_address);

            //= https://www.rfc-editor.org/rfc/rfc9000.txt#21.5.6
            //# it might be possible over time to identify
            //# specific UDP ports that are common targets of attacks or particular
            //# patterns in datagrams that are used for attacks.  Endpoints MAY
            //# choose to avoid sending datagrams to these ports or not send
            //# datagrams that match these patterns prior to validating the
            //# destination address.

            // NOTE: this may cause reachability issues if a peer or NAT use different
            //       port range types for the same connection. Additional research may
            //       be required to determine if this countermeasure needs to be relaxed.
            if PortRangeType::new(active_addr.port()) != PortRangeType::new(packet_addr.port()) {
                return Outcome::Deny;
            }

            //= https://www.rfc-editor.org/rfc/rfc9000.txt#21.5.6
            //# Endpoints MAY prevent connection attempts or
            //# migration to a loopback address.  Endpoints SHOULD NOT allow
            //# connections or migration to a loopback address if the same service
            //# was previously available at a different interface or if the address
            //# was provided by a service at a non-loopback address.

            //= https://www.rfc-editor.org/rfc/rfc9000.txt#21.5.6
            //# Similarly, endpoints could regard a change in address to a link-local
            //# address [RFC4291] or an address in a private-use range [RFC1918] from
            //# a global, unique-local [RFC4193], or non-private address as a
            //# potential attempt at request forgery.

            // Here, we ensure the ip range types match so peers are unable to change after
            // establishing a connection
            if active_addr.range_type() == packet_addr.range_type() {
                Outcome::Allow
            } else {
                Outcome::Deny
            }
        }
    }

    #[derive(Debug, PartialEq, Eq)]
    enum PortRangeType {
        System,
        User,
        Dynamic,
    }

    impl PortRangeType {
        #[inline]
        pub const fn new(value: u16) -> Self {
            //= https://www.rfc-editor.org/rfc/rfc6335.txt#6
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
            Outcome::Deny
        }
    }
}

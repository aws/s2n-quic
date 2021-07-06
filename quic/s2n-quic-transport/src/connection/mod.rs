// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! This module contains the implementation of QUIC `Connections` and their management

use crate::{endpoint, recovery::congestion_controller};
use s2n_quic_core::{connection, inet::SocketAddress, time::Timestamp};

mod api;
mod api_provider;
mod close_sender;
mod connection_container;
mod connection_id_mapper;
mod connection_impl;
mod connection_interests;
mod connection_timers;
mod connection_trait;
mod errors;
pub(crate) mod finalization;
mod internal_connection_id;
pub(crate) mod local_id_registry;
pub(crate) mod peer_id_registry;
mod shared_state;
pub(crate) mod transmission;

pub(crate) use api_provider::{ConnectionApi, ConnectionApiProvider};
pub(crate) use connection_container::{ConnectionContainer, ConnectionContainerIterationResult};
pub(crate) use connection_id_mapper::ConnectionIdMapper;
pub(crate) use connection_interests::ConnectionInterests;
pub(crate) use connection_timers::{ConnectionTimerEntry, ConnectionTimers};
pub(crate) use connection_trait::{args::*, ConnectionTrait as Trait};
pub(crate) use internal_connection_id::{InternalConnectionId, InternalConnectionIdGenerator};
pub(crate) use local_id_registry::LocalIdRegistry;
pub(crate) use peer_id_registry::PeerIdRegistry;
pub(crate) use shared_state::{SharedConnectionState, SynchronizedSharedConnectionState};
pub(crate) use transmission::{ConnectionTransmission, ConnectionTransmissionContext};

pub use api::Connection;
pub use connection_impl::ConnectionImpl as Implementation;
/// re-export core
pub use s2n_quic_core::connection::*;

/// Parameters which are passed to a Connection.
/// These are unique per created connection.
pub struct Parameters<Cfg: endpoint::Config> {
    /// The [`Connection`]s internal identifier
    pub internal_connection_id: InternalConnectionId,
    /// The local ID registry which should be utilized by the connection
    pub local_id_registry: LocalIdRegistry,
    /// The peer ID registry which should be utilized by the connection
    pub peer_id_registry: PeerIdRegistry,
    /// The per-connection timer
    pub timer: ConnectionTimerEntry,
    /// The last utilized remote Connection ID
    pub peer_connection_id: PeerId,
    /// The last utilized local Connection ID
    pub local_connection_id: LocalId,
    /// The peers socket address
    pub peer_socket_address: SocketAddress,
    /// The initial congestion controller for the connection
    pub congestion_controller: <Cfg::CongestionControllerEndpoint as congestion_controller::Endpoint>::CongestionController,
    /// The time the connection is being created
    pub timestamp: Timestamp,
    /// The QUIC protocol version which is used for this particular connection
    pub quic_version: u32,
    /// The limits that were advertised to the peer
    pub limits: connection::Limits,
}

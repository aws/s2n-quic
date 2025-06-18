// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! This module contains the implementation of QUIC `Connections` and their management

use crate::{
    endpoint, recovery::congestion_controller, space::PacketSpaceManager,
    wakeup_queue::WakeupHandle,
};
use s2n_quic_core::{connection, event, event::supervisor, path::mtu, time::Timestamp};

mod api;
mod api_provider;
mod close_sender;
mod connection_container;
mod connection_id_mapper;
mod connection_impl;
mod connection_interests;
mod connection_timers;
mod connection_trait;
pub(crate) mod finalization;
mod internal_connection_id;
pub(crate) mod local_id_registry;
pub(crate) mod open_token;
pub(crate) mod peer_id_registry;
pub(crate) mod transmission;

pub(crate) use api_provider::{ConnectionApi, ConnectionApiProvider};
pub(crate) use connection_container::{ConnectionContainer, ConnectionContainerIterationResult};
pub(crate) use connection_id_mapper::{ConnectionIdMapper, OpenRegistry};
pub(crate) use connection_interests::ConnectionInterests;
pub(crate) use connection_timers::ConnectionTimers;
pub(crate) use connection_trait::ConnectionTrait as Trait;
pub(crate) use internal_connection_id::{InternalConnectionId, InternalConnectionIdGenerator};
pub(crate) use local_id_registry::LocalIdRegistry;
pub(crate) use peer_id_registry::PeerIdRegistry;
pub(crate) use transmission::{ConnectionTransmission, ConnectionTransmissionContext};

pub use api::Connection;
pub use connection_impl::ConnectionImpl as Implementation;
pub use connection_trait::Lock;
pub use open_token::Pair as OpenToken;
/// re-export core
pub use s2n_quic_core::connection::*;

/// Parameters which are passed to a Connection.
/// These are unique per created connection.
pub struct Parameters<'a, Cfg: endpoint::Config> {
    /// The [`Connection`]s internal identifier
    pub internal_connection_id: InternalConnectionId,
    /// The local ID registry which should be utilized by the connection
    pub local_id_registry: LocalIdRegistry,
    /// The peer ID registry which should be utilized by the connection
    pub peer_id_registry: PeerIdRegistry,
    /// The open connections registry which should be utilized by the connection
    /// None for accepted/inbound connections.
    pub open_registry: Option<OpenRegistry>,
    /// The last utilized remote Connection ID
    pub peer_connection_id: PeerId,
    /// The last utilized local Connection ID
    pub local_connection_id: LocalId,
    /// The path handle on which the connection was created
    pub path_handle: Cfg::PathHandle,
    /// The space manager created for the connection
    pub space_manager: PacketSpaceManager<Cfg>,
    /// A struct which triggers a wakeup for the given connection
    ///
    /// This should be called from the application task
    pub wakeup_handle: WakeupHandle<InternalConnectionId>,
    /// The initial congestion controller for the connection
    pub congestion_controller: <Cfg::CongestionControllerEndpoint as congestion_controller::Endpoint>::CongestionController,
    /// The time the connection is being created
    pub timestamp: Timestamp,
    /// The QUIC protocol version which is used for this particular connection
    pub quic_version: u32,
    /// The limits that were advertised to the peer
    pub limits: connection::Limits,
    /// Configuration for the maximum transmission unit (MTU) that can be sent on a path
    pub mtu_config: mtu::Config,
    /// The context that should be passed to all related connection events
    pub event_context: <Cfg::EventSubscriber as event::Subscriber>::ConnectionContext,
    /// The context passed to the connection supervisor
    pub supervisor_context: &'a supervisor::Context<'a>,
    /// The datagram provider for the endpoint
    pub datagram_endpoint: &'a mut Cfg::DatagramEndpoint,
    /// The dc provider for the endpoint
    pub dc_endpoint: &'a mut Cfg::DcEndpoint,
    /// The event subscriber for the endpoint
    pub event_subscriber: &'a mut Cfg::EventSubscriber,
    /// The connection limits provider
    pub limits_endpoint: &'a mut Cfg::ConnectionLimits,
    pub random_endpoint: &'a mut Cfg::RandomGenerator,
    pub interceptor_endpoint: &'a mut Cfg::PacketInterceptor,
}

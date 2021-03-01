//! This module contains the implementation of QUIC `Connections` and their management

use crate::stream::StreamTrait;
use s2n_quic_core::{
    application::ApplicationErrorCode, connection, crypto::tls, endpoint, frame::ConnectionClose,
    inet::SocketAddress, recovery::CongestionController, stream::StreamError, time::Timestamp,
    transport::error::TransportError,
};

mod api;
mod api_provider;
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
pub(crate) use connection_trait::ConnectionTrait as Trait;
pub(crate) use internal_connection_id::{InternalConnectionId, InternalConnectionIdGenerator};
pub(crate) use local_id_registry::LocalIdRegistry;
pub(crate) use peer_id_registry::PeerIdRegistry;
pub(crate) use shared_state::{SharedConnectionState, SynchronizedSharedConnectionState};
pub(crate) use transmission::{ConnectionTransmission, ConnectionTransmissionContext};

pub use api::Connection;
pub use connection_impl::ConnectionImpl as Implementation;
use core::fmt::Debug;
/// re-export core
pub use s2n_quic_core::connection::*;

/// Stores configuration parameters for a connection which might be shared
/// between multiple connections of the same type.
pub trait Config: 'static + Send + Debug {
    /// The congestion controller used for the connection
    type CongestionController: CongestionController;
    /// The type of the Streams which are managed by the `Connection`
    type Stream: StreamTrait;
    /// Session type
    type TLSSession: tls::Session;

    const ENDPOINT_TYPE: endpoint::Type;
}

/// Parameters which are passed to a Connection.
/// These are unique per created connection.
pub struct Parameters<Cfg: Config> {
    /// The connections shared configuration
    pub connection_config: Cfg,
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
    pub congestion_controller: Cfg::CongestionController,
    /// The time the connection is being created
    pub timestamp: Timestamp,
    /// The QUIC protocol version which is used for this particular connection
    pub quic_version: u32,
    /// The limits that were advertised to the peer
    pub limits: connection::Limits,
}

/// Enumerates reasons for closing a connection
#[derive(Clone, Copy, Debug)]
pub enum CloseReason<'a> {
    /// The connection gets closed because the idle timer expired
    IdleTimerExpired,
    /// The connection closed because the peer requested it through a
    /// CONNECTION_CLOSE frame
    PeerImmediateClose(ConnectionClose<'a>),
    /// The connection closed because the local application requested it
    LocalImmediateClose(ApplicationErrorCode),
    /// The connection closed due to a transport error, which requires sending
    /// CONNECTION_CLOSE to the peer
    LocalObservedTransportErrror(TransportError),
    /// The connection closed because the peer requested it by sending a
    /// stateless reset
    StatelessReset,
}

impl<'a> Into<Error> for CloseReason<'a> {
    fn into(self) -> Error {
        match self {
            Self::IdleTimerExpired => Error::IdleTimerExpired,
            Self::PeerImmediateClose(error) => error.into(),
            Self::LocalImmediateClose(error) => error.into(),
            Self::LocalObservedTransportErrror(error) => error.into(),
            Self::StatelessReset => Error::Closed,
        }
    }
}

impl<'a> Into<StreamError> for CloseReason<'a> {
    fn into(self) -> StreamError {
        let error: Error = self.into();
        error.into()
    }
}

#[cfg(any(test, feature = "testing"))]
pub mod testing {
    use super::*;

    #[derive(Debug)]
    pub struct Server;

    impl Config for Server {
        type Stream = crate::stream::StreamImpl;
        type CongestionController = s2n_quic_core::recovery::CubicCongestionController;
        type TLSSession = tls::testing::Session;
        const ENDPOINT_TYPE: endpoint::Type = endpoint::Type::Server;
    }

    #[derive(Debug)]
    pub struct Client;

    impl Config for Client {
        type Stream = crate::stream::StreamImpl;
        type CongestionController = s2n_quic_core::recovery::CubicCongestionController;
        type TLSSession = tls::testing::Session;
        const ENDPOINT_TYPE: endpoint::Type = endpoint::Type::Client;
    }
}

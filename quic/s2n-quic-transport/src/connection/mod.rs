//! This module contains the implementation of QUIC `Connections` and their management

use crate::stream::{StreamLimits, StreamTrait};
use s2n_quic_core::{
    application::ApplicationErrorCode,
    crypto::tls,
    endpoint,
    frame::ConnectionClose,
    inet::SocketAddress,
    stream::StreamError,
    time::Timestamp,
    transport::{
        error::TransportError,
        parameters::{AckSettings, InitialFlowControlLimits},
    },
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
mod internal_connection_id;
mod shared_state;
mod transmission;

pub(crate) use api_provider::{ConnectionApi, ConnectionApiProvider};
pub(crate) use connection_container::{ConnectionContainer, ConnectionContainerIterationResult};
pub(crate) use connection_id_mapper::{ConnectionIdMapper, ConnectionIdMapperRegistration};
pub(crate) use connection_interests::ConnectionInterests;
pub(crate) use connection_timers::{ConnectionTimerEntry, ConnectionTimers};
pub(crate) use connection_trait::ConnectionTrait as Trait;
pub(crate) use internal_connection_id::{InternalConnectionId, InternalConnectionIdGenerator};
pub(crate) use shared_state::{SharedConnectionState, SynchronizedSharedConnectionState};
pub(crate) use transmission::{ConnectionTransmission, ConnectionTransmissionContext};

pub use api::Connection;
pub use connection_impl::ConnectionImpl as Implementation;
/// re-export core
pub use s2n_quic_core::connection::*;

/// Stores configuration parameters for a connection which might be shared
/// between multiple connections of the same type.
pub trait Config: 'static + Send {
    /// The type of the Streams which are managed by the `Connection`
    type Stream: StreamTrait;
    /// Session type
    type TLSSession: tls::Session;

    const ENDPOINT_TYPE: endpoint::EndpointType;

    /// Our initial flow control limits as advertised in transport parameters.
    fn local_flow_control_limits(&self) -> &InitialFlowControlLimits;
    /// Our ack settings as advertised in transport parameters.
    fn local_ack_settings(&self) -> &AckSettings;
    /// Returns the limits for this connection that are not defined through
    /// transport parameters
    fn connection_limits(&self) -> &Limits;
}

/// Parameters which are passed to a Connection.
/// These are unique per created connection.
pub struct Parameters<Cfg: Config> {
    /// The connections shared configuration
    pub connection_config: Cfg,
    /// The [`Connection`]s internal identifier
    pub internal_connection_id: InternalConnectionId,
    /// The connection ID mapper registration which should be utilized by the connection
    pub connection_id_mapper_registration: ConnectionIdMapperRegistration,
    /// The per-connection timer
    pub timer: ConnectionTimerEntry,
    /// The last utilized remote Connection ID
    pub peer_connection_id: ID,
    /// The last utilized local Connection ID
    pub local_connection_id: ID,
    /// The peers socket address
    pub peer_socket_address: SocketAddress,
    /// The time the connection is being created
    pub timestamp: Timestamp,
    /// The QUIC protocol version which is used for this particular connection
    pub quic_version: u32,
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
}

impl<'a> Into<Error> for CloseReason<'a> {
    fn into(self) -> Error {
        match self {
            Self::IdleTimerExpired => Error::IdleTimerExpired,
            Self::PeerImmediateClose(error) => error.into(),
            Self::LocalImmediateClose(error) => error.into(),
            Self::LocalObservedTransportErrror(error) => error.into(),
        }
    }
}

impl<'a> Into<StreamError> for CloseReason<'a> {
    fn into(self) -> StreamError {
        let error: Error = self.into();
        error.into()
    }
}

/// Per-connection limits
#[derive(Debug, Copy, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Limits {
    /// The limits for streams on this connection
    pub stream_limits: StreamLimits,

    // TODO remove this field when more fields are added to increase the size
    // temporary field to supress clippy::trivially_copy_pass_by_ref warnings
    _padding: u64,
}

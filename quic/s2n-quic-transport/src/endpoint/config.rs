//! Configuration parameters for `Endpoint`s

use crate::connection::{self, ConnectionConfig, ConnectionTrait};
use s2n_quic_core::{crypto::tls, endpoint::EndpointType};

/// Configuration paramters for a QUIC endpoint
pub trait EndpointConfig {
    /// The type of connection configurations for connections managed by the
    /// endpoint.
    type ConnectionConfigType: ConnectionConfig;
    /// The type of the TLS endpoint which is utilized
    type TLSEndpointType: tls::Endpoint<
        Session = <Self::ConnectionConfigType as ConnectionConfig>::TLSSession,
    >;
    /// The connections type
    type ConnectionType: ConnectionTrait<Config = Self::ConnectionConfigType>;
    /// The type of the generator of new connection IDs
    type ConnectionIdFormat: connection::id::Format;

    /// The type of the local endpoint
    const ENDPOINT_TYPE: EndpointType =
        <Self::ConnectionConfigType as ConnectionConfig>::ENDPOINT_TYPE;

    /// Obtain the configuration for the next connection to be handled
    fn create_connection_config(&mut self) -> Self::ConnectionConfigType;

    /// Returns the connection ID format for the endpoint
    fn connection_id_format(&mut self) -> &mut Self::ConnectionIdFormat;
}

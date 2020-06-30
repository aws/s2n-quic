//! Configuration parameters for `Endpoint`s

use crate::connection::{ConnectionConfig, ConnectionTrait};
use core::time::Duration;
use s2n_quic_core::{
    connection::ConnectionId, crypto::tls::TLSEndpoint, endpoint::EndpointType,
    packet::DestinationConnectionIDDecoder,
};

/// Configuration paramters for a QUIC endpoint
pub trait EndpointConfig {
    /// The type of connection configurations for connections managed by the
    /// endpoint.
    type ConnectionConfigType: ConnectionConfig;
    /// The type of the TLS endpoint which is utilized
    type TLSEndpointType: TLSEndpoint<
        Session = <Self::ConnectionConfigType as ConnectionConfig>::TLSSession,
    >;
    /// The connections type
    type ConnectionType: ConnectionTrait<Config = Self::ConnectionConfigType>;
    /// The type of the generator of new connection IDs
    type ConnectionIdGeneratorType: ConnectionIdGenerator<DestinationConnectionIDDecoderType = <Self::ConnectionConfigType as ConnectionConfig>::DestinationConnectionIDDecoderType>;

    /// The type of the local endpoint
    const ENDPOINT_TYPE: EndpointType =
        <Self::ConnectionConfigType as ConnectionConfig>::ENDPOINT_TYPE;

    /// Obtain the configuration for the next connection to be handled
    fn create_connection_config(&mut self) -> Self::ConnectionConfigType;
}

/// Generates connection IDs for incoming connections
pub trait ConnectionIdGenerator {
    /// The type which is used to decode connection IDs
    type DestinationConnectionIDDecoderType: DestinationConnectionIDDecoder;

    /// Generates a local connection ID for a new connection
    fn generate_connection_id(&mut self) -> (ConnectionId, Option<Duration>);

    /// Returns a connection id decoder for cononections created by this generator
    fn destination_connection_id_decoder(&self) -> Self::DestinationConnectionIDDecoderType;
}

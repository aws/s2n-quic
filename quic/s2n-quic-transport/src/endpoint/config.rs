//! Configuration parameters for `Endpoint`s

use crate::connection;
use s2n_quic_core::{crypto::tls, endpoint::EndpointType};

/// Configuration paramters for a QUIC endpoint
pub trait Config {
    /// The type of connection configurations for connections managed by the
    /// endpoint.
    type ConnectionConfig: connection::Config;
    /// The type of the TLS endpoint which is utilized
    type TLSEndpoint: tls::Endpoint<
        Session = <Self::ConnectionConfig as connection::Config>::TLSSession,
    >;
    /// The connections type
    type Connection: connection::Trait<Config = Self::ConnectionConfig>;
    /// The connection ID format
    type ConnectionIdFormat: connection::id::Format;
    type TokenFormat: s2n_quic_core::token::Format;

    /// The type of the local endpoint
    const ENDPOINT_TYPE: EndpointType =
        <Self::ConnectionConfig as connection::Config>::ENDPOINT_TYPE;

    /// Obtain the configuration for the next connection to be handled
    fn create_connection_config(&mut self) -> Self::ConnectionConfig;

    /// Returns the tls endpoint value
    fn tls_endpoint(&mut self) -> &mut Self::TLSEndpoint;

    /// Returns the connection ID format for the endpoint
    fn connection_id_format(&mut self) -> &mut Self::ConnectionIdFormat;
}

//! Configuration parameters for `Endpoint`s

use crate::connection;
use s2n_quic_core::{crypto::tls, endpoint, recovery::congestion_controller};

/// Configuration paramters for a QUIC endpoint
pub trait Config: Sized {
    /// The type of connection configurations for connections managed by the
    /// endpoint.
    type ConnectionConfig: connection::Config;
    /// The type of the TLS endpoint which is utilized
    type TLSEndpoint: tls::Endpoint<
        Session = <Self::ConnectionConfig as connection::Config>::TLSSession,
    >;
    type CongestionControllerEndpoint: congestion_controller::Endpoint<
        CongestionController = <Self::ConnectionConfig as connection::Config>::CongestionController,
    >;
    /// The connections type
    type Connection: connection::Trait<Config = Self::ConnectionConfig>;
    /// The connection ID format
    type ConnectionIdFormat: connection::id::Format;
    /// The validation token format
    type TokenFormat: s2n_quic_core::token::Format;
    /// The endpoint limits
    type EndpointLimits: endpoint::Limits;

    /// The type of the local endpoint
    const ENDPOINT_TYPE: endpoint::Type =
        <Self::ConnectionConfig as connection::Config>::ENDPOINT_TYPE;

    /// Obtain the configuration for the next connection to be handled
    fn create_connection_config(&mut self) -> Self::ConnectionConfig;

    /// Returns the context for the endpoint configuration
    fn context(&mut self) -> Context<Self>;
}

pub struct Context<'a, Cfg: Config> {
    /// The congestion controller endpoint associated with the endpoint config
    pub congestion_controller: &'a mut Cfg::CongestionControllerEndpoint,

    /// The connection id format associated with the endpoint config
    pub connection_id_format: &'a mut Cfg::ConnectionIdFormat,

    /// The TLS endpoint associated with the endpoint config
    pub tls: &'a mut Cfg::TLSEndpoint,

    /// The endpoint limits
    pub endpoint_limits: &'a mut Cfg::EndpointLimits,

    /// Token generator / validator
    pub token: &'a mut Cfg::TokenFormat,
}

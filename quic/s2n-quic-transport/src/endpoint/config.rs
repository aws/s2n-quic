// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Configuration parameters for `Endpoint`s

use crate::{connection, stream};
use s2n_quic_core::{
    crypto::tls, endpoint, event, packet, path, random, recovery::congestion_controller,
    stateless_reset,
};

/// Configuration parameters for a QUIC endpoint
pub trait Config: 'static + Send + Sized + core::fmt::Debug {
    /// The type of the TLS endpoint which is utilized
    type TLSEndpoint: tls::Endpoint;
    type CongestionControllerEndpoint: congestion_controller::Endpoint;
    /// The connections type
    type Connection: connection::Trait<Config = Self>;
    /// The type of lock that synchronizes connection state between threads
    type ConnectionLock: connection::Lock<Self::Connection>;
    /// The connection ID format
    type ConnectionIdFormat: connection::id::Format;
    /// The stateless reset token generator
    type StatelessResetTokenGenerator: stateless_reset::token::Generator;
    /// The random data generator
    type RandomGenerator: random::Generator;
    /// The validation token format
    type TokenFormat: s2n_quic_core::token::Format;
    /// The endpoint limits
    type EndpointLimits: endpoint::Limiter;
    /// The connection limits
    type ConnectionLimits: connection::limits::Limiter;
    /// The type of stream
    type Stream: stream::StreamTrait;
    /// The connection close formatter
    type ConnectionCloseFormatter: connection::close::Formatter;
    /// The event subscriber
    type EventSubscriber: event::Subscriber;
    /// The type by which paths are identified
    type PathHandle: path::Handle;
    /// The path migration validator for the endpoint
    type PathMigrationValidator: path::migration::Validator;
    /// The packet_interceptor implementation for the endpoint
    type PacketInterceptor: packet::interceptor::Interceptor;

    /// The type of the local endpoint
    const ENDPOINT_TYPE: endpoint::Type;

    /// Returns the context for the endpoint configuration
    fn context(&mut self) -> Context<Self>;
}

#[derive(Debug)]
pub struct Context<'a, Cfg: Config> {
    /// The congestion controller endpoint associated with the endpoint config
    pub congestion_controller: &'a mut Cfg::CongestionControllerEndpoint,

    /// The connection id format associated with the endpoint config
    pub connection_id_format: &'a mut Cfg::ConnectionIdFormat,

    /// The stateless reset token generator associated with the endpoint config
    pub stateless_reset_token_generator: &'a mut Cfg::StatelessResetTokenGenerator,

    /// The random data generator associated with the endpoint config
    pub random_generator: &'a mut Cfg::RandomGenerator,

    /// The TLS endpoint associated with the endpoint config
    pub tls: &'a mut Cfg::TLSEndpoint,

    /// The endpoint limits
    pub endpoint_limits: &'a mut Cfg::EndpointLimits,

    /// Token generator / validator
    pub token: &'a mut Cfg::TokenFormat,

    /// The connection limits
    pub connection_limits: &'a mut Cfg::ConnectionLimits,

    pub connection_close_formatter: &'a mut Cfg::ConnectionCloseFormatter,

    pub event_subscriber: &'a mut Cfg::EventSubscriber,

    pub path_migration: &'a mut Cfg::PathMigrationValidator,

    pub packet_interceptor: &'a mut Cfg::PacketInterceptor,
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use core::marker::PhantomData;
use s2n_quic_core::{connection::id::Generator, crypto, path};
use s2n_quic_transport::{connection, endpoint, stream};

impl_providers_state! {
    #[derive(Debug, Default)]
    struct Providers {
        congestion_controller: CongestionController,
        connection_close_formatter: ConnectionCloseFormatter,
        connection_id: ConnectionID,
        packet_interceptor: PacketInterceptor,
        stateless_reset_token: StatelessResetToken,
        random: Random,
        event: Event,
        limits: Limits,
        io: IO,
        sync: Sync,
        tls: Tls,
    }

    /// Opaque trait containing all of the configured providers
    trait ClientProviders {}
}

impl<
        CongestionController: congestion_controller::Provider,
        ConnectionCloseFormatter: connection_close_formatter::Provider,
        ConnectionID: connection_id::Provider,
        PacketInterceptor: packet_interceptor::Provider,
        StatelessResetToken: stateless_reset_token::Provider,
        Random: random::Provider,
        Event: event::Provider,
        Limits: limits::Provider,
        IO: io::Provider,
        Sync: sync::Provider,
        Tls: tls::Provider,
    >
    Providers<
        CongestionController,
        ConnectionCloseFormatter,
        ConnectionID,
        PacketInterceptor,
        StatelessResetToken,
        Random,
        Event,
        Limits,
        IO,
        Sync,
        Tls,
    >
{
    pub fn start(self) -> Result<Client, StartError> {
        let Self {
            congestion_controller,
            connection_close_formatter,
            connection_id,
            packet_interceptor,
            stateless_reset_token,
            random,
            event,
            limits,
            io,
            sync,
            tls,
        } = self;

        let congestion_controller = congestion_controller.start().map_err(StartError::new)?;
        let connection_close_formatter = connection_close_formatter
            .start()
            .map_err(StartError::new)?;
        let connection_id = connection_id.start().map_err(StartError::new)?;
        let packet_interceptor = packet_interceptor.start().map_err(StartError::new)?;
        let stateless_reset_token = stateless_reset_token.start().map_err(StartError::new)?;
        let random = random.start().map_err(StartError::new)?;
        let endpoint_limits = EndpointLimits;
        let limits = limits.start().map_err(StartError::new)?;
        let event = event.start().map_err(StartError::new)?;
        let token = Token;
        let sync = sync.start().map_err(StartError::new)?;
        let path_migration = PathMigration;
        let tls = tls.start_client().map_err(StartError::new)?;

        // Validate providers
        // TODO: Add more validation https://github.com/aws/s2n-quic/issues/285
        let valid_lifetime = |lifetime| {
            (connection::id::MIN_LIFETIME..=connection::id::MAX_LIFETIME).contains(&lifetime)
        };
        if connection_id
            .lifetime()
            .map_or(false, |lifetime| !valid_lifetime(lifetime))
        {
            return Err(StartError::new(connection::id::Error::InvalidLifetime));
        };

        let endpoint_config = EndpointConfig {
            congestion_controller,
            connection_close_formatter,
            connection_id,
            packet_interceptor,
            stateless_reset_token,
            random,
            endpoint_limits,
            event,
            limits,
            sync,
            tls,
            token,
            path_handle: PhantomData,
            path_migration,
        };

        let (endpoint, connector) = endpoint::Endpoint::new_client(endpoint_config);

        // Start the IO last
        let local_addr = io.start(endpoint).map_err(StartError::new)?;

        Ok(Client {
            connector,
            local_addr,
        })
    }
}

#[derive(Debug)]
struct EndpointLimits;

impl endpoint::limits::Limiter for EndpointLimits {
    fn on_connection_attempt(
        &mut self,
        _info: &endpoint::limits::ConnectionAttempt,
    ) -> endpoint::limits::Outcome {
        unreachable!("endpoint limits should not be used with clients")
    }
}

#[derive(Debug)]
struct Token;

impl crate::provider::address_token::Format for Token {
    const TOKEN_LEN: usize = 0;

    fn generate_new_token(
        &mut self,
        _context: &mut s2n_quic_core::token::Context<'_>,
        _source_connection_id: &s2n_quic_core::connection::LocalId,
        _output_buffer: &mut [u8],
    ) -> Option<()> {
        unreachable!("tokens should not be generated with clients")
    }

    fn generate_retry_token(
        &mut self,
        _context: &mut s2n_quic_core::token::Context<'_>,
        _original_destination_connection_id: &s2n_quic_core::connection::InitialId,
        _output_buffer: &mut [u8],
    ) -> Option<()> {
        unreachable!("tokens should not be generated with clients")
    }

    fn validate_token(
        &mut self,
        _context: &mut s2n_quic_core::token::Context<'_>,
        _token: &[u8],
    ) -> Option<s2n_quic_core::connection::InitialId> {
        unreachable!("tokens should not be generated with clients")
    }
}

#[derive(Debug)]
struct PathMigration;

impl crate::provider::path_migration::Validator for PathMigration {
    fn on_migration_attempt(
        &mut self,
        _attempt: &path::migration::Attempt,
    ) -> path::migration::Outcome {
        unreachable!("path migration should not be validated with clients")
    }
}

#[allow(dead_code)] // don't warn on unused providers for now
struct EndpointConfig<
    CongestionController,
    ConnectionCloseFormatter,
    ConnectionID,
    PacketInterceptor,
    PathHandle,
    StatelessResetToken,
    Random,
    Event,
    Limits,
    Sync,
    Tls,
> {
    congestion_controller: CongestionController,
    connection_close_formatter: ConnectionCloseFormatter,
    connection_id: ConnectionID,
    packet_interceptor: PacketInterceptor,
    stateless_reset_token: StatelessResetToken,
    random: Random,
    endpoint_limits: EndpointLimits,
    event: Event,
    limits: Limits,
    sync: Sync,
    tls: Tls,
    token: Token,
    path_handle: PhantomData<PathHandle>,
    path_migration: PathMigration,
}

impl<
        CongestionController: congestion_controller::Endpoint,
        ConnectionCloseFormatter: connection_close_formatter::Formatter,
        ConnectionID: connection::id::Format,
        PacketInterceptor: packet_interceptor::PacketInterceptor,
        PathHandle: path::Handle,
        StatelessResetToken: stateless_reset_token::Generator,
        Random: s2n_quic_core::random::Generator,
        Event: s2n_quic_core::event::Subscriber,
        Limits: s2n_quic_core::connection::limits::Limiter,
        Sync,
        Tls: crypto::tls::Endpoint,
    > core::fmt::Debug
    for EndpointConfig<
        CongestionController,
        ConnectionCloseFormatter,
        ConnectionID,
        PacketInterceptor,
        PathHandle,
        StatelessResetToken,
        Random,
        Event,
        Limits,
        Sync,
        Tls,
    >
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClientConfig").finish()
    }
}

impl<
        CongestionController: congestion_controller::Endpoint,
        ConnectionCloseFormatter: connection_close_formatter::Formatter,
        ConnectionID: connection::id::Format,
        PacketInterceptor: packet_interceptor::PacketInterceptor,
        PathHandle: path::Handle,
        StatelessResetToken: stateless_reset_token::Generator,
        Random: s2n_quic_core::random::Generator,
        Event: s2n_quic_core::event::Subscriber,
        Limits: s2n_quic_core::connection::limits::Limiter,
        Sync: 'static + Send,
        Tls: crypto::tls::Endpoint,
    > endpoint::Config
    for EndpointConfig<
        CongestionController,
        ConnectionCloseFormatter,
        ConnectionID,
        PacketInterceptor,
        PathHandle,
        StatelessResetToken,
        Random,
        Event,
        Limits,
        Sync,
        Tls,
    >
{
    type ConnectionIdFormat = ConnectionID;
    type ConnectionCloseFormatter = ConnectionCloseFormatter;
    type PathHandle = PathHandle;
    type StatelessResetTokenGenerator = StatelessResetToken;
    type RandomGenerator = Random;
    type Connection = connection::Implementation<Self>;
    // TODO allow users to specify another lock type
    type ConnectionLock = std::sync::Mutex<Self::Connection>;
    type CongestionControllerEndpoint = CongestionController;
    type EndpointLimits = EndpointLimits;
    type EventSubscriber = Event;
    type TLSEndpoint = Tls;
    type TokenFormat = Token;
    type ConnectionLimits = Limits;
    type Stream = stream::StreamImpl;
    type PathMigrationValidator = PathMigration;
    type PacketInterceptor = PacketInterceptor;

    const ENDPOINT_TYPE: endpoint::Type = endpoint::Type::Client;

    fn context(&mut self) -> endpoint::Context<Self> {
        endpoint::Context {
            congestion_controller: &mut self.congestion_controller,
            connection_close_formatter: &mut self.connection_close_formatter,
            connection_id_format: &mut self.connection_id,
            packet_interceptor: &mut self.packet_interceptor,
            stateless_reset_token_generator: &mut self.stateless_reset_token,
            random_generator: &mut self.random,
            tls: &mut self.tls,
            endpoint_limits: &mut self.endpoint_limits,
            token: &mut self.token,
            connection_limits: &mut self.limits,
            event_subscriber: &mut self.event,
            path_migration: &mut self.path_migration,
        }
    }
}

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
        endpoint_limits: EndpointLimits,
        event: Event,
        limits: Limits,
        io: IO,
        path_migration: PathMigration,
        sync: Sync,
        tls: Tls,
        address_token: AddressToken,
    }

    /// Opaque trait containing all of the configured providers
    trait ServerProviders {}
}

impl<
        CongestionController: congestion_controller::Provider,
        ConnectionCloseFormatter: connection_close_formatter::Provider,
        ConnectionID: connection_id::Provider,
        PacketInterceptor: packet_interceptor::Provider,
        StatelessResetToken: stateless_reset_token::Provider,
        Random: random::Provider,
        EndpointLimits: endpoint_limits::Provider,
        Event: event::Provider,
        Limits: limits::Provider,
        IO: io::Provider,
        PathMigration: path_migration::Provider,
        Sync: sync::Provider,
        Tls: tls::Provider,
        AddressToken: address_token::Provider,
    >
    Providers<
        CongestionController,
        ConnectionCloseFormatter,
        ConnectionID,
        PacketInterceptor,
        StatelessResetToken,
        Random,
        EndpointLimits,
        Event,
        Limits,
        IO,
        PathMigration,
        Sync,
        Tls,
        AddressToken,
    >
{
    pub fn start(self) -> Result<Server, StartError> {
        let Self {
            congestion_controller,
            connection_close_formatter,
            connection_id,
            packet_interceptor,
            stateless_reset_token,
            random,
            endpoint_limits,
            event,
            limits,
            address_token,
            io,
            path_migration,
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
        let endpoint_limits = endpoint_limits.start().map_err(StartError::new)?;
        let limits = limits.start().map_err(StartError::new)?;
        let event = event.start().map_err(StartError::new)?;
        let address_token = address_token.start().map_err(StartError::new)?;
        let sync = sync.start().map_err(StartError::new)?;
        let path_migration = path_migration.start().map_err(StartError::new)?;
        let tls = tls.start_server().map_err(StartError::new)?;

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
            address_token,
            path_handle: PhantomData,
            path_migration,
        };

        let (endpoint, acceptor) = endpoint::Endpoint::new_server(endpoint_config);

        // Start the IO last
        let local_addr = io.start(endpoint).map_err(StartError::new)?;

        Ok(Server {
            acceptor,
            local_addr,
        })
    }
}

#[allow(dead_code)] // don't warn on unused providers for now
struct EndpointConfig<
    CongestionController,
    ConnectionCloseFormatter,
    ConnectionID,
    PacketInterceptor,
    PathHandle,
    PathMigration,
    StatelessResetToken,
    Random,
    EndpointLimits,
    Event,
    Limits,
    Sync,
    Tls,
    AddressToken,
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
    address_token: AddressToken,
    path_handle: PhantomData<PathHandle>,
    path_migration: PathMigration,
}

impl<
        CongestionController: congestion_controller::Endpoint,
        ConnectionCloseFormatter: connection_close_formatter::Formatter,
        ConnectionID: connection::id::Format,
        PacketInterceptor: packet_interceptor::PacketInterceptor,
        PathMigration: path_migration::Validator,
        PathHandle: path::Handle,
        StatelessResetToken: stateless_reset_token::Generator,
        Random: s2n_quic_core::random::Generator,
        EndpointLimits: s2n_quic_core::endpoint::Limiter,
        Event: s2n_quic_core::event::Subscriber,
        Limits: s2n_quic_core::connection::limits::Limiter,
        Sync,
        Tls: crypto::tls::Endpoint,
        AddressToken: address_token::Format,
    > core::fmt::Debug
    for EndpointConfig<
        CongestionController,
        ConnectionCloseFormatter,
        ConnectionID,
        PacketInterceptor,
        PathHandle,
        PathMigration,
        StatelessResetToken,
        Random,
        EndpointLimits,
        Event,
        Limits,
        Sync,
        Tls,
        AddressToken,
    >
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ServerConfig").finish()
    }
}

impl<
        CongestionController: congestion_controller::Endpoint,
        ConnectionCloseFormatter: connection_close_formatter::Formatter,
        ConnectionID: connection::id::Format,
        PacketInterceptor: packet_interceptor::PacketInterceptor,
        PathHandle: path::Handle,
        PathMigration: path_migration::Validator,
        StatelessResetToken: stateless_reset_token::Generator,
        Random: s2n_quic_core::random::Generator,
        EndpointLimits: s2n_quic_core::endpoint::Limiter,
        Event: s2n_quic_core::event::Subscriber,
        Limits: s2n_quic_core::connection::limits::Limiter,
        Sync: 'static + Send,
        Tls: crypto::tls::Endpoint,
        AddressToken: address_token::Format,
    > endpoint::Config
    for EndpointConfig<
        CongestionController,
        ConnectionCloseFormatter,
        ConnectionID,
        PacketInterceptor,
        PathHandle,
        PathMigration,
        StatelessResetToken,
        Random,
        EndpointLimits,
        Event,
        Limits,
        Sync,
        Tls,
        AddressToken,
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
    type TokenFormat = AddressToken;
    type ConnectionLimits = Limits;
    type Stream = stream::StreamImpl;
    type PathMigrationValidator = PathMigration;
    type PacketInterceptor = PacketInterceptor;

    const ENDPOINT_TYPE: endpoint::Type = endpoint::Type::Server;

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
            token: &mut self.address_token,
            connection_limits: &mut self.limits,
            event_subscriber: &mut self.event,
            path_migration: &mut self.path_migration,
        }
    }
}

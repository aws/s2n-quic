// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use core::marker::PhantomData;
use s2n_quic_core::{connection::id::Generator, crypto, path};
use s2n_quic_transport::{acceptor::Acceptor, connection, endpoint, stream};

impl_providers_state! {
    #[derive(Debug, Default)]
    struct Providers {
        congestion_controller: CongestionController,
        connection_close_formatter: ConnectionCloseFormatter,
        connection_id: ConnectionID,
        stateless_reset_token: StatelessResetToken,
        random: Random,
        endpoint_limits: EndpointLimits,
        event: Event,
        limits: Limits,
        io: IO,
        sync: Sync,
        tls: Tls,
        token: Token,
    }

    /// Opaque trait containing all of the configured providers
    trait ServerProviders {}
}

impl<
        CongestionController: congestion_controller::Provider,
        ConnectionCloseFormatter: connection_close_formatter::Provider,
        ConnectionID: connection_id::Provider,
        StatelessResetToken: stateless_reset_token::Provider,
        Random: random::Provider,
        EndpointLimits: endpoint_limits::Provider,
        Event: event::Provider,
        Limits: limits::Provider,
        IO: io::Provider,
        Sync: sync::Provider,
        Tls: tls::Provider,
        Token: token::Provider,
    >
    Providers<
        CongestionController,
        ConnectionCloseFormatter,
        ConnectionID,
        StatelessResetToken,
        Random,
        EndpointLimits,
        Event,
        Limits,
        IO,
        Sync,
        Tls,
        Token,
    >
{
    pub fn start(self) -> Result<Acceptor, StartError> {
        let Self {
            congestion_controller,
            connection_close_formatter,
            connection_id,
            stateless_reset_token,
            random,
            endpoint_limits,
            event,
            limits,
            token,
            io,
            sync,
            tls,
        } = self;

        let congestion_controller = congestion_controller.start().map_err(StartError::new)?;
        let connection_close_formatter = connection_close_formatter
            .start()
            .map_err(StartError::new)?;
        let connection_id = connection_id.start().map_err(StartError::new)?;
        let stateless_reset_token = stateless_reset_token.start().map_err(StartError::new)?;
        let random = random.start().map_err(StartError::new)?;
        let endpoint_limits = endpoint_limits.start().map_err(StartError::new)?;
        let limits = limits.start().map_err(StartError::new)?;
        let event = event.start().map_err(StartError::new)?;
        let token = token.start().map_err(StartError::new)?;
        let sync = sync.start().map_err(StartError::new)?;
        let tls = tls.start_server().map_err(StartError::new)?;

        // Validate providers
        // TODO: Add more validation https://github.com/awslabs/s2n-quic/issues/285
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
            stateless_reset_token,
            random,
            endpoint_limits,
            event,
            limits,
            sync,
            tls,
            token,
            path_handle: PhantomData,
        };

        let (endpoint, acceptor) = endpoint::Endpoint::new(endpoint_config);

        // Start the IO last
        io.start(endpoint).map_err(StartError::new)?;

        Ok(acceptor)
    }
}

#[allow(dead_code)] // don't warn on unused providers for now
struct EndpointConfig<
    CongestionController,
    ConnectionCloseFormatter,
    ConnectionID,
    PathHandle,
    StatelessResetToken,
    Random,
    EndpointLimits,
    Event,
    Limits,
    Sync,
    Tls,
    Token,
> {
    congestion_controller: CongestionController,
    connection_close_formatter: ConnectionCloseFormatter,
    connection_id: ConnectionID,
    stateless_reset_token: StatelessResetToken,
    random: Random,
    endpoint_limits: EndpointLimits,
    event: Event,
    limits: Limits,
    sync: Sync,
    tls: Tls,
    token: Token,
    path_handle: PhantomData<PathHandle>,
}

impl<
        CongestionController: congestion_controller::Endpoint,
        ConnectionCloseFormatter: connection_close_formatter::Formatter,
        ConnectionID: connection::id::Format,
        PathHandle: path::Handle,
        StatelessResetToken: stateless_reset_token::Generator,
        Random: s2n_quic_core::random::Generator,
        EndpointLimits: s2n_quic_core::endpoint::Limits,
        Event: s2n_quic_core::event::Subscriber,
        Limits: s2n_quic_core::connection::limits::Limiter,
        Sync,
        Tls: crypto::tls::Endpoint,
        Token: token::Format,
    > core::fmt::Debug
    for EndpointConfig<
        CongestionController,
        ConnectionCloseFormatter,
        ConnectionID,
        PathHandle,
        StatelessResetToken,
        Random,
        EndpointLimits,
        Event,
        Limits,
        Sync,
        Tls,
        Token,
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
        PathHandle: path::Handle,
        StatelessResetToken: stateless_reset_token::Generator,
        Random: s2n_quic_core::random::Generator,
        EndpointLimits: s2n_quic_core::endpoint::Limits,
        Event: s2n_quic_core::event::Subscriber,
        Limits: s2n_quic_core::connection::limits::Limiter,
        Sync: 'static + Send,
        Tls: crypto::tls::Endpoint,
        Token: token::Format,
    > endpoint::Config
    for EndpointConfig<
        CongestionController,
        ConnectionCloseFormatter,
        ConnectionID,
        PathHandle,
        StatelessResetToken,
        Random,
        EndpointLimits,
        Event,
        Limits,
        Sync,
        Tls,
        Token,
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

    const ENDPOINT_TYPE: endpoint::Type = endpoint::Type::Server;

    fn context(&mut self) -> endpoint::Context<Self> {
        endpoint::Context {
            congestion_controller: &mut self.congestion_controller,
            connection_close_formatter: &mut self.connection_close_formatter,
            connection_id_format: &mut self.connection_id,
            stateless_reset_token_generator: &mut self.stateless_reset_token,
            random_generator: &mut self.random,
            tls: &mut self.tls,
            endpoint_limits: &mut self.endpoint_limits,
            token: &mut self.token,
            connection_limits: &mut self.limits,
            event_subscriber: &mut self.event,
        }
    }
}

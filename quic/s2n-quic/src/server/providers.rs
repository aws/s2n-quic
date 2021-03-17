// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use core::time::Duration;
use futures::{select_biased, FutureExt};
use s2n_quic_core::{connection::id::Generator, crypto};
use s2n_quic_transport::{acceptor::Acceptor, connection, endpoint, stream};

impl_providers_state! {
    #[derive(Debug, Default)]
    struct Providers {
        clock: Clock,
        congestion_controller: CongestionController,
        connection_id: ConnectionID,
        stateless_reset_token: StatelessResetToken,
        random: Random,
        endpoint_limits: EndpointLimits,
        limits: Limits,
        log: Log,
        runtime: Runtime,
        io: IO,
        sync: Sync,
        tls: Tls,
        token: Token,
    }

    /// Opaque trait containing all of the configured providers
    trait ServerProviders {}
}

impl<
        Clock: clock::Provider,
        CongestionController: congestion_controller::Provider,
        ConnectionID: connection_id::Provider,
        StatelessResetToken: stateless_reset_token::Provider,
        Random: random::Provider,
        EndpointLimits: endpoint_limits::Provider,
        Limits: limits::Provider,
        Log: log::Provider,
        Runtime: runtime::Provider,
        IO: io::Provider,
        Sync: sync::Provider,
        Tls: tls::Provider,
        Token: token::Provider,
    >
    Providers<
        Clock,
        CongestionController,
        ConnectionID,
        StatelessResetToken,
        Random,
        EndpointLimits,
        Limits,
        Log,
        Runtime,
        IO,
        Sync,
        Tls,
        Token,
    >
{
    pub fn start(self) -> Result<Acceptor, StartError> {
        use crate::provider::runtime::Environment;

        let Self {
            clock,
            congestion_controller,
            connection_id,
            stateless_reset_token,
            random,
            endpoint_limits,
            limits,
            log,
            token,
            runtime,
            io,
            sync,
            tls,
        } = self;

        let clock = clock.start().map_err(StartError::new)?;
        let congestion_controller = congestion_controller.start().map_err(StartError::new)?;
        let connection_id = connection_id.start().map_err(StartError::new)?;
        let stateless_reset_token = stateless_reset_token.start().map_err(StartError::new)?;
        let random = random.start().map_err(StartError::new)?;
        let endpoint_limits = endpoint_limits.start().map_err(StartError::new)?;
        let limits = limits.start().map_err(StartError::new)?;
        let log = log.start().map_err(StartError::new)?;
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

        // Start the IO last
        let io = io.start().map_err(StartError::new)?;
        let mut rx = io.rx;
        let mut tx = io.tx;

        let endpoint_config = EndpointConfig {
            congestion_controller,
            connection_id,
            stateless_reset_token,
            random,
            endpoint_limits,
            limits,
            log,
            token,
            sync,
            tls,
        };

        let (mut endpoint, acceptor) = endpoint::Endpoint::new(endpoint_config);

        runtime
            .start(move |environment| {
                use s2n_quic_core::{
                    io::{rx::Rx, tx::Tx},
                    time::Clock,
                };

                async move {
                    loop {
                        // TODO read a `is_closed` atomic value and shutdown if true

                        let now = clock.get_time();

                        let delay = endpoint
                            .next_timer_expiration()
                            .map(|timeout| timeout.saturating_duration_since(now))
                            .unwrap_or_else(|| Duration::from_secs(1));

                        let tx_future = async {
                            // If the TX queue is empty, allow other tasks to make progress by returning
                            // a future which never resolves.
                            if tx.is_empty() {
                                futures::future::pending().await
                            } else {
                                match tx.transmit().await {
                                    Ok(len) => Ok(len),
                                    Err(err) => Err(err.to_string()),
                                }
                            }
                        };

                        // This list of futures is ordered by priority of execution
                        select_biased! {
                            tx_result = tx_future.fuse() => {
                                match tx_result {
                                    Ok(_) => {}
                                    Err(err) => {
                                        // TODO log error
                                        eprintln!("TX ERROR: {}", err);
                                        break;
                                    }
                                }
                            }
                            rx_result = rx.receive().fuse() => {
                                match rx_result {
                                    Ok(0) => continue,
                                    Ok(_) => {
                                        endpoint.receive(&mut rx, clock.get_time());
                                    }
                                    Err(err) => {
                                        // TODO log error
                                        eprintln!("RX ERROR: {}", err);
                                        break;
                                    }
                                }
                            }
                            _ = endpoint.pending_wakeups(now).fuse() => {
                                // do nothing; the wakeups are handled inside the endpoint
                            }
                            _ = environment.delay(delay).fuse() => {
                                // do nothing; timer expiration is handled on each iteration
                            }
                        }

                        endpoint.handle_timers(clock.get_time());
                        endpoint.issue_new_connection_ids(clock.get_time());
                        endpoint.transmit(&mut tx, clock.get_time());
                    }

                    // TODO gracefully shutdown endpoint
                    eprintln!("shutting down endpoint")
                }
            })
            .map_err(StartError::new)?;

        Ok(acceptor)
    }
}

#[allow(dead_code)] // don't warn on unused providers for now
struct EndpointConfig<
    CongestionController,
    ConnectionID,
    StatelessResetToken,
    Random,
    EndpointLimits,
    Limits,
    Log,
    Sync,
    Tls,
    Token,
> {
    congestion_controller: CongestionController,
    connection_id: ConnectionID,
    stateless_reset_token: StatelessResetToken,
    random: Random,
    endpoint_limits: EndpointLimits,
    limits: Limits,
    log: Log,
    sync: Sync,
    tls: Tls,
    token: Token,
}

impl<
        CongestionController: congestion_controller::Endpoint,
        ConnectionID: connection::id::Format,
        StatelessResetToken: stateless_reset_token::Generator,
        Random: s2n_quic_core::random::Generator,
        EndpointLimits: s2n_quic_core::endpoint::Limits,
        Limits: s2n_quic_core::connection::limits::Limiter,
        Log,
        Sync,
        Tls: crypto::tls::Endpoint,
        Token: token::Format,
    > core::fmt::Debug
    for EndpointConfig<
        CongestionController,
        ConnectionID,
        StatelessResetToken,
        Random,
        EndpointLimits,
        Limits,
        Log,
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
        ConnectionID: connection::id::Format,
        StatelessResetToken: stateless_reset_token::Generator,
        Random: s2n_quic_core::random::Generator,
        EndpointLimits: s2n_quic_core::endpoint::Limits,
        Limits: s2n_quic_core::connection::limits::Limiter,
        Log: 'static,
        Sync: 'static,
        Tls: crypto::tls::Endpoint,
        Token: token::Format,
    > endpoint::Config
    for EndpointConfig<
        CongestionController,
        ConnectionID,
        StatelessResetToken,
        Random,
        EndpointLimits,
        Limits,
        Log,
        Sync,
        Tls,
        Token,
    >
{
    type ConnectionIdFormat = ConnectionID;
    type StatelessResetTokenGenerator = StatelessResetToken;
    type RandomGenerator = Random;
    type Connection = connection::Implementation<Self>;
    type CongestionControllerEndpoint = CongestionController;
    type EndpointLimits = EndpointLimits;
    type TLSEndpoint = Tls;
    type TokenFormat = Token;
    type ConnectionLimits = Limits;
    type Stream = stream::StreamImpl;

    const ENDPOINT_TYPE: endpoint::Type = endpoint::Type::Server;

    fn context(&mut self) -> endpoint::Context<Self> {
        endpoint::Context {
            congestion_controller: &mut self.congestion_controller,
            connection_id_format: &mut self.connection_id,
            stateless_reset_token_generator: &mut self.stateless_reset_token,
            random_generator: &mut self.random,
            tls: &mut self.tls,
            endpoint_limits: &mut self.endpoint_limits,
            token: &mut self.token,
            connection_limits: &mut self.limits,
        }
    }
}

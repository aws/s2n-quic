// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

/// Example mitigation for Slowloris-style Denial of Service attacks. For details on this attack,
/// see [QUIC§21.6](https://www.rfc-editor.org/rfc/rfc9000.html#name-slowloris-attacks).
///
/// The Connection Supervisor used in this example may also be used to mitigate the more general
/// Peer Denial of Service attack described in [QUIC§21.9](https://www.rfc-editor.org/rfc/rfc9000.html#name-peer-denial-of-service).
pub mod slowloris {
    use s2n_quic::provider::{
        event,
        event::{events, supervisor, ConnectionMeta, Timestamp},
    };
    use std::time::Duration;

    /// The maximum number of connections that may be active concurrently before
    /// low throughput connections are closed.
    const CONNECTION_COUNT_THRESHOLD: usize = 1000;
    /// The minimum throughput a connection must sustain, in bytes per second
    const MIN_THROUGHPUT: usize = 500;

    /// Define a Connection Context containing any per-connection state you wish to track.
    /// For this example, we need to track the number of bytes transferred and the last
    /// time the transferred byte count was totalled.
    #[derive(Debug, Clone)]
    pub struct MyConnectionContext {
        transferred_bytes: usize,
        last_update: Timestamp,
    }

    /// Define a struct containing any state across all connections you wish to track.
    /// For this example, there is no additional state we need to track so the struct is empty.
    #[derive(Default)]
    pub struct MyConnectionSupervisor;

    /// Implement the `event::Subscriber` trait for your struct. The `create_connection_context`
    /// method must be implemented to initialize the Connection Context for each connection.
    /// Other methods may be implemented as needed.
    impl event::Subscriber for MyConnectionSupervisor {
        type ConnectionContext = MyConnectionContext;

        /// Initialize the Connection Context that is passed to the `supervisor_timeout` and
        /// `on_supervisor_timeout` methods, as well as each connection-related event.
        fn create_connection_context(
            &mut self,
            meta: &events::ConnectionMeta,
            _info: &events::ConnectionInfo,
        ) -> Self::ConnectionContext {
            MyConnectionContext {
                transferred_bytes: 0,
                last_update: meta.timestamp,
            }
        }

        /// Implement `supervisor_timeout` to define the period at which `on_supervisor_timeout` will
        /// be invoked. For this example, a constant of 1 second is used, but this value can be
        /// varied over time or based on the connection.
        fn supervisor_timeout(
            &mut self,
            _conn_context: &mut Self::ConnectionContext,
            _meta: &ConnectionMeta,
            _context: &supervisor::Context,
        ) -> Option<Duration> {
            Some(Duration::from_secs(1))
        }

        /// Implement `on_supervisor_timeout` to define what action should be taken on the connection
        /// when the `supervisor_timeout` expires. For this example, the connection will be closed
        /// immediately (`supervisor::Outcome::ImmediateClose`) if the number of open connections
        /// is greater than `CONNECTION_COUNT_THRESHOLD` and the the throughput of the connection
        /// since the last `supervisor_timeout` has dropped below `MIN_THROUGHPUT`.
        fn on_supervisor_timeout(
            &mut self,
            conn_context: &mut Self::ConnectionContext,
            meta: &ConnectionMeta,
            context: &supervisor::Context,
        ) -> supervisor::Outcome {
            if !context.is_handshaking && context.connection_count > CONNECTION_COUNT_THRESHOLD {
                let elapsed_time = meta.timestamp.duration_since_start()
                    - conn_context.last_update.duration_since_start();

                // Calculate throughput as bytes per second
                let throughput =
                    (conn_context.transferred_bytes as f32 / elapsed_time.as_secs_f32()) as usize;

                if throughput < MIN_THROUGHPUT {
                    // Close the connection immediately without notifying the peer
                    return supervisor::Outcome::ImmediateClose {
                        reason: "Connection throughput was below MIN_THROUGHPUT",
                    };
                }
            }

            // Update the `last_update` timestamp and reset transferred bytes
            conn_context.last_update = meta.timestamp;
            conn_context.transferred_bytes = 0;

            // Allow the connection to continue
            supervisor::Outcome::Continue
        }

        /// Implement `on_tx_stream_progress` to be notified every time forward progress is made
        /// on an outgoing stream.
        fn on_tx_stream_progress(
            &mut self,
            context: &mut Self::ConnectionContext,
            _meta: &events::ConnectionMeta,
            event: &events::TxStreamProgress,
        ) {
            context.transferred_bytes += event.bytes;
        }

        /// Implement `on_rx_progress` to be notified every time forward progress is made on an
        /// incoming stream.
        fn on_rx_stream_progress(
            &mut self,
            context: &mut Self::ConnectionContext,
            _meta: &events::ConnectionMeta,
            event: &events::RxStreamProgress,
        ) {
            context.transferred_bytes += event.bytes;
        }
    }
}

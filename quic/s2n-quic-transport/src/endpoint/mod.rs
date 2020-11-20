//! This module defines a QUIC endpoint

use crate::{
    acceptor::Acceptor,
    connection::{
        self, ConnectionContainer, ConnectionContainerIterationResult, ConnectionIdMapper,
        InternalConnectionId, InternalConnectionIdGenerator, Trait as _,
    },
    timer::TimerManager,
    unbounded_channel,
    wakeup_queue::WakeupQueue,
};
use alloc::collections::VecDeque;
use core::task::{self, Poll};
use s2n_codec::DecoderBufferMut;
use s2n_quic_core::{
    inet::DatagramInfo,
    io::{rx, tx},
    packet::ProtectedPacket,
    time::Timestamp,
};

mod config;
mod initial;
mod version;

pub use config::{Config, Context};
use connection::id::ConnectionInfo;
/// re-export core
pub use s2n_quic_core::endpoint::*;

/// A QUIC `Endpoint`
pub struct Endpoint<Cfg: Config> {
    /// Configuration parameters for the endpoint
    config: Cfg,
    /// Contains all active connections
    connections: ConnectionContainer<Cfg::Connection>,
    /// Creates internal IDs for new connections
    connection_id_generator: InternalConnectionIdGenerator,
    /// Maps from external to internal connection IDs
    connection_id_mapper: ConnectionIdMapper,
    /// Manages timers for connections
    timer_manager: TimerManager<InternalConnectionId>,
    /// Allows to wakeup the endpoint task which might be blocked on waiting for packets
    /// from application tasks (which e.g. enqueued new data to send).
    wakeup_queue: WakeupQueue<InternalConnectionId>,
    /// This queue contains wakeups we retrieved from the [`wakeup_queue`] earlier.
    /// This is not a local variable in order to reuse the allocated queue capacity in between
    /// [`Endpoint`] interactions.
    dequeued_wakeups: VecDeque<InternalConnectionId>,
    version_negotiator: version::Negotiator<Cfg>,
}

// Safety: The endpoint is marked as `!Send`, because the struct contains `Rc`s.
// However those `Rcs` are only referenced by other objects within the `Endpoint`
// and which also get moved.
unsafe impl<Cfg: Config> Send for Endpoint<Cfg> {}

impl<Cfg: Config> Endpoint<Cfg> {
    /// Creates a new QUIC endpoint using the given configuration
    pub fn new(config: Cfg) -> (Self, Acceptor) {
        let (connection_sender, connection_receiver) = unbounded_channel::channel();
        let acceptor = Acceptor::new(connection_receiver);

        let endpoint = Self {
            config,
            connections: ConnectionContainer::new(connection_sender),
            connection_id_generator: InternalConnectionIdGenerator::new(),
            connection_id_mapper: ConnectionIdMapper::new(),
            timer_manager: TimerManager::new(),
            wakeup_queue: WakeupQueue::new(),
            dequeued_wakeups: VecDeque::new(),
            version_negotiator: version::Negotiator::default(),
        };

        (endpoint, acceptor)
    }

    /// Ingests a queue of datagrams
    pub fn receive<'a, Rx: rx::Rx<'a>>(&mut self, rx: &'a mut Rx, timestamp: Timestamp) {
        use rx::{Entry, Queue};

        let mut queue = rx.queue();
        let entries = queue.as_slice_mut();

        for entry in entries.iter_mut() {
            if let Some(remote_address) = entry.remote_address() {
                let datagram = DatagramInfo {
                    timestamp,
                    payload_len: entry.payload_len(),
                    ecn: entry.ecn(),
                    remote_address,
                };

                self.receive_datagram(&datagram, entry.payload_mut())
            }
        }
        let len = entries.len();
        queue.finish(len);
    }

    /// Ingests a single datagram
    fn receive_datagram(&mut self, datagram: &DatagramInfo, payload: &mut [u8]) {
        let endpoint_context = self.config.context();

        // Try to decode the first packet in the datagram
        let buffer = DecoderBufferMut::new(payload);
        let connection_info = ConnectionInfo::new(&datagram.remote_address);
        let (packet, remaining) = if let Ok((packet, remaining)) = ProtectedPacket::decode(
            buffer,
            &connection_info,
            endpoint_context.connection_id_format,
        ) {
            (packet, remaining)
        } else {
            // Packet is not decodable. Skip it.
            // TODO: Potentially add a metric
            dbg!("invalid packet received");
            return;
        };

        // ensure the version is supported
        if self
            .version_negotiator
            .on_packet(datagram, &packet)
            .is_err()
        {
            return;
        }

        let connection_id = match connection::Id::try_from_bytes(packet.destination_connection_id())
        {
            Some(connection_id) => connection_id,
            None => {
                // Ignore the datagram
                dbg!("packet with invalid connection ID received");
                return;
            }
        };

        // TODO validate the connection ID before looking up the connection in the map

        // Try to lookup the internal connection ID and dispatch the packet
        // to the Connection
        if let Some(internal_id) = self
            .connection_id_mapper
            .lookup_internal_connection_id(&connection_id)
        {
            let is_ok = self
                .connections
                .with_connection(internal_id, |conn, shared_state| {
                    // The path `Id` needs to be passed around instead of the path to get around `&mut self` and
                    // `&mut self.path_manager` being borrowed at the same time
                    let path_id = conn
                        .on_datagram_received(
                            shared_state,
                            datagram,
                            &connection_id,
                            endpoint_context.congestion_controller,
                        )
                        .map_err(|_| ())?;

                    if let Err(err) = conn.handle_packet(shared_state, datagram, path_id, packet) {
                        conn.handle_transport_error(shared_state, datagram, err);
                        return Err(());
                    }

                    if let Err(err) = conn.handle_remaining_packets(
                        shared_state,
                        datagram,
                        path_id,
                        connection_id,
                        endpoint_context.connection_id_format,
                        remaining,
                    ) {
                        conn.handle_transport_error(shared_state, datagram, err);
                        return Err(());
                    }

                    Ok(())
                })
                .is_some();

            if !is_ok {
                debug_assert!(
                    false,
                    "Connection was found in external but not internal connection map"
                );
            }

            return;
        }

        if Cfg::ENDPOINT_TYPE.is_server() {
            match packet {
                ProtectedPacket::Initial(packet) => {
                    if let Err(err) = self.handle_initial_packet(datagram, packet, remaining) {
                        dbg!(err);
                    }
                }
                _ => {
                    // If the connection was not found, issue a stateless reset
                    self.enqueue_stateless_reset(datagram, packet.destination_connection_id());
                }
            }
        } else {
            // TODO: Find out what is required for the client. It seems like
            // those should at least send stateless resets on Initial packets
        }

        // TODO: If short packet could not be decoded, check if it's a stateless reset token
        // TODO: Handle version negotiation packets
    }

    /// Enqueues sending a stateless reset to a peer.
    ///
    /// Sending the reset was caused through the passed `datagram`.
    fn enqueue_stateless_reset(
        &mut self,
        _datagram: &DatagramInfo,
        _destination_connection_id: &[u8],
    ) {
        // TODO: Implement me
        dbg!("stateless reset triggered");
    }

    /// Queries the endpoint for outgoing datagrams
    pub fn transmit<'a, Tx: tx::Tx<'a>>(&mut self, tx: &'a mut Tx, timestamp: Timestamp) {
        let mut queue = tx.queue();

        // Iterate over all connections which want to transmit data
        let mut transmit_result = Ok(());
        self.connections
            .iterate_transmission_list(|connection, shared_state| {
                transmit_result = connection.on_transmit(shared_state, &mut queue, timestamp);
                if transmit_result.is_err() {
                    // If one connection fails, return
                    ConnectionContainerIterationResult::BreakAndInsertAtBack
                } else {
                    ConnectionContainerIterationResult::Continue
                }
            });

        if transmit_result.is_ok() {
            self.version_negotiator.on_transmit(&mut queue);
        }
    }

    /// Handles all timer events. This should be called when a timer expired
    /// according to [`next_timer_expiration()`].
    pub fn handle_timers(&mut self, now: Timestamp) {
        for internal_id in self.timer_manager.expirations(now) {
            self.connections
                .with_connection(internal_id, |conn, shared_state| {
                    conn.on_timeout(shared_state, now);
                });
        }
    }

    /// Returns a future that handles wakeup events
    pub fn pending_wakeups(&mut self, timestamp: Timestamp) -> PendingWakeups<Cfg> {
        PendingWakeups {
            endpoint: self,
            timestamp,
        }
    }

    /// Handles all wakeup events.
    /// This should be called in every eventloop iteration.
    /// Returns the number of wakeups which have occurred and had been handled.
    pub fn poll_pending_wakeups(
        &mut self,
        context: &'_ task::Context,
        timestamp: Timestamp,
    ) -> Poll<usize> {
        // The mem::replace is needed to work around a limitation which does not allow us to pass
        // the new queue directly - even though we will populate the field again after the call.
        let dequeued_wakeups = core::mem::replace(&mut self.dequeued_wakeups, VecDeque::new());
        self.dequeued_wakeups = self
            .wakeup_queue
            .poll_pending_wakeups(dequeued_wakeups, context);
        let nr_wakeups = self.dequeued_wakeups.len();

        for internal_id in &self.dequeued_wakeups {
            self.connections
                .with_connection(*internal_id, |conn, shared_state| {
                    conn.on_wakeup(shared_state, timestamp);
                });
        }

        if nr_wakeups > 0 {
            Poll::Ready(nr_wakeups)
        } else {
            Poll::Pending
        }
    }

    /// Returns the timestamp when the [`handle_timers`] method of the `Endpoint`
    /// should be called next time.
    pub fn next_timer_expiration(&self) -> Option<Timestamp> {
        self.timer_manager.next_expiration()
    }
}

/// A future for handling wakeup events on an endpoint
pub struct PendingWakeups<'a, Cfg: Config> {
    endpoint: &'a mut Endpoint<Cfg>,
    timestamp: Timestamp,
}

impl<'a, Cfg: Config> core::future::Future for PendingWakeups<'a, Cfg> {
    type Output = usize;

    fn poll(
        mut self: core::pin::Pin<&mut Self>,
        cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Self::Output> {
        let timestamp = self.timestamp;
        self.endpoint.poll_pending_wakeups(cx, timestamp)
    }
}

#[cfg(any(test, feature = "testing"))]
pub mod testing {
    use super::*;

    #[derive(Debug)]
    pub struct Server;

    impl Config for Server {
        type CongestionControllerEndpoint = crate::recovery::testing::Endpoint;
        type TLSEndpoint = s2n_quic_core::crypto::tls::testing::Endpoint;
        type ConnectionConfig = connection::testing::Server;
        type Connection = connection::Implementation<Self::ConnectionConfig>;
        type EndpointLimits = Limits;
        type ConnectionIdFormat = connection::id::testing::Format;
        type TokenFormat = s2n_quic_core::token::testing::Format;

        fn create_connection_config(&mut self) -> Self::ConnectionConfig {
            todo!()
        }

        fn context(&mut self) -> super::Context<Self> {
            todo!()
        }
    }

    #[derive(Debug)]
    pub struct Client;

    impl Config for Client {
        type CongestionControllerEndpoint = crate::recovery::testing::Endpoint;
        type TLSEndpoint = s2n_quic_core::crypto::tls::testing::Endpoint;
        type ConnectionConfig = connection::testing::Client;
        type Connection = connection::Implementation<Self::ConnectionConfig>;
        type EndpointLimits = Limits;
        type ConnectionIdFormat = connection::id::testing::Format;
        type TokenFormat = s2n_quic_core::token::testing::Format;

        fn create_connection_config(&mut self) -> Self::ConnectionConfig {
            todo!()
        }

        fn context(&mut self) -> super::Context<Self> {
            todo!()
        }
    }

    #[derive(Debug)]
    pub struct Limits;

    impl super::Limits for Limits {
        fn on_connection_attempt(
            &mut self,
            _attempt: &super::limits::ConnectionAttempt,
        ) -> super::limits::Outcome {
            super::limits::Outcome::Allow
        }
    }
}

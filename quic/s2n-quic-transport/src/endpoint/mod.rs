//! This module defines a QUIC endpoint

use crate::{
    acceptor::Acceptor,
    connection::{
        ConnectionContainer, ConnectionContainerIterationResult, ConnectionIdMapper,
        ConnectionTrait, InternalConnectionId, InternalConnectionIdGenerator,
    },
    contexts::ConnectionWriteContext,
    timer::TimerManager,
    unbounded_channel,
    wakeup_queue::WakeupQueue,
};
use alloc::collections::VecDeque;
use core::task::{Context, Poll};
use s2n_codec::DecoderBufferMut;
use s2n_quic_core::{
    connection::ConnectionId, inet::DatagramInfo, packet::ProtectedPacket, time::Timestamp,
};
use s2n_quic_platform::io::tx::TxQueue;

mod config;
pub use config::{ConnectionIdGenerator, EndpointConfig};

mod initial;

/// A QUIC `Endpoint`
pub struct Endpoint<ConfigType: EndpointConfig> {
    /// Configuration parameters for the endpoint
    config: ConfigType,
    /// Contains all active connections
    connections: ConnectionContainer<ConfigType::ConnectionType>,
    /// Creates internal IDs for new connections
    connection_id_generator: InternalConnectionIdGenerator,
    /// Maps from external to internal connection IDs
    connection_id_mapper: ConnectionIdMapper,
    /// Generates local connection IDs
    local_connection_id_generator: ConfigType::ConnectionIdGeneratorType,
    /// Manages timers for connections
    timer_manager: TimerManager<InternalConnectionId>,
    /// Manages TLS sessions
    tls_endpoint: ConfigType::TLSEndpointType,
    /// Allows to wakeup the endpoint task which might be blocked on waiting for packets
    /// from application tasks (which e.g. enqueued new data to send).
    wakeup_queue: WakeupQueue<InternalConnectionId>,
    /// This queue contains wakeups we retrieved from the [`wakeup_queue`] earlier.
    /// This is not a local variable in order to reuse the allocated queue capacity in between
    /// [`Endpoint`] interactions.
    dequeued_wakeups: VecDeque<InternalConnectionId>,
}

// Safety: The endpoint is marked as `!Send`, because the struct contains `Rc`s.
// However those `Rcs` are only referenced by other objects within the `Endpoint`
// and which also get moved.
unsafe impl<ConfigType: EndpointConfig> Send for Endpoint<ConfigType> {}

impl<ConfigType: EndpointConfig> Endpoint<ConfigType> {
    /// Creates a new QUIC endpoint using the given configuration
    pub fn new(
        config: ConfigType,
        connection_id_generator: ConfigType::ConnectionIdGeneratorType,
        tls_endpoint: ConfigType::TLSEndpointType,
    ) -> (Self, Acceptor) {
        let (connection_sender, connection_receiver) = unbounded_channel::channel();
        let acceptor = Acceptor::new(connection_receiver);

        let endpoint = Self {
            config,
            connections: ConnectionContainer::new(connection_sender),
            connection_id_generator: InternalConnectionIdGenerator::new(),
            connection_id_mapper: ConnectionIdMapper::new(),
            local_connection_id_generator: connection_id_generator,
            timer_manager: TimerManager::new(),
            tls_endpoint,
            wakeup_queue: WakeupQueue::new(),
            dequeued_wakeups: VecDeque::new(),
        };

        (endpoint, acceptor)
    }

    /// Ingests a datagram
    pub fn receive_datagram(&mut self, datagram: &DatagramInfo, payload: &mut [u8]) {
        // Obtain the connection ID decoder from the generator that we are using.
        let destination_connection_id_decoder = self
            .local_connection_id_generator
            .destination_connection_id_decoder();

        // Try to decode the first packet in the datagram
        let buffer = DecoderBufferMut::new(payload);
        let (packet, remaining) = if let Ok((packet, remaining)) =
            ProtectedPacket::decode(buffer, destination_connection_id_decoder)
        {
            (packet, remaining)
        } else {
            // Packet is not decodable. Skip it.
            // TODO: Potentially add a metric
            return;
        };

        let connection_id = match ConnectionId::try_from_bytes(packet.destination_connection_id()) {
            Some(connection_id) => connection_id,
            None => return, // Ignore the datagram
        };

        // Try to lookup the internal connection ID and dispatch the packet
        // to the Connection
        if let Some(internal_id) = self
            .connection_id_mapper
            .lookup_internal_connection_id(&connection_id)
        {
            let is_ok = self
                .connections
                .with_connection(internal_id, |conn, shared_state| {
                    if let Err(e) = conn.handle_first_and_remaining_packets(
                        shared_state,
                        datagram,
                        packet,
                        connection_id,
                        remaining,
                    ) {
                        eprintln!("Packet handling error: {:?}", e);
                    }
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

        if ConfigType::ENDPOINT_TYPE.is_server() {
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
    }

    /// Queries the endpoint for outgoing datagrams
    pub fn transmit<Tx: TxQueue>(&mut self, tx: &mut Tx, timestamp: Timestamp) {
        /// Adaptor struct from `Tx` to `ConnectionWriteContext`.
        /// Should be extracted as soon as more parameters are used for
        /// `ConnectionWriteContext`
        struct WriteContext<'a, QueueType> {
            queue: &'a mut QueueType,
        }

        impl<'a, QueueType: TxQueue> ConnectionWriteContext for WriteContext<'a, QueueType> {
            type QueueType = QueueType;

            fn tx_queue(&mut self) -> &mut Self::QueueType {
                self.queue
            }
        }

        let mut context = WriteContext { queue: tx };

        // Iterate over all connections which want to transmit data
        let mut transmit_result = Ok(());
        self.connections
            .iterate_transmission_list(|connection, shared_state| {
                transmit_result = connection.on_transmit(shared_state, &mut context, timestamp);
                if transmit_result.is_err() {
                    // If one connection fails, return
                    ConnectionContainerIterationResult::BreakAndInsertAtBack
                } else {
                    ConnectionContainerIterationResult::Continue
                }
            });

        let _ = transmit_result; // TODO: Do something in the error case
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

    /// Handles all wakeup events.
    /// This should be called in every eventloop iteration.
    /// Returns the number of wakeups which have occurred and had been handled.
    pub fn poll_pending_wakeups(&mut self, context: &'_ Context) -> Poll<usize> {
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
                    conn.on_wakeup(shared_state);
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

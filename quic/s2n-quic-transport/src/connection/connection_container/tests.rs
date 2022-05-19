// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::{
    connection::{
        self, connection_interests::ConnectionInterests,
        internal_connection_id::InternalConnectionId, InternalConnectionIdGenerator,
        ProcessingError,
    },
    endpoint, path, stream,
};
use bolero::{check, generator::*};
use bytes::Bytes;
use core::{
    task::{Context, Poll},
    time::Duration,
};
use s2n_quic_core::{
    application, event,
    event::builder::DatagramDropReason,
    inet::{DatagramInfo, SocketAddress},
    io::tx,
    packet::{
        handshake::ProtectedHandshake,
        initial::{CleartextInitial, ProtectedInitial},
        retry::ProtectedRetry,
        short::ProtectedShort,
        version_negotiation::ProtectedVersionNegotiation,
        zero_rtt::ProtectedZeroRtt,
    },
    path::MaxMtu,
    time::{Timer, Timestamp},
};
use std::sync::Mutex;

struct TestConnection {
    is_handshaking: bool,
    has_been_accepted: bool,
    is_closed: bool,
    interests: ConnectionInterests,
    close_timer: Timer,
}

impl Default for TestConnection {
    fn default() -> Self {
        Self {
            is_handshaking: true,
            has_been_accepted: false,
            is_closed: false,
            interests: ConnectionInterests {
                transmission: true,
                ..Default::default()
            },
            close_timer: Default::default(),
        }
    }
}

impl connection::Trait for TestConnection {
    type Config = crate::endpoint::testing::Server;

    fn new(_params: connection::Parameters<Self::Config>) -> Result<Self, connection::Error> {
        Ok(Self::default())
    }

    fn internal_connection_id(&self) -> InternalConnectionId {
        todo!()
    }

    fn is_handshaking(&self) -> bool {
        self.is_handshaking
    }

    fn close(
        &mut self,
        _error: connection::Error,
        _close_formatter: &<Self::Config as endpoint::Config>::ConnectionCloseFormatter,
        _packet_buffer: &mut endpoint::PacketBuffer,
        timestamp: Timestamp,
        _subscriber: &mut <Self::Config as endpoint::Config>::EventSubscriber,
        _packet_interceptor: &mut <Self::Config as endpoint::Config>::PacketInterceptor,
    ) {
        assert!(!self.is_closed);
        assert!(!self.close_timer.is_armed());
        self.close_timer.set(timestamp + Duration::from_secs(1));
    }

    fn mark_as_accepted(&mut self) {
        assert!(!self.has_been_accepted);
        self.has_been_accepted = true;
        self.interests.accept = false;
    }

    fn on_new_connection_id(
        &mut self,
        _connection_id_format: &mut <Self::Config as endpoint::Config>::ConnectionIdFormat,
        _stateless_reset_token_generator: &mut <Self::Config as endpoint::Config>::StatelessResetTokenGenerator,
        _timestamp: Timestamp,
    ) -> Result<(), connection::local_id_registry::LocalIdRegistrationError> {
        Ok(())
    }

    fn on_transmit<Tx: tx::Queue>(
        &mut self,
        _queue: &mut Tx,
        _timestamp: Timestamp,
        _subscriber: &mut <Self::Config as endpoint::Config>::EventSubscriber,
        _packet_interceptor: &mut <Self::Config as endpoint::Config>::PacketInterceptor,
    ) -> Result<(), crate::contexts::ConnectionOnTransmitError> {
        Ok(())
    }

    fn on_timeout(
        &mut self,
        _connection_id_mapper: &mut connection::ConnectionIdMapper,
        timestamp: Timestamp,
        _supervisor_context: &supervisor::Context,
        _random_generator: &mut <Self::Config as endpoint::Config>::RandomGenerator,
        _subscriber: &mut <Self::Config as endpoint::Config>::EventSubscriber,
    ) -> Result<(), connection::Error> {
        if self.close_timer.poll_expiration(timestamp).is_ready() {
            self.is_closed = true;
        }
        Ok(())
    }

    fn on_pending_ack_ranges(
        &mut self,
        _random_generator: &mut <Self::Config as endpoint::Config>::RandomGenerator,
        _timestamp: Timestamp,
        _subscriber: &mut <Self::Config as endpoint::Config>::EventSubscriber,
    ) -> Result<(), connection::Error> {
        Ok(())
    }

    fn on_wakeup(
        &mut self,
        _timestamp: Timestamp,
        _subscriber: &mut <Self::Config as endpoint::Config>::EventSubscriber,
        _datagram: &mut <Self::Config as endpoint::Config>::DatagramEndpoint,
    ) -> Result<(), connection::Error> {
        Ok(())
    }

    fn handle_initial_packet(
        &mut self,
        _datagram: &DatagramInfo,
        _path_id: path::Id,
        _packet: ProtectedInitial,
        _random_generator: &mut <Self::Config as endpoint::Config>::RandomGenerator,
        _subscriber: &mut <Self::Config as endpoint::Config>::EventSubscriber,
        _packet_interceptor: &mut <Self::Config as endpoint::Config>::PacketInterceptor,
        _datagram_endpoint: &mut <Self::Config as endpoint::Config>::DatagramEndpoint,
    ) -> Result<(), ProcessingError> {
        Ok(())
    }

    /// Is called when an unprotected initial packet had been received
    fn handle_cleartext_initial_packet(
        &mut self,
        _datagram: &DatagramInfo,
        _path_id: path::Id,
        _packet: CleartextInitial,
        _random_generator: &mut <Self::Config as endpoint::Config>::RandomGenerator,
        _subscriber: &mut <Self::Config as endpoint::Config>::EventSubscriber,
        _packet_interceptor: &mut <Self::Config as endpoint::Config>::PacketInterceptor,
        _datagram_endpoint: &mut <Self::Config as endpoint::Config>::DatagramEndpoint,
    ) -> Result<(), ProcessingError> {
        Ok(())
    }

    /// Is called when a handshake packet had been received
    fn handle_handshake_packet(
        &mut self,
        _datagram: &DatagramInfo,
        _path_id: path::Id,
        _packet: ProtectedHandshake,
        _random_generator: &mut <Self::Config as endpoint::Config>::RandomGenerator,
        _subscriber: &mut <Self::Config as endpoint::Config>::EventSubscriber,
        _packet_interceptor: &mut <Self::Config as endpoint::Config>::PacketInterceptor,
        _datagram_endpoint: &mut <Self::Config as endpoint::Config>::DatagramEndpoint,
    ) -> Result<(), ProcessingError> {
        Ok(())
    }

    /// Is called when a short packet had been received
    fn handle_short_packet(
        &mut self,
        _datagram: &DatagramInfo,
        _path_id: path::Id,
        _packet: ProtectedShort,
        _random_generator: &mut <Self::Config as endpoint::Config>::RandomGenerator,
        _subscriber: &mut <Self::Config as endpoint::Config>::EventSubscriber,
        _packet_interceptor: &mut <Self::Config as endpoint::Config>::PacketInterceptor,
    ) -> Result<(), ProcessingError> {
        Ok(())
    }

    /// Is called when a version negotiation packet had been received
    fn handle_version_negotiation_packet(
        &mut self,
        _datagram: &DatagramInfo,
        _path_id: path::Id,
        _packet: ProtectedVersionNegotiation,
        _subscriber: &mut <Self::Config as endpoint::Config>::EventSubscriber,
        _packet_interceptor: &mut <Self::Config as endpoint::Config>::PacketInterceptor,
    ) -> Result<(), ProcessingError> {
        Ok(())
    }

    /// Is called when a zero rtt packet had been received
    fn handle_zero_rtt_packet(
        &mut self,
        _datagram: &DatagramInfo,
        _path_id: path::Id,
        _packet: ProtectedZeroRtt,
        _subscriber: &mut <Self::Config as endpoint::Config>::EventSubscriber,
        _packet_interceptor: &mut <Self::Config as endpoint::Config>::PacketInterceptor,
    ) -> Result<(), ProcessingError> {
        Ok(())
    }

    /// Is called when a retry packet had been received
    fn handle_retry_packet(
        &mut self,
        _datagram: &DatagramInfo,
        _path_id: path::Id,
        _packet: ProtectedRetry,
        _subscriber: &mut <Self::Config as endpoint::Config>::EventSubscriber,
        _packet_interceptor: &mut <Self::Config as endpoint::Config>::PacketInterceptor,
    ) -> Result<(), ProcessingError> {
        Ok(())
    }

    /// Notifies a connection it has received a datagram from a peer
    fn on_datagram_received(
        &mut self,
        _path: &<Self::Config as endpoint::Config>::PathHandle,
        _datagram: &DatagramInfo,
        _congestion_controller_endpoint: &mut <Self::Config as endpoint::Config>::CongestionControllerEndpoint,
        _path_migration: &mut <Self::Config as endpoint::Config>::PathMigrationValidator,
        _max_mtu: MaxMtu,
        _subscriber: &mut <Self::Config as endpoint::Config>::EventSubscriber,
    ) -> Result<path::Id, DatagramDropReason> {
        todo!()
    }

    /// Returns the Connections interests
    fn interests(&self) -> ConnectionInterests {
        self.interests
    }

    /// Returns the QUIC version selected for the current connection
    fn quic_version(&self) -> u32 {
        123
    }

    fn poll_stream_request(
        &mut self,
        _stream_id: stream::StreamId,
        _request: &mut stream::ops::Request,
        _context: Option<&Context>,
    ) -> Result<stream::ops::Response, stream::StreamError> {
        todo!()
    }

    fn poll_accept_stream(
        &mut self,
        _stream_type: Option<stream::StreamType>,
        _context: &Context,
    ) -> Poll<Result<Option<stream::StreamId>, connection::Error>> {
        todo!()
    }

    fn poll_open_stream(
        &mut self,
        _stream_type: stream::StreamType,
        _token: &mut connection::OpenToken,
        _context: &Context,
    ) -> Poll<Result<stream::StreamId, connection::Error>> {
        todo!()
    }

    fn application_close(&mut self, _error: Option<application::Error>) {
        // no-op
    }

    fn server_name(&self) -> Option<ServerName> {
        todo!()
    }

    fn application_protocol(&self) -> Bytes {
        todo!()
    }

    fn ping(&mut self) -> Result<(), connection::Error> {
        todo!()
    }

    fn keep_alive(&mut self, _enabled: bool) -> Result<(), connection::Error> {
        todo!()
    }

    fn local_address(&self) -> Result<SocketAddress, connection::Error> {
        todo!()
    }

    fn remote_address(&self) -> Result<SocketAddress, connection::Error> {
        Ok(SocketAddress::default())
    }

    fn error(&self) -> Option<connection::Error> {
        None
    }

    fn query_event_context(&self, _query: &mut dyn event::query::Query) {
        todo!()
    }

    fn query_event_context_mut(&mut self, _query: &mut dyn event::query::QueryMut) {
        todo!()
    }

    fn with_event_publisher<F>(
        &mut self,
        _timestamp: Timestamp,
        _path_id: Option<path::Id>,
        _subscriber: &mut <Self::Config as endpoint::Config>::EventSubscriber,
        _f: F,
    ) where
        F: FnOnce(
            &mut event::ConnectionPublisherSubscriber<
                <Self::Config as endpoint::Config>::EventSubscriber,
            >,
            &path::Path<Self::Config>,
        ),
    {
        todo!()
    }
}

struct TestLock {
    connection: Mutex<(TestConnection, bool)>,
}

impl TestLock {
    fn poison(&self) {
        if let Ok(mut lock) = self.connection.lock() {
            lock.1 = true;
        }
    }
}

impl connection::Lock<TestConnection> for TestLock {
    type Error = ();

    fn new(connection: TestConnection) -> Self {
        Self {
            connection: std::sync::Mutex::new((connection, false)),
        }
    }

    fn read<F: FnOnce(&TestConnection) -> R, R>(&self, f: F) -> Result<R, Self::Error> {
        let lock = self.connection.lock().map_err(|_| ())?;
        let (conn, is_poisoned) = &*lock;
        if *is_poisoned {
            return Err(());
        }
        let result = f(conn);
        Ok(result)
    }

    fn write<F: FnOnce(&mut TestConnection) -> R, R>(&self, f: F) -> Result<R, Self::Error> {
        let mut lock = self.connection.lock().map_err(|_| ())?;
        let (conn, is_poisoned) = &mut *lock;
        if *is_poisoned {
            return Err(());
        }
        let result = f(conn);
        Ok(result)
    }
}

#[derive(Debug, TypeGenerator)]
enum Operation {
    Insert,
    UpdateInterests {
        index: usize,
        finalization: bool,
        closing: bool,
        accept: bool,
        transmission: bool,
        new_connection_id: bool,
        timeout: Option<u16>,
    },
    CloseApp,
    Receive,
    Timeout(u16),
    Transmit(u16),
    NewConnId(u16),
    Finalize,
    Poison(usize),
}

#[test]
fn container_test() {
    use core::time::Duration;

    check!().with_type::<Vec<Operation>>().for_each(|ops| {
        let mut id_gen = InternalConnectionIdGenerator::new();
        let mut connections = vec![];
        let (handle, acceptor, connector, _close_handle) = endpoint::handle::Handle::new(100);
        let (waker, _wake_count) = futures_test::task::new_count_waker();
        let mut now = unsafe { Timestamp::from_duration(Duration::from_secs(0)) };

        let mut handle = Some(handle);
        let mut container: ConnectionContainer<TestConnection, TestLock> =
            ConnectionContainer::new(acceptor, connector);

        for op in ops.iter() {
            match op {
                Operation::Insert => {
                    let id = id_gen.generate_id();
                    let connection = TestConnection::default();
                    container.insert_connection(connection, id);
                    connections.push(id);

                    let mut was_called = false;
                    container.with_connection(id, |_conn| {
                        was_called = true;
                    });
                    assert!(was_called);
                }
                Operation::UpdateInterests {
                    index,
                    finalization,
                    closing,
                    accept,
                    transmission,
                    new_connection_id,
                    timeout,
                } => {
                    if connections.is_empty() {
                        continue;
                    }
                    let index = index % connections.len();
                    let id = connections[index];

                    let mut was_called = false;
                    container.with_connection(id, |conn| {
                        was_called = true;

                        let i = &mut conn.interests;

                        if *finalization {
                            // in the finalization state, that should be the only interest
                            *i = ConnectionInterests {
                                finalization: true,
                                ..Default::default()
                            };
                            return;
                        }

                        i.closing = *closing;
                        if !conn.has_been_accepted {
                            i.accept = *accept;
                        }
                        if *accept {
                            conn.is_handshaking = false;
                        }
                        i.transmission = *transmission;
                        i.new_connection_id = *new_connection_id;
                        i.timeout = timeout.map(|ms| now + Duration::from_millis(ms as _));

                        // we need to express at least one interest to ensure progress
                        if !(i.transmission || i.new_connection_id || i.timeout.is_some()) {
                            i.transmission = true;
                        }
                    });

                    if *finalization {
                        connections.remove(index);
                    }

                    assert!(was_called);
                }
                Operation::CloseApp => {
                    handle = None;
                }
                Operation::Receive => {
                    if let Some(handle) = handle.as_mut() {
                        while let Poll::Ready(Some(_accepted)) = handle
                            .acceptor
                            .poll_accept(&mut Context::from_waker(&waker))
                        {
                            // TODO assert that the accepted connection expressed accept
                            // interest
                        }
                    }
                }
                Operation::Timeout(ms) => {
                    now += Duration::from_millis(*ms as _);
                    container.iterate_timeout_list(now, |conn, _context| {
                        let i = &mut conn.interests;

                        assert!(
                            i.timeout.take().unwrap() <= now,
                            "connections should only be present when timeout interest is expressed"
                        );

                        // we need to express at least one interest to ensure progress
                        if !(i.transmission || i.new_connection_id || i.timeout.is_some()) {
                            i.transmission = true;
                        }
                    });
                }
                Operation::Transmit(count) => {
                    let mut count = *count;
                    container.iterate_transmission_list(|conn| {
                        assert!(conn.interests.transmission);

                        if count == 0 {
                            ConnectionContainerIterationResult::BreakAndInsertAtBack
                        } else {
                            count -= 1;
                            ConnectionContainerIterationResult::Continue
                        }
                    })
                }
                Operation::NewConnId(count) => {
                    let mut count = *count;
                    container.iterate_new_connection_id_list(|conn| {
                        assert!(conn.interests.new_connection_id);

                        if count == 0 {
                            ConnectionContainerIterationResult::BreakAndInsertAtBack
                        } else {
                            count -= 1;
                            ConnectionContainerIterationResult::Continue
                        }
                    })
                }
                Operation::Finalize => {
                    container.finalize_done_connections();
                }
                Operation::Poison(index) => {
                    if connections.is_empty() {
                        continue;
                    }
                    let index = index % connections.len();
                    let id = connections[index];

                    let node = container.connection_map.find(&id).get().unwrap();
                    node.inner.poison();

                    let mut was_called = false;
                    container.with_connection(id, |_conn| {
                        was_called = true;
                    });
                    assert!(!was_called);
                    connections.remove(index);
                }
            }
        }

        container.finalize_done_connections();

        let mut connections = connections.drain(..);
        let mut cursor = container.connection_map.front();

        while let Some(conn) = cursor.get() {
            assert_eq!(conn.internal_connection_id, connections.next().unwrap());
            cursor.move_next();
        }

        assert!(connections.next().is_none());
    });
}

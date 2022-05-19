// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! This module contains the implementation of QUIC `Connections` and their management

use crate::{
    connection::{
        self, connection_interests::ConnectionInterests, id::ConnectionInfo,
        internal_connection_id::InternalConnectionId, local_id_registry::LocalIdRegistrationError,
        ConnectionIdMapper, Parameters as ConnectionParameters, ProcessingError,
    },
    contexts::ConnectionOnTransmitError,
    endpoint,
    path::{self, path_event},
    stream,
};
use bytes::Bytes;
use core::task::{Context, Poll};
use s2n_codec::DecoderBufferMut;
use s2n_quic_core::{
    application,
    application::ServerName,
    event::{self, builder::DatagramDropReason, supervisor, ConnectionPublisher, IntoEvent},
    inet::{DatagramInfo, SocketAddress},
    io::tx,
    packet::{
        handshake::ProtectedHandshake,
        initial::{CleartextInitial, ProtectedInitial},
        retry::ProtectedRetry,
        short::ProtectedShort,
        version_negotiation::ProtectedVersionNegotiation,
        zero_rtt::ProtectedZeroRtt,
        ProtectedPacket,
    },
    path::{Handle as _, MaxMtu},
    time::Timestamp,
};

/// A trait which represents an internally used `Connection`
pub trait ConnectionTrait: 'static + Send + Sized {
    /// Static configuration of a connection
    type Config: endpoint::Config;

    /// Creates a new `Connection` instance with the given configuration
    fn new(parameters: ConnectionParameters<Self::Config>) -> Result<Self, connection::Error>;

    /// Returns the Connections internal ID
    fn internal_connection_id(&self) -> InternalConnectionId;

    /// Returns whether the connection is in the handshake state
    fn is_handshaking(&self) -> bool;

    /// Initiates closing the connection as described in
    /// https://www.rfc-editor.org/rfc/rfc9000#section-10
    fn close(
        &mut self,
        error: connection::Error,
        close_formatter: &<Self::Config as endpoint::Config>::ConnectionCloseFormatter,
        packet_buffer: &mut endpoint::PacketBuffer,
        timestamp: Timestamp,
        subscriber: &mut <Self::Config as endpoint::Config>::EventSubscriber,
        packet_interceptor: &mut <Self::Config as endpoint::Config>::PacketInterceptor,
    );

    /// Marks a connection which advertised itself as having completed the handshake
    /// (via [`ConnectionInterests`]) as accepted. After this call the `accept` interest should
    /// no longer be signalled.
    fn mark_as_accepted(&mut self);

    /// Generates and registers new connection IDs using the given `ConnectionIdFormat` and
    /// `StatelessResetTokenGenerator`
    fn on_new_connection_id(
        &mut self,
        connection_id_format: &mut <Self::Config as endpoint::Config>::ConnectionIdFormat,
        stateless_reset_token_generator: &mut <Self::Config as endpoint::Config>::StatelessResetTokenGenerator,
        timestamp: Timestamp,
    ) -> Result<(), LocalIdRegistrationError>;

    /// Queries the connection for outgoing packets
    fn on_transmit<Tx>(
        &mut self,
        queue: &mut Tx,
        timestamp: Timestamp,
        subscriber: &mut <Self::Config as endpoint::Config>::EventSubscriber,
        packet_interceptor: &mut <Self::Config as endpoint::Config>::PacketInterceptor,
    ) -> Result<(), ConnectionOnTransmitError>
    where
        Tx: tx::Queue<Handle = <Self::Config as endpoint::Config>::PathHandle>;

    /// Handles all timeouts on the `Connection`.
    ///
    /// `timestamp` passes the current time.
    fn on_timeout(
        &mut self,
        connection_id_mapper: &mut ConnectionIdMapper,
        timestamp: Timestamp,
        supervisor_context: &supervisor::Context,
        random_generator: &mut <Self::Config as endpoint::Config>::RandomGenerator,
        subscriber: &mut <Self::Config as endpoint::Config>::EventSubscriber,
    ) -> Result<(), connection::Error>;

    /// Process pendings ACKs for the `Connection`.
    fn on_pending_ack_ranges(
        &mut self,
        random_generator: &mut <Self::Config as endpoint::Config>::RandomGenerator,
        timestamp: Timestamp,
        subscriber: &mut <Self::Config as endpoint::Config>::EventSubscriber,
    ) -> Result<(), connection::Error>;

    /// Handles all external wakeups on the [`Connection`].
    fn on_wakeup(
        &mut self,
        timestamp: Timestamp,
        subscriber: &mut <Self::Config as endpoint::Config>::EventSubscriber,
        datagram: &mut <Self::Config as endpoint::Config>::DatagramEndpoint,
    ) -> Result<(), connection::Error>;

    // Packet handling

    /// Is called when an initial packet had been received
    #[allow(clippy::too_many_arguments)]
    fn handle_initial_packet(
        &mut self,
        datagram: &DatagramInfo,
        path_id: path::Id,
        packet: ProtectedInitial,
        random_generator: &mut <Self::Config as endpoint::Config>::RandomGenerator,
        subscriber: &mut <Self::Config as endpoint::Config>::EventSubscriber,
        packet_interceptor: &mut <Self::Config as endpoint::Config>::PacketInterceptor,
        datagram_endpoint: &mut <Self::Config as endpoint::Config>::DatagramEndpoint,
    ) -> Result<(), ProcessingError>;

    /// Is called when an unprotected initial packet had been received
    #[allow(clippy::too_many_arguments)]
    fn handle_cleartext_initial_packet(
        &mut self,
        datagram: &DatagramInfo,
        path_id: path::Id,
        packet: CleartextInitial,
        random_generator: &mut <Self::Config as endpoint::Config>::RandomGenerator,
        subscriber: &mut <Self::Config as endpoint::Config>::EventSubscriber,
        packet_interceptor: &mut <Self::Config as endpoint::Config>::PacketInterceptor,
        datagram_endpoint: &mut <Self::Config as endpoint::Config>::DatagramEndpoint,
    ) -> Result<(), ProcessingError>;

    /// Is called when a handshake packet had been received
    #[allow(clippy::too_many_arguments)]
    fn handle_handshake_packet(
        &mut self,
        datagram: &DatagramInfo,
        path_id: path::Id,
        packet: ProtectedHandshake,
        random_generator: &mut <Self::Config as endpoint::Config>::RandomGenerator,
        subscriber: &mut <Self::Config as endpoint::Config>::EventSubscriber,
        packet_interceptor: &mut <Self::Config as endpoint::Config>::PacketInterceptor,
        datagram_endpoint: &mut <Self::Config as endpoint::Config>::DatagramEndpoint,
    ) -> Result<(), ProcessingError>;

    /// Is called when a short packet had been received
    fn handle_short_packet(
        &mut self,
        datagram: &DatagramInfo,
        path_id: path::Id,
        packet: ProtectedShort,
        random_generator: &mut <Self::Config as endpoint::Config>::RandomGenerator,
        subscriber: &mut <Self::Config as endpoint::Config>::EventSubscriber,
        packet_interceptor: &mut <Self::Config as endpoint::Config>::PacketInterceptor,
    ) -> Result<(), ProcessingError>;

    /// Is called when a version negotiation packet had been received
    fn handle_version_negotiation_packet(
        &mut self,
        datagram: &DatagramInfo,
        path_id: path::Id,
        packet: ProtectedVersionNegotiation,
        subscriber: &mut <Self::Config as endpoint::Config>::EventSubscriber,
        packet_interceptor: &mut <Self::Config as endpoint::Config>::PacketInterceptor,
    ) -> Result<(), ProcessingError>;

    /// Is called when a zero rtt packet had been received
    fn handle_zero_rtt_packet(
        &mut self,
        datagram: &DatagramInfo,
        path_id: path::Id,
        packet: ProtectedZeroRtt,
        subscriber: &mut <Self::Config as endpoint::Config>::EventSubscriber,
        packet_interceptor: &mut <Self::Config as endpoint::Config>::PacketInterceptor,
    ) -> Result<(), ProcessingError>;

    /// Is called when a retry packet had been received
    fn handle_retry_packet(
        &mut self,
        datagram: &DatagramInfo,
        path_id: path::Id,
        packet: ProtectedRetry,
        subscriber: &mut <Self::Config as endpoint::Config>::EventSubscriber,
        packet_interceptor: &mut <Self::Config as endpoint::Config>::PacketInterceptor,
    ) -> Result<(), ProcessingError>;

    /// Notifies a connection it has received a datagram from a peer
    #[allow(clippy::too_many_arguments)]
    fn on_datagram_received(
        &mut self,
        path_handle: &<Self::Config as endpoint::Config>::PathHandle,
        datagram: &DatagramInfo,
        congestion_controller_endpoint: &mut <Self::Config as endpoint::Config>::CongestionControllerEndpoint,
        migration_validator: &mut <Self::Config as endpoint::Config>::PathMigrationValidator,
        max_mtu: MaxMtu,
        subscriber: &mut <Self::Config as endpoint::Config>::EventSubscriber,
    ) -> Result<path::Id, DatagramDropReason>;

    /// Returns the Connections interests
    fn interests(&self) -> ConnectionInterests;

    /// Returns the QUIC version selected for the current connection
    fn quic_version(&self) -> u32;

    /// Handles reception of a single QUIC packet
    #[allow(clippy::too_many_arguments)]
    fn handle_packet(
        &mut self,
        datagram: &DatagramInfo,
        path_id: path::Id,
        packet: ProtectedPacket,
        random_generator: &mut <Self::Config as endpoint::Config>::RandomGenerator,
        subscriber: &mut <Self::Config as endpoint::Config>::EventSubscriber,
        packet_interceptor: &mut <Self::Config as endpoint::Config>::PacketInterceptor,
        datagram_endpoint: &mut <Self::Config as endpoint::Config>::DatagramEndpoint,
    ) -> Result<(), ProcessingError> {
        //= https://www.rfc-editor.org/rfc/rfc9000#section-5.2.1
        //# If a client receives a packet that uses a different version than it
        //# initially selected, it MUST discard that packet.
        if let Some(version) = packet.version() {
            if version != self.quic_version() {
                self.with_event_publisher(
                    datagram.timestamp,
                    Some(path_id),
                    subscriber,
                    |publisher, path| {
                        publisher.on_packet_dropped(event::builder::PacketDropped {
                            reason: event::builder::PacketDropReason::VersionMismatch {
                                version,
                                path: path_event!(path, path_id),
                            },
                        })
                    },
                );
                return Ok(());
            }
        }

        //= https://www.rfc-editor.org/rfc/rfc9001#section-4.1.4
        //# An endpoint SHOULD continue
        //# to respond to packets that can be processed during this time.
        // We make a best effort to process all of the packet spaces we have available. There isn't
        // any special logic required to meet this requirement as each packet is handled
        // independently.

        match packet {
            ProtectedPacket::Short(packet) => self.handle_short_packet(
                datagram,
                path_id,
                packet,
                random_generator,
                subscriber,
                packet_interceptor,
            ),
            ProtectedPacket::VersionNegotiation(packet) => self.handle_version_negotiation_packet(
                datagram,
                path_id,
                packet,
                subscriber,
                packet_interceptor,
            ),
            ProtectedPacket::Initial(packet) => self.handle_initial_packet(
                datagram,
                path_id,
                packet,
                random_generator,
                subscriber,
                packet_interceptor,
                datagram_endpoint,
            ),
            ProtectedPacket::ZeroRtt(packet) => self.handle_zero_rtt_packet(
                datagram,
                path_id,
                packet,
                subscriber,
                packet_interceptor,
            ),
            ProtectedPacket::Handshake(packet) => self.handle_handshake_packet(
                datagram,
                path_id,
                packet,
                random_generator,
                subscriber,
                packet_interceptor,
                datagram_endpoint,
            ),
            ProtectedPacket::Retry(packet) => {
                self.handle_retry_packet(datagram, path_id, packet, subscriber, packet_interceptor)
            }
        }
    }

    /// This is called to handle the remaining and yet undecoded packets inside
    /// a datagram.
    #[allow(clippy::too_many_arguments)]
    fn handle_remaining_packets(
        &mut self,
        path_handle: &<Self::Config as endpoint::Config>::PathHandle,
        datagram: &DatagramInfo,
        path_id: path::Id,
        connection_id_validator: &<Self::Config as endpoint::Config>::ConnectionIdFormat,
        mut payload: DecoderBufferMut,
        random_generator: &mut <Self::Config as endpoint::Config>::RandomGenerator,
        subscriber: &mut <Self::Config as endpoint::Config>::EventSubscriber,
        packet_interceptor: &mut <Self::Config as endpoint::Config>::PacketInterceptor,
        datagram_endpoint: &mut <Self::Config as endpoint::Config>::DatagramEndpoint,
    ) -> Result<(), connection::Error> {
        let remote_address = path_handle.remote_address();
        let connection_info = ConnectionInfo::new(&remote_address);

        while !payload.is_empty() {
            if let Ok((packet, remaining)) =
                ProtectedPacket::decode(payload, &connection_info, connection_id_validator)
            {
                payload = remaining;

                //= https://www.rfc-editor.org/rfc/rfc9000#section-12.2
                //# Senders MUST NOT coalesce QUIC packets
                //# with different connection IDs into a single UDP datagram.  Receivers
                //# SHOULD ignore any subsequent packets with a different Destination
                //# Connection ID than the first packet in the datagram.
                if datagram.destination_connection_id.as_bytes()
                    != packet.destination_connection_id()
                {
                    self.with_event_publisher(
                        datagram.timestamp,
                        Some(path_id),
                        subscriber,
                        |publisher, path| {
                            publisher.on_packet_dropped(event::builder::PacketDropped {
                                reason: event::builder::PacketDropReason::ConnectionIdMismatch {
                                    packet_cid: packet.destination_connection_id(),
                                    path: path_event!(path, path_id),
                                },
                            })
                        },
                    );
                    break;
                }

                let result = self.handle_packet(
                    datagram,
                    path_id,
                    packet,
                    random_generator,
                    subscriber,
                    packet_interceptor,
                    datagram_endpoint,
                );

                if let Err(ProcessingError::ConnectionError(err)) = result {
                    // CryptoErrors returned as a result of a packet failing decryption will be
                    // silently discarded, but this method could return an error on protocol
                    // violations which would result in shutting down the connection anyway. In this
                    // case this will return early without processing the remaining packets.
                    if !payload.is_empty() {
                        self.with_event_publisher(
                            datagram.timestamp,
                            Some(path_id),
                            subscriber,
                            |publisher, path| {
                                publisher.on_packet_dropped(event::builder::PacketDropped {
                                    reason: event::builder::PacketDropReason::ConnectionError {
                                        path: path_event!(path, path_id),
                                    },
                                })
                            },
                        );
                    }
                    return Err(err);
                }
            } else {
                //= https://www.rfc-editor.org/rfc/rfc9000#section-12.2
                //# Every QUIC packet that is coalesced into a single UDP datagram is
                //# separate and complete.  The receiver of coalesced QUIC packets MUST
                //# individually process each QUIC packet and separately acknowledge
                //# them, as if they were received as the payload of different UDP
                //# datagrams.  For example, if decryption fails (because the keys are
                //# not available or for any other reason), the receiver MAY either
                //# discard or buffer the packet for later processing and MUST attempt to
                //# process the remaining packets.
                //
                // We choose to discard the rest of the datagram on parsing errors since it
                // would be difficult to recover from an invalid packet.
                self.with_event_publisher(
                    datagram.timestamp,
                    Some(path_id),
                    subscriber,
                    |publisher, path| {
                        publisher.on_packet_dropped(event::builder::PacketDropped {
                            reason: event::builder::PacketDropReason::DecodingFailed {
                                path: path_event!(path, path_id),
                            },
                        })
                    },
                );

                break;
            }
        }

        Ok(())
    }

    fn poll_stream_request(
        &mut self,
        stream_id: stream::StreamId,
        request: &mut stream::ops::Request,
        context: Option<&Context>,
    ) -> Result<stream::ops::Response, stream::StreamError>;

    fn poll_accept_stream(
        &mut self,
        stream_type: Option<stream::StreamType>,
        context: &Context,
    ) -> Poll<Result<Option<stream::StreamId>, connection::Error>>;

    fn poll_open_stream(
        &mut self,
        stream_type: stream::StreamType,
        open_token: &mut connection::OpenToken,
        context: &Context,
    ) -> Poll<Result<stream::StreamId, connection::Error>>;

    fn application_close(&mut self, error: Option<application::Error>);

    fn server_name(&self) -> Option<ServerName>;

    fn application_protocol(&self) -> Bytes;

    fn ping(&mut self) -> Result<(), connection::Error>;

    fn keep_alive(&mut self, enabled: bool) -> Result<(), connection::Error>;

    fn local_address(&self) -> Result<SocketAddress, connection::Error>;

    fn remote_address(&self) -> Result<SocketAddress, connection::Error>;

    fn error(&self) -> Option<connection::Error>;

    fn query_event_context(&self, query: &mut dyn event::query::Query);

    fn query_event_context_mut(&mut self, query: &mut dyn event::query::QueryMut);

    fn with_event_publisher<F>(
        &mut self,
        timestamp: Timestamp,
        path_id: Option<path::Id>,
        subscriber: &mut <Self::Config as endpoint::Config>::EventSubscriber,
        f: F,
    ) where
        F: FnOnce(
            &mut event::ConnectionPublisherSubscriber<
                <Self::Config as endpoint::Config>::EventSubscriber,
            >,
            &path::Path<Self::Config>,
        );
}

/// A lock that synchronizes connection state between the QUIC endpoint thread and application
pub trait Lock<T>: 'static + Send + Sync {
    type Error;

    /// Creates a connection lock
    fn new(value: T) -> Self;

    /// Obtains a read-only reference to the inner connection
    fn read<F: FnOnce(&T) -> R, R>(&self, f: F) -> Result<R, Self::Error>;

    /// Obtains a mutable reference to the inner connection
    fn write<F: FnOnce(&mut T) -> R, R>(&self, f: F) -> Result<R, Self::Error>;
}

#[cfg(feature = "std")]
impl<T: 'static + Send> Lock<T> for std::sync::Mutex<T> {
    type Error = ();

    fn new(value: T) -> Self {
        std::sync::Mutex::new(value)
    }

    fn read<F: FnOnce(&T) -> R, R>(&self, f: F) -> Result<R, Self::Error> {
        let lock = self.lock().map_err(|_| ())?;
        let result = f(&*lock);
        Ok(result)
    }

    fn write<F: FnOnce(&mut T) -> R, R>(&self, f: F) -> Result<R, Self::Error> {
        let mut lock = self.lock().map_err(|_| ())?;
        let result = f(&mut *lock);
        Ok(result)
    }
}

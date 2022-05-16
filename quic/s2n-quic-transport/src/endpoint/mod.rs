// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! This module defines a QUIC endpoint

use crate::{
    connection::{
        self,
        limits::{ConnectionInfo as LimitsInfo, Limiter as _},
        ConnectionContainer, ConnectionContainerIterationResult, ConnectionIdMapper,
        InternalConnectionId, InternalConnectionIdGenerator, ProcessingError, Trait as _,
    },
    endpoint,
    endpoint::close::CloseHandle,
    recovery::congestion_controller::{self, Endpoint as _},
    space::PacketSpaceManager,
    wakeup_queue::WakeupQueue,
};
use alloc::collections::VecDeque;
use core::{
    convert::TryInto,
    task::{self, Poll},
};
use s2n_codec::{DecoderBuffer, DecoderBufferMut};
use s2n_quic_core::{
    connection::{
        id::{ConnectionInfo, Generator},
        InitialId, LocalId, PeerId,
    },
    crypto::{tls, tls::Endpoint as _, CryptoSuite, InitialKey},
    endpoint::{limits::Outcome, Limiter as _},
    event::{
        self, supervisor, ConnectionPublisher, EndpointPublisher as _, IntoEvent, Subscriber as _,
    },
    inet::{datagram, DatagramInfo},
    io::{rx, tx},
    packet::{initial::ProtectedInitial, ProtectedPacket},
    path,
    path::{Handle as _, MaxMtu},
    random::Generator as _,
    stateless_reset::token::{Generator as _, LEN as StatelessResetTokenLen},
    time::{Clock, Timestamp},
    token::{self, Format},
    transport::parameters::ClientTransportParameters,
};

pub mod close;
mod config;
pub mod connect;
pub mod handle;
mod initial;
mod packet_buffer;
mod retry;
mod stateless_reset;
mod version;

// exports
pub use config::{Config, Context};
pub use packet_buffer::Buffer as PacketBuffer;
pub use s2n_quic_core::endpoint::*;

const DEFAULT_MAX_PEERS: usize = 1024;

/// A QUIC `Endpoint`
pub struct Endpoint<Cfg: Config> {
    /// Configuration parameters for the endpoint
    config: Cfg,
    /// Contains all active connections
    connections: ConnectionContainer<Cfg::Connection, Cfg::ConnectionLock>,
    /// Creates internal IDs for new connections
    connection_id_generator: InternalConnectionIdGenerator,
    /// Maps from external to internal connection IDs
    connection_id_mapper: ConnectionIdMapper,
    /// Allows to wakeup the endpoint task which might be blocked on waiting for packets
    /// from application tasks (which e.g. enqueued new data to send).
    wakeup_queue: WakeupQueue<InternalConnectionId>,
    /// Used to receive close attempts and track close state.
    close_handle: CloseHandle,
    /// This queue contains wakeups we retrieved from the [`Self::wakeup_queue`] earlier.
    /// This is not a local variable in order to reuse the allocated queue capacity in between
    /// [`Endpoint`] interactions.
    dequeued_wakeups: VecDeque<InternalConnectionId>,
    version_negotiator: version::Negotiator<Cfg>,
    retry_dispatch: retry::Dispatch<Cfg::PathHandle>,
    stateless_reset_dispatch: stateless_reset::Dispatch<Cfg::PathHandle>,
    close_packet_buffer: packet_buffer::Buffer,
    /// The largest maximum transmission unit (MTU) that can be sent on a path
    max_mtu: MaxMtu,
}

impl<Cfg: Config> s2n_quic_core::endpoint::Endpoint for Endpoint<Cfg> {
    type PathHandle = Cfg::PathHandle;
    type Subscriber = Cfg::EventSubscriber;

    const ENDPOINT_TYPE: s2n_quic_core::endpoint::Type = Cfg::ENDPOINT_TYPE;

    fn receive<Rx, C>(&mut self, queue: &mut Rx, clock: &C)
    where
        Rx: rx::Queue<Handle = Cfg::PathHandle>,
        C: Clock,
    {
        use rx::Entry;

        let local_address = queue.local_address();
        let entries = queue.as_slice_mut();
        let mut now: Option<Timestamp> = None;

        for entry in entries.iter_mut() {
            let timestamp = match now {
                Some(time) => time,
                None => {
                    now = Some(clock.get_time());
                    now.expect("value set above")
                }
            };

            if let Some((header, payload)) = entry.read(&local_address) {
                self.receive_datagram(&header, payload, timestamp)
            }
        }

        let endpoint_context = self.config.context();
        let close_packet_buffer = &mut self.close_packet_buffer;
        // process ACKs on Connections with interest
        self.connections.iterate_ack_list(|connection| {
            let timestamp = match now {
                Some(time) => time,
                None => {
                    now = Some(clock.get_time());
                    now.expect("value set above")
                }
            };

            // handle error and close the connection
            if let Err(error) =
                connection.on_pending_ack_ranges(timestamp, endpoint_context.event_subscriber)
            {
                connection.close(
                    error,
                    endpoint_context.connection_close_formatter,
                    close_packet_buffer,
                    timestamp,
                    endpoint_context.event_subscriber,
                    endpoint_context.packet_interceptor,
                );
            }
        });

        let len = entries.len();
        queue.finish(len);
    }

    fn transmit<Tx, C>(&mut self, queue: &mut Tx, clock: &C)
    where
        Tx: tx::Queue<Handle = Self::PathHandle>,
        C: Clock,
    {
        self.on_timeout(clock.get_time());

        // Iterate over all connections which want to transmit data
        let mut transmit_result = Ok(());
        let endpoint_context = self.config.context();

        let timestamp = clock.get_time();

        self.connections.iterate_transmission_list(|connection| {
            transmit_result = connection.on_transmit(
                queue,
                timestamp,
                endpoint_context.event_subscriber,
                endpoint_context.packet_interceptor,
            );
            if transmit_result.is_err() {
                // If one connection fails, return
                ConnectionContainerIterationResult::BreakAndInsertAtBack
            } else {
                ConnectionContainerIterationResult::Continue
            }
        });

        if transmit_result.is_ok() {
            let mut publisher = event::EndpointPublisherSubscriber::new(
                event::builder::EndpointMeta {
                    endpoint_type: Cfg::ENDPOINT_TYPE,
                    timestamp,
                },
                None,
                endpoint_context.event_subscriber,
            );
            self.version_negotiator.on_transmit(queue, &mut publisher);
            self.retry_dispatch.on_transmit(queue, &mut publisher);
            self.stateless_reset_dispatch
                .on_transmit(queue, &mut publisher);
        }
    }

    fn poll_wakeups<C: Clock>(
        &mut self,
        cx: &mut task::Context<'_>,
        clock: &C,
    ) -> Poll<Result<usize, s2n_quic_core::endpoint::CloseError>> {
        if self.close_handle.poll_interest().is_ready() // poll for close interest
            && self.connections.is_empty() // wait for all connections to close gracefully
            && self.connections.is_open()
        {
            // transition to close state
            self.close_handle.close();

            // stop accepting new connections and prepare to close the endpoint
            self.connections.close();
        }

        // Drop the endpoint if there is no more progress to be made.
        if !self.connections.is_open() {
            return Poll::Ready(Err(s2n_quic_core::endpoint::CloseError));
        }

        self.wakeup_queue
            .poll_pending_wakeups(&mut self.dequeued_wakeups, cx);

        let mut now: Option<Timestamp> = None;
        let mut wakeup_count = self.dequeued_wakeups.len();
        let close_packet_buffer = &mut self.close_packet_buffer;
        let endpoint_context = self.config.context();

        for internal_id in self.dequeued_wakeups.drain(..) {
            self.connections.with_connection(internal_id, |conn| {
                let timestamp = match now {
                    Some(now) => now,
                    _ => {
                        let time = clock.get_time();
                        now = Some(time);
                        time
                    }
                };

                if let Err(error) = conn.on_wakeup(
                    timestamp,
                    endpoint_context.event_subscriber,
                    endpoint_context.datagram,
                ) {
                    conn.close(
                        error,
                        endpoint_context.connection_close_formatter,
                        close_packet_buffer,
                        timestamp,
                        endpoint_context.event_subscriber,
                        endpoint_context.packet_interceptor,
                    );
                }
            });
        }

        // try to open connection requests from the application
        if Cfg::ENDPOINT_TYPE.is_client() {
            loop {
                match self.connections.poll_connection_request(cx) {
                    Poll::Pending => break,
                    Poll::Ready(Some(request)) => {
                        wakeup_count += 1;

                        let time = clock.get_time();
                        if let Err(err) = self.create_client_connection(request, time) {
                            // TODO report that the connection was not successfully created
                            // TODO emit event
                            dbg!(err);
                        }
                    }
                    Poll::Ready(None) => {
                        // the client handle has been dropped so break from loop
                        break;
                    }
                }
            }
        }

        if wakeup_count > 0 {
            Poll::Ready(Ok(wakeup_count))
        } else {
            Poll::Pending
        }
    }

    #[inline]
    fn timeout(&self) -> Option<Timestamp> {
        self.connections.next_expiration()
    }

    #[inline]
    fn set_max_mtu(&mut self, max_mtu: MaxMtu) {
        self.max_mtu = max_mtu
    }

    #[inline]
    fn subscriber(&mut self) -> &mut Self::Subscriber {
        self.config.context().event_subscriber
    }
}

impl<Cfg: Config> Endpoint<Cfg> {
    /// Creates a new QUIC server endpoint using the given configuration
    pub fn new_server(config: Cfg) -> (Self, handle::Acceptor) {
        assert!(
            Cfg::ENDPOINT_TYPE.is_server(),
            "only server endpoints can be created with server configurations"
        );
        let (endpoint, handle) = Self::new(config);
        (endpoint, handle.acceptor)
    }

    /// Creates a new QUIC client endpoint using the given configuration
    pub fn new_client(config: Cfg) -> (Self, handle::Connector) {
        assert!(
            Cfg::ENDPOINT_TYPE.is_client(),
            "only client endpoints can be created with client configurations"
        );
        let (endpoint, handle) = Self::new(config);
        (endpoint, handle.connector)
    }

    fn new(mut config: Cfg) -> (Self, handle::Handle) {
        // TODO make this limit configurable
        let max_opening_connections = 1000;
        let (handle, acceptor_sender, connector_receiver, close_handle) =
            handle::Handle::new(max_opening_connections);

        let connection_id_mapper =
            ConnectionIdMapper::new(config.context().random_generator, Cfg::ENDPOINT_TYPE);

        let endpoint = Self {
            config,
            connections: ConnectionContainer::new(acceptor_sender, connector_receiver),
            connection_id_generator: InternalConnectionIdGenerator::new(),
            connection_id_mapper,
            wakeup_queue: WakeupQueue::new(),
            close_handle,
            dequeued_wakeups: VecDeque::new(),
            version_negotiator: version::Negotiator::default(),
            retry_dispatch: retry::Dispatch::default(),
            stateless_reset_dispatch: stateless_reset::Dispatch::default(),
            close_packet_buffer: Default::default(),
            max_mtu: Default::default(),
        };

        (endpoint, handle)
    }

    /// Determine the next step when a peer attempts a connection
    fn connection_allowed(
        &mut self,
        header: &datagram::Header<Cfg::PathHandle>,
        packet: &ProtectedInitial,
        payload_len: usize,
        timestamp: Timestamp,
    ) -> Option<()> {
        if !self.connections.can_accept() {
            return None;
        }

        let remote_address = header.path.remote_address();

        let attempt = s2n_quic_core::endpoint::limits::ConnectionAttempt::new(
            self.connections.handshake_connections(),
            self.connections.len(),
            &remote_address,
            timestamp.into_event(),
        );

        let context = self.config.context();
        let outcome = context.endpoint_limits.on_connection_attempt(&attempt);
        let mut publisher = event::EndpointPublisherSubscriber::new(
            event::builder::EndpointMeta {
                endpoint_type: Cfg::ENDPOINT_TYPE,
                timestamp,
            },
            None,
            context.event_subscriber,
        );

        match outcome {
            Outcome::Allow { .. } => Some(()),
            Outcome::Retry { .. } => {
                //= https://www.rfc-editor.org/rfc/rfc9000#section-8.1.2
                //# A server can also use a Retry packet to defer the state and
                //# processing costs of connection establishment.  Requiring the server
                //# to provide a different connection ID, along with the
                //# original_destination_connection_id transport parameter defined in
                //# Section 18.2, forces the server to demonstrate that it, or an entity
                //# it cooperates with, received the original Initial packet from the
                //# client.

                let connection_info = ConnectionInfo::new(&remote_address);

                let local_connection_id = context.connection_id_format.generate(&connection_info);

                self.retry_dispatch.queue::<
                    _,
                    <<<Cfg as Config>::TLSEndpoint as tls::Endpoint>::Session as CryptoSuite>::RetryKey,
                    _,
                >(
                    header.path,
                    packet,
                    local_connection_id,
                    context.random_generator,
                    context.token
                );

                None
            }
            Outcome::Close { .. } => {
                //= https://www.rfc-editor.org/rfc/rfc9000#section-5.2.2
                //= type=TODO
                //= tracking-issue=270
                //# If a server refuses to accept a new connection, it SHOULD send an
                //# Initial packet containing a CONNECTION_CLOSE frame with error code
                //# CONNECTION_REFUSED.

                publisher.on_endpoint_datagram_dropped(event::builder::EndpointDatagramDropped {
                    len: payload_len as u16,
                    reason: event::builder::DatagramDropReason::RejectedConnectionAttempt,
                });

                None
            }
            Outcome::Drop { .. } => {
                publisher.on_endpoint_datagram_dropped(event::builder::EndpointDatagramDropped {
                    len: payload_len as u16,
                    reason: event::builder::DatagramDropReason::RejectedConnectionAttempt,
                });
                None
            }
            _ => {
                publisher.on_endpoint_datagram_dropped(event::builder::EndpointDatagramDropped {
                    len: payload_len as u16,
                    reason: event::builder::DatagramDropReason::RejectedConnectionAttempt,
                });
                // Outcome is non_exhaustive so drop on things we don't understand
                None
            }
        }
    }

    /// Ingests a single datagram
    fn receive_datagram(
        &mut self,
        header: &datagram::Header<Cfg::PathHandle>,
        payload: &mut [u8],
        timestamp: Timestamp,
    ) {
        let endpoint_context = self.config.context();

        let remote_address = header.path.remote_address();

        // Try to decode the first packet in the datagram
        let payload_len = payload.len();
        let buffer = DecoderBufferMut::new(payload);
        let connection_info = ConnectionInfo::new(&remote_address);
        let (packet, remaining) = if let Ok((packet, remaining)) = ProtectedPacket::decode(
            buffer,
            &connection_info,
            endpoint_context.connection_id_format,
        ) {
            (packet, remaining)
        } else {
            //= https://www.rfc-editor.org/rfc/rfc9000#section-5.2.2
            //# Servers MUST drop incoming packets under all other circumstances.

            //= https://www.rfc-editor.org/rfc/rfc9000#section-10.3
            //# However, endpoints MUST treat any packet ending in a
            //# valid stateless reset token as a Stateless Reset, as other QUIC
            //# versions might allow the use of a long header.

            // The packet may be a stateless reset, check before returning.
            let internal_connection_id = self.close_on_matching_stateless_reset(payload, timestamp);

            if internal_connection_id.is_none() {
                // The packet didn't contain a valid stateless token
                let mut publisher = event::EndpointPublisherSubscriber::new(
                    event::builder::EndpointMeta {
                        endpoint_type: Cfg::ENDPOINT_TYPE,
                        timestamp,
                    },
                    None,
                    self.config.context().event_subscriber,
                );
                publisher.on_endpoint_datagram_dropped(event::builder::EndpointDatagramDropped {
                    len: payload_len as u16,
                    reason: event::builder::DatagramDropReason::DecodingFailed,
                });
            }

            return;
        };

        let mut publisher = event::EndpointPublisherSubscriber::new(
            event::builder::EndpointMeta {
                endpoint_type: Cfg::ENDPOINT_TYPE,
                timestamp,
            },
            packet.version(),
            endpoint_context.event_subscriber,
        );

        // Ensure the version is supported. This check occurs before the destination
        // connection ID is parsed since future versions of QUIC could have different
        // length requirements for connection IDs.
        if self
            .version_negotiator
            .on_packet(&header.path, payload_len, &packet, &mut publisher)
            .is_err()
        {
            publisher.on_endpoint_datagram_dropped(event::builder::EndpointDatagramDropped {
                len: payload_len as u16,
                reason: event::builder::DatagramDropReason::UnsupportedVersion,
            });
            return;
        }

        let destination_connection_id =
            match connection::LocalId::try_from_bytes(packet.destination_connection_id()) {
                Some(connection_id) => connection_id,
                None => {
                    // Ignore the datagram
                    publisher.on_endpoint_datagram_dropped(
                        event::builder::EndpointDatagramDropped {
                            len: payload_len as u16,
                            reason:
                                event::builder::DatagramDropReason::InvalidDestinationConnectionId,
                        },
                    );
                    return;
                }
            };

        let source_connection_id = packet
            .source_connection_id()
            .and_then(PeerId::try_from_bytes);

        let datagram = &DatagramInfo {
            timestamp,
            payload_len,
            ecn: header.ecn,
            destination_connection_id,
            source_connection_id,
        };

        // TODO validate the connection ID before looking up the connection in the map
        let close_packet_buffer = &mut self.close_packet_buffer;

        // Try to lookup the internal connection ID and dispatch the packet
        // to the Connection
        if let Some(internal_id) = self
            .connection_id_mapper
            .lookup_internal_connection_id(&datagram.destination_connection_id)
        {
            let mut check_for_stateless_reset = false;
            let max_mtu = self.max_mtu;

            let _ = self.connections.with_connection(internal_id, |conn| {
                // The path `Id` needs to be passed around instead of the path to get around `&mut self` and
                // `&mut self.path_manager` being borrowed at the same time
                let path_id = conn
                    .on_datagram_received(
                        &header.path,
                        datagram,
                        endpoint_context.congestion_controller,
                        endpoint_context.path_migration,
                        max_mtu,
                        endpoint_context.event_subscriber,
                    )
                    .map_err(|datagram_drop_reason| {
                        // An error received at this point was caused by a datagram that has not
                        // been authenticated yet, and thus the connection should not be closed.
                        conn.with_event_publisher(
                            datagram.timestamp,
                            None,
                            endpoint_context.event_subscriber,
                            |publisher, _path| {
                                publisher.on_datagram_dropped(event::builder::DatagramDropped {
                                    len: datagram.payload_len as u16,
                                    reason: datagram_drop_reason,
                                });
                            },
                        );
                    })?;

                //= https://www.rfc-editor.org/rfc/rfc9000#section-10.2.1
                //# An endpoint
                //# that is closing is not required to process any received frame.

                if let Err(err) = conn.handle_packet(
                    datagram,
                    path_id,
                    packet,
                    endpoint_context.random_generator,
                    endpoint_context.event_subscriber,
                    endpoint_context.packet_interceptor,
                    endpoint_context.datagram,
                ) {
                    match err {
                        ProcessingError::DuplicatePacket => {
                            // We discard duplicate packets
                        }
                        ProcessingError::NonEmptyRetryToken => {
                            //= https://www.rfc-editor.org/rfc/rfc9000#section-17.2.2
                            //# Initial packets sent by the server MUST set the Token Length field
                            //# to 0; clients that receive an Initial packet with a non-zero Token
                            //# Length field MUST either discard the packet or generate a
                            //# connection error of type PROTOCOL_VIOLATION.
                            //
                            // We discard server initials with non empty retry tokens instead of closing
                            // the connection to prevent an attacker that can spoof initial packets
                            // from gaining the ability to close a connection by setting a retry token.
                        }
                        ProcessingError::RetryScidEqualsDcid => {
                            //= https://www.rfc-editor.org/rfc/rfc9000#section-17.2.5.1
                            //# A client MUST
                            //# discard a Retry packet that contains a Source Connection ID field
                            //# that is identical to the Destination Connection ID field of its
                            //# Initial packet.
                        }
                        ProcessingError::ConnectionError(err) => {
                            conn.close(
                                err,
                                endpoint_context.connection_close_formatter,
                                close_packet_buffer,
                                datagram.timestamp,
                                endpoint_context.event_subscriber,
                                endpoint_context.packet_interceptor,
                            );
                            return Err(());
                        }
                        ProcessingError::CryptoError(_) => {
                            // CryptoErrors returned as a result of a packet failing decryption
                            // will be silently discarded, but are a potential indication of a
                            // stateless reset from the peer

                            //= https://www.rfc-editor.org/rfc/rfc9000#section-5.2.1
                            //# Due to packet reordering or loss, a client might receive packets for
                            //# a connection that are encrypted with a key it has not yet computed.
                            //# The client MAY drop these packets, or it MAY buffer them in
                            //# anticipation of later packets that allow it to compute the key.
                            //
                            // Packets that fail decryption are discarded rather than buffered.

                            //= https://www.rfc-editor.org/rfc/rfc9000#section-10.3.1
                            //# Endpoints MAY skip this check if any packet from a datagram is
                            //# successfully processed.  However, the comparison MUST be performed
                            //# when the first packet in an incoming datagram either cannot be
                            //# associated with a connection, or cannot be decrypted.
                            check_for_stateless_reset = true;
                        }
                    }
                }

                if let Err(err) = conn.handle_remaining_packets(
                    &header.path,
                    datagram,
                    path_id,
                    endpoint_context.connection_id_format,
                    remaining,
                    endpoint_context.random_generator,
                    endpoint_context.event_subscriber,
                    endpoint_context.packet_interceptor,
                    endpoint_context.datagram,
                ) {
                    conn.close(
                        err,
                        endpoint_context.connection_close_formatter,
                        close_packet_buffer,
                        datagram.timestamp,
                        endpoint_context.event_subscriber,
                        endpoint_context.packet_interceptor,
                    );
                    return Err(());
                }

                Ok(())
            });

            if check_for_stateless_reset {
                self.close_on_matching_stateless_reset(payload, timestamp);
            }

            return;
        }

        match (Cfg::ENDPOINT_TYPE, packet) {
            (s2n_quic_core::endpoint::Type::Server, ProtectedPacket::Initial(packet)) => {
                let source_connection_id =
                    match connection::PeerId::try_from_bytes(packet.source_connection_id()) {
                        Some(connection_id) => connection_id,
                        None => {
                            publisher.on_endpoint_datagram_dropped(
                           event::builder::EndpointDatagramDropped {
                               len: payload_len as u16,
                               reason:
                                   event::builder::DatagramDropReason::InvalidSourceConnectionId,
                           },
                       );
                            return;
                        }
                    };

                //= https://www.rfc-editor.org/rfc/rfc9000#section-8.1
                //= type=TODO
                //= tracking-issue=140
                //# Additionally, an endpoint MAY consider the peer address validated if
                //# the peer uses a connection ID chosen by the endpoint and the
                //# connection ID contains at least 64 bits of entropy

                //= https://www.rfc-editor.org/rfc/rfc9000#section-8.1.2
                //# In response to processing an Initial packet containing a token that
                //# was provided in a Retry packet, a server cannot send another Retry
                //# packet; it can only refuse the connection or permit it to proceed.
                let retry_token_dcid = if !packet.token().is_empty() {
                    let mut context = token::Context::new(
                        &remote_address,
                        &source_connection_id,
                        endpoint_context.random_generator,
                    );

                    let outcome = endpoint_context
                        .token
                        .validate_token(&mut context, packet.token());

                    if outcome.is_none() {
                        //= https://www.rfc-editor.org/rfc/rfc9000#section-8.1.3
                        //= type=TODO
                        //= tracking-issue=344
                        //# If the token is invalid, then the
                        //# server SHOULD proceed as if the client did not have a validated
                        //# address, including potentially sending a Retry packet.

                        //= https://www.rfc-editor.org/rfc/rfc9000#section-8.1.2
                        //= type=TODO
                        //= tracking-issue=344
                        //# Instead, the
                        //# server SHOULD immediately close (Section 10.2) the connection with an
                        //# INVALID_TOKEN error.
                        publisher.on_endpoint_datagram_dropped(
                            event::builder::EndpointDatagramDropped {
                                len: payload_len as u16,
                                reason: event::builder::DatagramDropReason::InvalidRetryToken,
                            },
                        );

                        //= https://www.rfc-editor.org/rfc/rfc9000#section-8.1.3
                        //# Servers MAY
                        //# discard any Initial packet that does not carry the expected token.
                        return;
                    }

                    //= https://www.rfc-editor.org/rfc/rfc9000#section-8.1.3
                    //# If the validation succeeds, the server SHOULD then allow
                    //# the handshake to proceed.
                    outcome
                } else {
                    //= https://www.rfc-editor.org/rfc/rfc9000#section-8.1.2
                    //# Upon receiving the client's Initial packet, the server can request
                    //# address validation by sending a Retry packet (Section 17.2.5)
                    //# containing a token.
                    if self
                        .connection_allowed(header, &packet, payload_len, timestamp)
                        .is_none()
                    {
                        //= https://www.rfc-editor.org/rfc/rfc9000#section-17.2.5.1
                        //# A server MUST NOT send more than one Retry
                        //# packet in response to a single UDP datagram.
                        return;
                    }

                    None
                };

                if let Err(err) = self.handle_initial_packet(
                    header,
                    datagram,
                    packet,
                    remaining,
                    retry_token_dcid,
                ) {
                    // TODO send a minimal connection close frame
                    let mut publisher = event::EndpointPublisherSubscriber::new(
                        event::builder::EndpointMeta {
                            endpoint_type: Cfg::ENDPOINT_TYPE,
                            timestamp,
                        },
                        None,
                        self.config.context().event_subscriber,
                    );
                    publisher.on_endpoint_connection_attempt_failed(
                        event::builder::EndpointConnectionAttemptFailed { error: err },
                    );
                }
            }
            (_, packet) => {
                publisher.on_endpoint_datagram_dropped(event::builder::EndpointDatagramDropped {
                    len: payload_len as u16,
                    reason: event::builder::DatagramDropReason::UnknownDestinationConnectionId,
                });

                let is_short_header_packet = matches!(packet, ProtectedPacket::Short(_));
                //= https://www.rfc-editor.org/rfc/rfc9000#section-10.3.1
                //# Endpoints MAY skip this check if any packet from a datagram is
                //# successfully processed.  However, the comparison MUST be performed
                //# when the first packet in an incoming datagram either cannot be
                //# associated with a connection, or cannot be decrypted.

                //= https://www.rfc-editor.org/rfc/rfc9000#section-10.3
                //# However, endpoints MUST treat any packet ending in a
                //# valid stateless reset token as a Stateless Reset, as other QUIC
                //# versions might allow the use of a long header.
                let is_stateless_reset = self
                    .close_on_matching_stateless_reset(payload, timestamp)
                    .is_some();

                //= https://www.rfc-editor.org/rfc/rfc9000#section-9.3.2
                //# For instance, an endpoint MAY send a Stateless Reset in
                //# response to any further incoming packets.

                //= https://www.rfc-editor.org/rfc/rfc9000#section-10.3
                //# An endpoint MAY send a Stateless Reset in response to receiving a packet
                //# that it cannot associate with an active connection.

                //= https://www.rfc-editor.org/rfc/rfc9000#section-10.3
                //# Because the stateless reset token is not available
                //# until connection establishment is complete or near completion,
                //# ignoring an unknown packet with a long header might be as effective
                //# as sending a Stateless Reset.
                if !is_stateless_reset
                    && Cfg::StatelessResetTokenGenerator::ENABLED
                    && is_short_header_packet
                {
                    self.enqueue_stateless_reset(header, datagram, &destination_connection_id);
                }
            }
        }
    }

    /// Enqueues sending a stateless reset to a peer.
    ///
    /// Sending the reset was caused through the passed `datagram`.
    fn enqueue_stateless_reset(
        &mut self,
        header: &datagram::Header<Cfg::PathHandle>,
        datagram: &DatagramInfo,
        destination_connection_id: &LocalId,
    ) {
        let token = self
            .config
            .context()
            .stateless_reset_token_generator
            .generate(destination_connection_id.as_bytes());
        let max_tag_length = self.config.context().tls.max_tag_length();
        // The datagram payload length is used as the packet length since
        // a stateless reset is only sent if the first packet in a datagram is
        // a short header packet and a short header packet must be the last packet
        // in a datagram; thus the entire datagram is one packet.
        let triggering_packet_len = datagram.payload_len;
        self.stateless_reset_dispatch.queue(
            header.path,
            token,
            max_tag_length,
            triggering_packet_len,
            self.config.context().random_generator,
        );
    }

    /// Checks if the given payload contains a stateless reset token matching a known token.
    /// If there is a match, the matching connection will be closed and the `InternalConnectionId`
    /// will be returned.
    fn close_on_matching_stateless_reset(
        &mut self,
        payload: &[u8],
        timestamp: Timestamp,
    ) -> Option<InternalConnectionId> {
        let buffer = DecoderBuffer::new(payload);

        //= https://www.rfc-editor.org/rfc/rfc9000#section-10.3.1
        //# The endpoint
        //# identifies a received datagram as a Stateless Reset by comparing the
        //# last 16 bytes of the datagram with all stateless reset tokens
        //# associated with the remote address on which the datagram was
        //# received.
        let token_index = payload.len().checked_sub(StatelessResetTokenLen)?;
        let buffer = buffer.skip(token_index).ok()?;
        let (token, _) = buffer.decode().ok()?;
        let endpoint_context = self.config.context();
        let internal_id = self
            .connection_id_mapper
            .remove_internal_connection_id_by_stateless_reset_token(&token)?;

        let mut publisher = event::EndpointPublisherSubscriber::new(
            event::builder::EndpointMeta {
                endpoint_type: Cfg::ENDPOINT_TYPE,
                timestamp,
            },
            None,
            endpoint_context.event_subscriber,
        );

        publisher.on_endpoint_packet_received(event::builder::EndpointPacketReceived {
            packet_header: event::builder::PacketHeader::StatelessReset {},
        });

        let close_packet_buffer = &mut self.close_packet_buffer;

        //= https://www.rfc-editor.org/rfc/rfc9000#section-10.3.1
        //# If the last 16 bytes of the datagram are identical in value to a
        //# stateless reset token, the endpoint MUST enter the draining period
        //# and not send any further packets on this connection.
        self.connections.with_connection(internal_id, |conn| {
            conn.close(
                connection::Error::stateless_reset(),
                endpoint_context.connection_close_formatter,
                close_packet_buffer,
                timestamp,
                endpoint_context.event_subscriber,
                endpoint_context.packet_interceptor,
            );
        });

        Some(internal_id)
    }

    fn on_timeout(&mut self, timestamp: Timestamp) {
        let connection_id_mapper = &mut self.connection_id_mapper;
        let close_packet_buffer = &mut self.close_packet_buffer;
        let endpoint_context = self.config.context();

        self.connections
            .iterate_timeout_list(timestamp, |conn, supervisor_context| {
                if let Err(error) = conn.on_timeout(
                    connection_id_mapper,
                    timestamp,
                    supervisor_context,
                    endpoint_context.random_generator,
                    endpoint_context.event_subscriber,
                ) {
                    conn.close(
                        error,
                        endpoint_context.connection_close_formatter,
                        close_packet_buffer,
                        timestamp,
                        endpoint_context.event_subscriber,
                        endpoint_context.packet_interceptor,
                    );
                }
            });

        // allow connections to generate a new connection id
        self.connections
            .iterate_new_connection_id_list(|connection| {
                let result = connection.on_new_connection_id(
                    endpoint_context.connection_id_format,
                    endpoint_context.stateless_reset_token_generator,
                    timestamp,
                );
                if result.is_ok() {
                    ConnectionContainerIterationResult::Continue
                } else {
                    // The provided Connection ID generator must never generate the same connection
                    // ID twice. If this happens, it is unlikely we could recover from it.
                    panic!("Generated connection ID was already in use");
                }
            });
    }

    fn create_client_connection(
        &mut self,
        request: endpoint::connect::Request,
        timestamp: Timestamp,
    ) -> Result<(), connection::Error> {
        let endpoint::connect::Request {
            connect:
                endpoint::connect::Connect {
                    remote_address,
                    server_name: hostname,
                },
            sender,
        } = request;

        let internal_connection_id = self.connection_id_generator.generate_id();
        let local_connection_id = self
            .config
            .context()
            .connection_id_format
            .generate(&ConnectionInfo::new(&remote_address));

        let local_connection_id_expiration_time = self
            .config
            .context()
            .connection_id_format
            .lifetime()
            .map(|duration| timestamp + duration);

        let local_id_registry = {
            // TODO: the client currently generates a random stateless_reset_token but doesnt
            // transmit it. Refactor `create_local_id_registry` to instead accept None for
            // stateless_reset_token.
            let stateless_reset_token = self
                .config
                .context()
                .stateless_reset_token_generator
                .generate(local_connection_id.as_bytes());
            self.connection_id_mapper.create_local_id_registry(
                internal_connection_id,
                &local_connection_id,
                local_connection_id_expiration_time,
                stateless_reset_token,
            )
        };

        let endpoint_context = self.config.context();

        //= https://www.rfc-editor.org/rfc/rfc9000#section-7.2
        //# When an Initial packet is sent by a client that has not previously
        //# received an Initial or Retry packet from the server, the client
        //# populates the Destination Connection ID field with an unpredictable
        //# value.
        let original_destination_connection_id = {
            let mut data = [0u8; InitialId::MIN_LEN];
            endpoint_context
                .random_generator
                .public_random_fill(&mut data);
            InitialId::try_from_bytes(&data).expect("InitialId creation failed.")
        };

        //= https://www.rfc-editor.org/rfc/rfc9000#section-10.3
        //# Note that clients cannot use the
        //# stateless_reset_token transport parameter because their transport
        //# parameters do not have confidentiality protection.
        //
        // The original_destination_connection_id is a random value used to establish the
        // connection. Since the connection is not yet secured, the client must not set a
        // stateless_reset_token.
        let peer_id_registry = self
            .connection_id_mapper
            .create_client_peer_id_registry(internal_connection_id);

        let congestion_controller = {
            let path_info = congestion_controller::PathInfo::new(&remote_address);
            endpoint_context
                .congestion_controller
                .new_congestion_controller(path_info)
        };

        //= https://www.rfc-editor.org/rfc/rfc9000#section-15
        //# This version of the specification is identified by the number
        //# 0x00000001.
        let quic_version = 0x00000001;

        let meta = event::builder::ConnectionMeta {
            endpoint_type: Cfg::ENDPOINT_TYPE,
            id: internal_connection_id.into(),
            timestamp,
        };
        let supervisor_context = supervisor::Context::new(
            self.connections.handshake_connections(),
            self.connections.len(),
            &remote_address,
            true,
        );
        let mut event_context = endpoint_context.event_subscriber.create_connection_context(
            &meta.clone().into_event(),
            &event::builder::ConnectionInfo {}.into_event(),
        );
        let mut publisher = event::ConnectionPublisherSubscriber::new(
            meta,
            quic_version,
            endpoint_context.event_subscriber,
            &mut event_context,
        );

        let mut transport_parameters = ClientTransportParameters {
            initial_source_connection_id: Some(local_connection_id.into()),
            ..Default::default()
        };
        let limits = endpoint_context
            .connection_limits
            .on_connection(&LimitsInfo::new(&remote_address));
        transport_parameters.load_limits(&limits);

        transport_parameters.active_connection_id_limit = s2n_quic_core::varint::VarInt::from(
            connection::peer_id_registry::ACTIVE_CONNECTION_ID_LIMIT,
        )
        .try_into()
        .unwrap();

        //= https://www.rfc-editor.org/rfc/rfc9000#section-7.2
        //# The Destination Connection ID field from the first Initial packet
        //# sent by a client is used to determine packet protection keys for
        //# Initial packets.
        //
        // Use the randomly generated `original_destination_connection_id` to generate the packet
        // protection keys.
        let (initial_key, initial_header_key) =
            <<Cfg::TLSEndpoint as tls::Endpoint>::Session as CryptoSuite>::InitialKey::new_client(
                original_destination_connection_id.as_bytes(),
            );
        let tls_session = endpoint_context
            .tls
            // TODO should SNI be optional? rustls expects a SNI but other tls providers dont seem
            // to require this value.
            .new_client_session(
                &transport_parameters,
                hostname.expect("application should provide a valid server name"),
            );
        let space_manager = PacketSpaceManager::new(
            original_destination_connection_id,
            tls_session,
            initial_key,
            initial_header_key,
            timestamp,
            &mut publisher,
        );

        let wakeup_handle = self
            .wakeup_queue
            .create_wakeup_handle(internal_connection_id);

        let path_handle =
            <<Cfg as endpoint::Config>::PathHandle as path::Handle>::from_remote_address(
                remote_address,
            );

        let connection_parameters = connection::Parameters {
            internal_connection_id,
            local_id_registry,
            peer_id_registry,
            space_manager,
            wakeup_handle,
            peer_connection_id: original_destination_connection_id.into(),
            local_connection_id,
            path_handle,
            congestion_controller,
            timestamp,
            quic_version,
            limits,
            max_mtu: self.max_mtu,
            event_context,
            supervisor_context: &supervisor_context,
            event_subscriber: endpoint_context.event_subscriber,
            datagram_endpoint: endpoint_context.datagram,
        };
        let connection = <Cfg as crate::endpoint::Config>::Connection::new(connection_parameters)?;
        self.connections
            .insert_client_connection(connection, internal_connection_id, sender);
        Ok(())
    }
}

#[cfg(any(test, feature = "testing"))]
pub mod testing {
    use super::*;
    use s2n_quic_core::{endpoint, event::testing::Subscriber, path, random, stateless_reset};

    #[derive(Debug)]
    pub struct Server;

    impl Config for Server {
        type CongestionControllerEndpoint =
            crate::recovery::congestion_controller::testing::mock::Endpoint;
        type TLSEndpoint = s2n_quic_core::crypto::tls::testing::Endpoint;
        type PathHandle = path::RemoteAddress;
        type Connection = connection::Implementation<Self>;
        type ConnectionLock = std::sync::Mutex<Self::Connection>;
        type EndpointLimits = Limits;
        type ConnectionIdFormat = connection::id::testing::Format;
        type StatelessResetTokenGenerator = stateless_reset::token::testing::Generator;
        type RandomGenerator = random::testing::Generator;
        type TokenFormat = s2n_quic_core::token::testing::Format;
        type ConnectionLimits = s2n_quic_core::connection::limits::Limits;
        type Stream = crate::stream::StreamImpl;
        type ConnectionCloseFormatter = s2n_quic_core::connection::close::Development;
        type EventSubscriber = Subscriber;
        type PathMigrationValidator = path::migration::default::Validator;
        type PacketInterceptor = s2n_quic_core::packet::interceptor::Disabled;
        type DatagramEndpoint = s2n_quic_core::datagram::Disabled;

        fn context(&mut self) -> super::Context<Self> {
            todo!()
        }

        const ENDPOINT_TYPE: endpoint::Type = endpoint::Type::Server;
    }

    #[derive(Debug)]
    pub struct Client;

    impl Config for Client {
        type CongestionControllerEndpoint =
            crate::recovery::congestion_controller::testing::mock::Endpoint;
        type TLSEndpoint = s2n_quic_core::crypto::tls::testing::Endpoint;
        type PathHandle = path::RemoteAddress;
        type Connection = connection::Implementation<Self>;
        type ConnectionLock = std::sync::Mutex<Self::Connection>;
        type EndpointLimits = Limits;
        type ConnectionIdFormat = connection::id::testing::Format;
        type StatelessResetTokenGenerator = stateless_reset::token::testing::Generator;
        type RandomGenerator = random::testing::Generator;
        type TokenFormat = s2n_quic_core::token::testing::Format;
        type ConnectionLimits = s2n_quic_core::connection::limits::Limits;
        type Stream = crate::stream::StreamImpl;
        type ConnectionCloseFormatter = s2n_quic_core::connection::close::Development;
        type EventSubscriber = Subscriber;
        type PathMigrationValidator = path::migration::default::Validator;
        type PacketInterceptor = s2n_quic_core::packet::interceptor::Disabled;
        type DatagramEndpoint = s2n_quic_core::datagram::Disabled;

        fn context(&mut self) -> super::Context<Self> {
            todo!()
        }

        const ENDPOINT_TYPE: endpoint::Type = endpoint::Type::Client;
    }

    #[derive(Debug)]
    pub struct Limits;

    impl endpoint::Limiter for Limits {
        fn on_connection_attempt(
            &mut self,
            _attempt: &endpoint::limits::ConnectionAttempt,
        ) -> endpoint::limits::Outcome {
            endpoint::limits::Outcome::allow()
        }
    }
}

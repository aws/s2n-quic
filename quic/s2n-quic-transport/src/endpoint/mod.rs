// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! This module defines a QUIC endpoint

use crate::{
    acceptor::Acceptor,
    connection::{
        self, ConnectionContainer, ConnectionContainerIterationResult, ConnectionIdMapper,
        InternalConnectionId, InternalConnectionIdGenerator, ProcessingError, Trait as _,
    },
    unbounded_channel,
    wakeup_queue::WakeupQueue,
};
use alloc::collections::VecDeque;
use core::task::{self, Poll};
use s2n_codec::{DecoderBuffer, DecoderBufferMut};
use s2n_quic_core::{
    connection::{
        id::{ConnectionInfo, Generator},
        LocalId,
    },
    crypto::{tls, tls::Endpoint as _, CryptoSuite},
    endpoint::{limits::Outcome, Limiter as _},
    event::{self, EndpointPublisher as _},
    inet::{datagram, DatagramInfo},
    io::{rx, tx},
    packet::{initial::ProtectedInitial, ProtectedPacket},
    path::{Handle as _, MaxMtu},
    stateless_reset::token::{Generator as _, LEN as StatelessResetTokenLen},
    time::{Clock, Timestamp},
    token::{self, Format},
};

mod config;
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

/// Safety: The endpoint is marked as `!Send`, because the struct contains `Rc`s.
/// However those `Rcs` are only referenced by other objects within the `Endpoint`
/// and which also get moved.
unsafe impl<Cfg: Config> Send for Endpoint<Cfg> {}

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

        let entries = queue.as_slice_mut();
        let mut now: Option<Timestamp> = None;

        for entry in entries.iter_mut() {
            if let Some((header, payload)) = entry.read() {
                let timestamp = match now {
                    Some(now) => now,
                    _ => {
                        let time = clock.get_time();
                        now = Some(time);
                        time
                    }
                };

                self.receive_datagram(&header, payload, timestamp)
            }
        }
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
            transmit_result =
                connection.on_transmit(queue, timestamp, endpoint_context.event_subscriber);
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
        if !self.connections.is_open() {
            return Poll::Ready(Err(s2n_quic_core::endpoint::CloseError));
        }

        self.wakeup_queue
            .poll_pending_wakeups(&mut self.dequeued_wakeups, cx);
        let nr_wakeups = self.dequeued_wakeups.len();
        let close_packet_buffer = &mut self.close_packet_buffer;
        let endpoint_context = self.config.context();

        let mut now: Option<Timestamp> = None;

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

                if let Err(error) = conn.on_wakeup(timestamp) {
                    conn.close(
                        error,
                        endpoint_context.connection_close_formatter,
                        close_packet_buffer,
                        timestamp,
                        endpoint_context.event_subscriber,
                    );
                }
            });
        }

        if nr_wakeups > 0 {
            Poll::Ready(Ok(nr_wakeups))
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
    /// Creates a new QUIC endpoint using the given configuration
    pub fn new(mut config: Cfg) -> (Self, Acceptor) {
        let (connection_sender, connection_receiver) = unbounded_channel::channel();
        let acceptor = Acceptor::new(connection_receiver);

        let connection_id_mapper =
            ConnectionIdMapper::new(config.context().random_generator, Cfg::ENDPOINT_TYPE);

        let endpoint = Self {
            config,
            connections: ConnectionContainer::new(connection_sender),
            connection_id_generator: InternalConnectionIdGenerator::new(),
            connection_id_mapper,
            wakeup_queue: WakeupQueue::new(),
            dequeued_wakeups: VecDeque::new(),
            version_negotiator: version::Negotiator::default(),
            retry_dispatch: retry::Dispatch::default(),
            stateless_reset_dispatch: stateless_reset::Dispatch::default(),
            close_packet_buffer: Default::default(),
            max_mtu: Default::default(),
        };

        (endpoint, acceptor)
    }

    /// Determine the next step when a peer attempts a connection
    fn connection_allowed(
        &mut self,
        header: &datagram::Header<Cfg::PathHandle>,
        packet: &ProtectedInitial,
    ) -> Option<()> {
        if !self.connections.can_accept() {
            return None;
        }

        let remote_address = header.path.remote_address();

        let attempt = s2n_quic_core::endpoint::limits::ConnectionAttempt::new(
            self.connections.handshake_connections(),
            &remote_address,
        );

        let context = self.config.context();
        let outcome = context.endpoint_limits.on_connection_attempt(&attempt);

        match outcome {
            Outcome::Allow => Some(()),
            Outcome::Retry { delay: _ } => {
                //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#8.1.2
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
            #[allow(unused_variables)]
            Outcome::Close { delay } => {
                //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#5.2.2
                //= type=TODO
                //= tracking-issue=270
                //# If a server refuses to accept a new connection, it SHOULD send an
                //# Initial packet containing a CONNECTION_CLOSE frame with error code
                //# CONNECTION_REFUSED.

                // TODO emit event

                None
            }
            Outcome::Drop => {
                // TODO emit drop event
                None
            }
            _ => {
                // Outcome is non_exhaustive so drop on things we don't understand
                // TODO emit drop event
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
            //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#5.2.2
            //# Servers MUST drop incoming packets under all other circumstances.

            //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#10.3
            //# However, endpoints MUST treat any packet ending
            //# in a valid stateless reset token as a stateless reset, as other QUIC
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
                    reason: event::builder::DropReason::DecodingFailed,
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
                reason: event::builder::DropReason::UnsupportedVersion,
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
                            reason: event::builder::DropReason::InvalidDestinationConnectionId,
                        },
                    );
                    return;
                }
            };

        let datagram = &DatagramInfo {
            timestamp,
            payload_len,
            ecn: header.ecn,
            destination_connection_id,
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
                        endpoint_context.random_generator,
                        endpoint_context.path_migration,
                        max_mtu,
                        endpoint_context.event_subscriber,
                    )
                    .map_err(|_| {
                        // TODO https://github.com/awslabs/s2n-quic/issues/669
                        // We are ignoring all errors here which seems like a bad
                        // practice. If we truly want to ignor all error, lets change the
                        // signature of on_datagram_received to not return a Result.
                        // Otherwise we should introduce an Error code that signifies
                        // it should be silently ignored.

                        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#9
                        //# If the peer
                        //# violates this requirement, the endpoint MUST either drop the incoming
                        //# packets on that path without generating a stateless reset or proceed
                        //# with path validation and allow the peer to migrate.  Generating a
                        //# stateless reset or closing the connection would allow third parties
                        //# in the network to cause connections to close by spoofing or otherwise
                        //# manipulating observed traffic.
                    })?;

                //= https://tools.ietf.org/id/draft-ietf-quic-transport-34.txt#10.2.1
                //# An endpoint
                //# that is closing is not required to process any received frame.

                if let Err(err) = conn.handle_packet(
                    datagram,
                    path_id,
                    packet,
                    endpoint_context.random_generator,
                    endpoint_context.event_subscriber,
                ) {
                    match err {
                        ProcessingError::DuplicatePacket => {
                            // We discard duplicate packets
                        }
                        ProcessingError::ConnectionError(err) => {
                            conn.close(
                                err,
                                endpoint_context.connection_close_formatter,
                                close_packet_buffer,
                                datagram.timestamp,
                                endpoint_context.event_subscriber,
                            );
                            return Err(());
                        }
                        ProcessingError::CryptoError(_) => {
                            // CryptoErrors returned as a result of a packet failing decryption
                            // will be silently discarded, but are a potential indication of a
                            // stateless reset from the peer

                            //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#5.2.1
                            //# Due to packet reordering or loss, a client might receive packets for
                            //# a connection that are encrypted with a key it has not yet computed.
                            //# The client MAY drop these packets, or MAY buffer them in anticipation
                            //# of later packets that allow it to compute the key.
                            //
                            // Packets that fail decryption are discarded rather than buffered.

                            //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#10.3.1
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
                ) {
                    conn.close(
                        err,
                        endpoint_context.connection_close_formatter,
                        close_packet_buffer,
                        datagram.timestamp,
                        endpoint_context.event_subscriber,
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

        if Cfg::ENDPOINT_TYPE.is_server() {
            match packet {
                ProtectedPacket::Initial(packet) => {
                    let source_connection_id =
                        match connection::PeerId::try_from_bytes(packet.source_connection_id()) {
                            Some(connection_id) => connection_id,
                            None => {
                                publisher.on_endpoint_datagram_dropped(
                                    event::builder::EndpointDatagramDropped {
                                        len: payload_len as u16,
                                        reason:
                                            event::builder::DropReason::InvalidSourceConnectionId,
                                    },
                                );
                                return;
                            }
                        };

                    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#8.1
                    //= type=TODO
                    //= tracking-issue=140
                    //# Additionally, a server MAY consider the client address validated if
                    //# the client uses a connection ID chosen by the server and the
                    //# connection ID contains at least 64 bits of entropy.

                    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#8.1.2
                    //# In response to processing an Initial containing a token that was
                    //# provided in a Retry packet, a server cannot send another Retry
                    //# packet; it can only refuse the connection or permit it to proceed.
                    let retry_token_dcid = if !packet.token().is_empty() {
                        let mut context = token::Context::new(
                            &remote_address,
                            &source_connection_id,
                            endpoint_context.random_generator,
                        );
                        if let Some(id) = endpoint_context
                            .token
                            .validate_token(&mut context, packet.token())
                        {
                            //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#8.1.3
                            //# If the
                            //# validation succeeds, the server SHOULD then allow the handshake to
                            //# proceed.
                            Some(id)
                        } else {
                            //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#8.1.3
                            //= type=TODO
                            //= tracking-issue=344
                            //# If the token is invalid then the
                            //# server SHOULD proceed as if the client did not have a validated
                            //# address, including potentially sending a Retry.

                            //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#8.1.2
                            //= type=TODO
                            //= tracking-issue=344
                            //# Instead, the
                            //# server SHOULD immediately close (Section 10.2) the connection with an
                            //# INVALID_TOKEN error.
                            publisher.on_endpoint_datagram_dropped(
                                event::builder::EndpointDatagramDropped {
                                    len: payload_len as u16,
                                    reason: event::builder::DropReason::InvalidRetryToken,
                                },
                            );

                            //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#8.1.3
                            //# Servers MAY
                            //# discard any Initial packet that does not carry the expected token.
                            return;
                        }
                    } else {
                        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#8.1.2
                        //# Upon receiving the client's Initial packet, the server can request
                        //# address validation by sending a Retry packet (Section 17.2.5)
                        //# containing a token.
                        if self.connection_allowed(header, &packet).is_none() {
                            let mut publisher = event::EndpointPublisherSubscriber::new(
                                event::builder::EndpointMeta {
                                    endpoint_type: Cfg::ENDPOINT_TYPE,
                                    timestamp,
                                },
                                None,
                                self.config.context().event_subscriber,
                            );

                            publisher.on_endpoint_datagram_dropped(
                                event::builder::EndpointDatagramDropped {
                                    len: payload_len as u16,
                                    reason: event::builder::DropReason::ConnectionNotAllowed,
                                },
                            );

                            //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#17.2.5.1
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
                        // TODO emit event
                        dbg!(err);
                    }
                }
                _ => {
                    let is_short_header_packet = matches!(packet, ProtectedPacket::Short(_));
                    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#10.3.1
                    //# Endpoints MAY skip this check if any packet from a datagram is
                    //# successfully processed.  However, the comparison MUST be performed
                    //# when the first packet in an incoming datagram either cannot be
                    //# associated with a connection, or cannot be decrypted.

                    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#10.3
                    //# However, endpoints MUST treat any packet ending
                    //# in a valid stateless reset token as a stateless reset, as other QUIC
                    //# versions might allow the use of a long header.
                    let is_stateless_reset = self
                        .close_on_matching_stateless_reset(payload, timestamp)
                        .is_some();

                    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#9.3.2
                    //# For instance, an endpoint MAY send a stateless reset in
                    //# response to any further incoming packets.

                    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#10.3
                    //# An endpoint MAY send a stateless reset in response to receiving a packet
                    //# that it cannot associate with an active connection.

                    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#10.3
                    //# Because the stateless reset token is not available
                    //# until connection establishment is complete or near completion,
                    //# ignoring an unknown packet with a long header might be as effective
                    //# as sending a stateless reset.
                    if !is_stateless_reset
                        && Cfg::StatelessResetTokenGenerator::ENABLED
                        && is_short_header_packet
                    {
                        self.enqueue_stateless_reset(header, datagram, &destination_connection_id);
                    }
                }
            }
        } else {
            // TODO: Find out what is required for the client. It seems like
            // those should at least send stateless resets on Initial packets
        }

        // TODO: Handle version negotiation packets
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

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#10.3.1
        //# The endpoint identifies a
        //# received datagram as a stateless reset by comparing the last 16 bytes
        //# of the datagram with all Stateless Reset Tokens associated with the
        //# remote address on which the datagram was received.
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

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#10.3.1
        //# If the last 16 bytes of the datagram are identical in value to a
        //# Stateless Reset Token, the endpoint MUST enter the draining period
        //# and not send any further packets on this connection.
        self.connections.with_connection(internal_id, |conn| {
            conn.close(
                connection::Error::StatelessReset,
                endpoint_context.connection_close_formatter,
                close_packet_buffer,
                timestamp,
                endpoint_context.event_subscriber,
            );
        });

        Some(internal_id)
    }

    fn on_timeout(&mut self, timestamp: Timestamp) {
        let connection_id_mapper = &mut self.connection_id_mapper;
        let close_packet_buffer = &mut self.close_packet_buffer;
        let endpoint_context = self.config.context();

        self.connections.iterate_timeout_list(timestamp, |conn| {
            if let Err(error) = conn.on_timeout(
                connection_id_mapper,
                timestamp,
                endpoint_context.event_subscriber,
            ) {
                conn.close(
                    error,
                    endpoint_context.connection_close_formatter,
                    close_packet_buffer,
                    timestamp,
                    endpoint_context.event_subscriber,
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
            endpoint::limits::Outcome::Allow
        }
    }
}

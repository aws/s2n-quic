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
    endpoint, path, stream,
};
use bytes::Bytes;
use core::task::{Context, Poll};
use s2n_codec::DecoderBufferMut;
use s2n_quic_core::{
    application, event,
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
    random, stateless_reset,
    time::Timestamp,
};

/// A trait which represents an internally used `Connection`
pub trait ConnectionTrait: 'static + Send + Sized {
    /// Static configuration of a connection
    type Config: endpoint::Config;

    /// Creates a new `Connection` instance with the given configuration
    fn new(parameters: ConnectionParameters<Self::Config>) -> Self;

    /// Returns the Connections internal ID
    fn internal_connection_id(&self) -> InternalConnectionId;

    /// Returns whether the connection is in the handshake state
    fn is_handshaking(&self) -> bool;

    /// Initiates closing the connection as described in
    /// https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#10
    fn close(
        &mut self,
        error: connection::Error,
        close_formatter: &<Self::Config as endpoint::Config>::ConnectionCloseFormatter,
        packet_buffer: &mut endpoint::PacketBuffer,
        timestamp: Timestamp,
        subscriber: &mut <Self::Config as endpoint::Config>::EventSubscriber,
    );

    /// Marks a connection which advertised itself as having completed the handshake
    /// (via [`ConnectionInterests`]) as accepted. After this call the `accept` interest should
    /// no longer be signalled.
    fn mark_as_accepted(&mut self);

    /// Generates and registers new connection IDs using the given `ConnectionIdFormat` and
    /// `StatelessResetTokenGenerator`
    fn on_new_connection_id<
        ConnectionIdFormat: connection::id::Format,
        StatelessResetTokenGenerator: stateless_reset::token::Generator,
    >(
        &mut self,
        connection_id_format: &mut ConnectionIdFormat,
        stateless_reset_token_generator: &mut StatelessResetTokenGenerator,
        timestamp: Timestamp,
    ) -> Result<(), LocalIdRegistrationError>;

    /// Queries the connection for outgoing packets
    fn on_transmit<Tx>(
        &mut self,
        queue: &mut Tx,
        timestamp: Timestamp,
        subscriber: &mut <Self::Config as endpoint::Config>::EventSubscriber,
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
        subscriber: &mut <Self::Config as endpoint::Config>::EventSubscriber,
    ) -> Result<(), connection::Error>;

    /// Handles all external wakeups on the [`Connection`].
    fn on_wakeup(&mut self, timestamp: Timestamp) -> Result<(), connection::Error>;

    // Packet handling

    /// Is called when an initial packet had been received
    fn handle_initial_packet<Rnd: random::Generator>(
        &mut self,
        datagram: &DatagramInfo,
        path_id: path::Id,
        packet: ProtectedInitial,
        random_generator: &mut Rnd,
        subscriber: &mut <Self::Config as endpoint::Config>::EventSubscriber,
    ) -> Result<(), ProcessingError>;

    /// Is called when an unprotected initial packet had been received
    fn handle_cleartext_initial_packet<Rnd: random::Generator>(
        &mut self,
        datagram: &DatagramInfo,
        path_id: path::Id,
        packet: CleartextInitial,
        random_generator: &mut Rnd,
        subscriber: &mut <Self::Config as endpoint::Config>::EventSubscriber,
    ) -> Result<(), ProcessingError>;

    /// Is called when a handshake packet had been received
    fn handle_handshake_packet<Rnd: random::Generator>(
        &mut self,
        datagram: &DatagramInfo,
        path_id: path::Id,
        packet: ProtectedHandshake,
        random_generator: &mut Rnd,
        subscriber: &mut <Self::Config as endpoint::Config>::EventSubscriber,
    ) -> Result<(), ProcessingError>;

    /// Is called when a short packet had been received
    fn handle_short_packet<Rnd: random::Generator>(
        &mut self,
        datagram: &DatagramInfo,
        path_id: path::Id,
        packet: ProtectedShort,
        random_generator: &mut Rnd,
        subscriber: &mut <Self::Config as endpoint::Config>::EventSubscriber,
    ) -> Result<(), ProcessingError>;

    /// Is called when a version negotiation packet had been received
    fn handle_version_negotiation_packet(
        &mut self,
        datagram: &DatagramInfo,
        path_id: path::Id,
        packet: ProtectedVersionNegotiation,
        subscriber: &mut <Self::Config as endpoint::Config>::EventSubscriber,
    ) -> Result<(), ProcessingError>;

    /// Is called when a zero rtt packet had been received
    fn handle_zero_rtt_packet(
        &mut self,
        datagram: &DatagramInfo,
        path_id: path::Id,
        packet: ProtectedZeroRtt,
        subscriber: &mut <Self::Config as endpoint::Config>::EventSubscriber,
    ) -> Result<(), ProcessingError>;

    /// Is called when a retry packet had been received
    fn handle_retry_packet(
        &mut self,
        datagram: &DatagramInfo,
        path_id: path::Id,
        packet: ProtectedRetry,
        subscriber: &mut <Self::Config as endpoint::Config>::EventSubscriber,
    ) -> Result<(), ProcessingError>;

    /// Notifies a connection it has received a datagram from a peer
    fn on_datagram_received(
        &mut self,
        path_handle: &<Self::Config as endpoint::Config>::PathHandle,
        datagram: &DatagramInfo,
        congestion_controller_endpoint: &mut <Self::Config as endpoint::Config>::CongestionControllerEndpoint,
        random_generator: &mut <Self::Config as endpoint::Config>::RandomGenerator,
        max_mtu: MaxMtu,
        subscriber: &mut <Self::Config as endpoint::Config>::EventSubscriber,
    ) -> Result<path::Id, connection::Error>;

    /// Returns the Connections interests
    fn interests(&self) -> ConnectionInterests;

    /// Returns the QUIC version selected for the current connection
    fn quic_version(&self) -> u32;

    /// Handles reception of a single QUIC packet
    fn handle_packet<Rnd: random::Generator>(
        &mut self,
        datagram: &DatagramInfo,
        path_id: path::Id,
        packet: ProtectedPacket,
        random_generator: &mut Rnd,
        subscriber: &mut <Self::Config as endpoint::Config>::EventSubscriber,
    ) -> Result<(), ProcessingError> {
        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#5.2.1
        //# If a client receives a packet that uses a different version than it
        //# initially selected, it MUST discard that packet.
        if let Some(version) = packet.version() {
            if version != self.quic_version() {
                // TODO emit packet dropped event here with version mismatch reason
                return Ok(());
            }
        }

        //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#4.1.4
        //# An endpoint SHOULD continue
        //# to respond to packets that can be processed during this time.
        // We make a best effort to process all of the packet spaces we have available. There isn't
        // any special logic required to meet this requirement as each packet is handled
        // independently.

        match packet {
            ProtectedPacket::Short(packet) => {
                self.handle_short_packet(datagram, path_id, packet, random_generator, subscriber)
            }
            ProtectedPacket::VersionNegotiation(packet) => {
                self.handle_version_negotiation_packet(datagram, path_id, packet, subscriber)
            }
            ProtectedPacket::Initial(packet) => {
                self.handle_initial_packet(datagram, path_id, packet, random_generator, subscriber)
            }
            ProtectedPacket::ZeroRtt(packet) => {
                self.handle_zero_rtt_packet(datagram, path_id, packet, subscriber)
            }
            ProtectedPacket::Handshake(packet) => self.handle_handshake_packet(
                datagram,
                path_id,
                packet,
                random_generator,
                subscriber,
            ),
            ProtectedPacket::Retry(packet) => {
                self.handle_retry_packet(datagram, path_id, packet, subscriber)
            }
        }
    }

    /// This is called to handle the remaining and yet undecoded packets inside
    /// a datagram.
    #[allow(clippy::too_many_arguments)]
    fn handle_remaining_packets<Validator: connection::id::Validator, Rnd: random::Generator>(
        &mut self,
        path_handle: &<Self::Config as endpoint::Config>::PathHandle,
        datagram: &DatagramInfo,
        path_id: path::Id,
        connection_id_validator: &Validator,
        mut payload: DecoderBufferMut,
        random_generator: &mut Rnd,
        subscriber: &mut <Self::Config as endpoint::Config>::EventSubscriber,
    ) -> Result<(), connection::Error> {
        let remote_address = path_handle.remote_address();
        let connection_info = ConnectionInfo::new(&remote_address);

        while !payload.is_empty() {
            if let Ok((packet, remaining)) =
                ProtectedPacket::decode(payload, &connection_info, connection_id_validator)
            {
                payload = remaining;

                //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#12.2
                //# Senders MUST NOT coalesce QUIC packets
                //# with different connection IDs into a single UDP datagram.  Receivers
                //# SHOULD ignore any subsequent packets with a different Destination
                //# Connection ID than the first packet in the datagram.
                if datagram.destination_connection_id.as_bytes()
                    != packet.destination_connection_id()
                {
                    // TODO emit packet dropped event with different CID reason
                    break;
                }

                let result =
                    self.handle_packet(datagram, path_id, packet, random_generator, subscriber);

                if let Err(ProcessingError::ConnectionError(err)) = result {
                    // CryptoErrors returned as a result of a packet failing decryption will be
                    // silently discarded, but this method could return an error on protocol
                    // violations which would result in shutting down the connection anyway. In this
                    // case this will return early without processing the remaining packets.
                    return Err(err);
                }
            } else {
                //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#12.2
                //# Every QUIC packet that is coalesced into a single UDP datagram is
                //# separate and complete.  The receiver of coalesced QUIC packets MUST
                //# individually process each QUIC packet and separately acknowledge
                //# them, as if they were received as the payload of different UDP
                //# datagrams.  For example, if decryption fails (because the keys are
                //# not available or any other reason), the receiver MAY either discard
                //# or buffer the packet for later processing and MUST attempt to process
                //# the remaining packets.

                // we choose to discard the rest of the datagram on parsing errors since it would
                // be difficult to recover from an invalid packet.

                // TODO emit packet dropped event with packet corruption reason

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
        context: &Context,
    ) -> Poll<Result<stream::StreamId, connection::Error>>;

    fn application_close(&mut self, error: Option<application::Error>);

    fn sni(&self) -> Option<Bytes>;

    fn alpn(&self) -> Bytes;

    fn ping(&mut self) -> Result<(), connection::Error>;

    fn local_address(&self) -> Result<SocketAddress, connection::Error>;

    fn remote_address(&self) -> Result<SocketAddress, connection::Error>;

    fn query_mut(&mut self, query: &mut dyn event::ConnectionQuery);
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

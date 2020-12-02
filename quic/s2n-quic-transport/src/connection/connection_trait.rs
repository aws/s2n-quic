//! This module contains the implementation of QUIC `Connections` and their management

use crate::{
    connection::{
        self, connection_id_mapper::ConnectionIdMapperRegistrationError,
        connection_interests::ConnectionInterests, id::ConnectionInfo,
        internal_connection_id::InternalConnectionId, shared_state::SharedConnectionState,
        CloseReason as ConnectionCloseReason, Parameters as ConnectionParameters,
    },
    contexts::ConnectionOnTransmitError,
    path,
    recovery::congestion_controller,
};
use s2n_codec::DecoderBufferMut;
use s2n_quic_core::{
    inet::DatagramInfo,
    io::tx,
    packet::{
        handshake::ProtectedHandshake,
        initial::{CleartextInitial, ProtectedInitial},
        retry::ProtectedRetry,
        short::ProtectedShort,
        version_negotiation::ProtectedVersionNegotiation,
        zero_rtt::ProtectedZeroRTT,
        ProtectedPacket,
    },
    time::Timestamp,
    transport::error::TransportError,
};

/// A trait which represents an internally used `Connection`
pub trait ConnectionTrait: Sized {
    /// Static configuration of a connection
    type Config: connection::Config;

    /// Creates a new `Connection` instance with the given configuration
    fn new(parameters: ConnectionParameters<Self::Config>) -> Self;

    /// Returns the connections configuration
    fn config(&self) -> &Self::Config;

    /// Returns the Connections internal ID
    fn internal_connection_id(&self) -> InternalConnectionId;

    /// Initiates closing the connection as described in
    /// https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#10
    ///
    /// This method can be called for any of the close reasons:
    /// - Idle timeout
    /// - Immediate close
    fn close(
        &mut self,
        shared_state: &mut SharedConnectionState<Self::Config>,
        close_reason: ConnectionCloseReason,
        timestamp: Timestamp,
    );

    /// Marks a connection which advertised itself as having completed the handshake
    /// (via [`ConnectionInterests`]) as accepted. After this call the `accept` interest should
    /// no longer be signalled.
    fn mark_as_accepted(&mut self);

    /// Queries the connection for interest in new connection IDs
    fn on_new_connection_id<ConnectionIdFormat: connection::id::Format>(
        &mut self,
        connection_id_format: &mut ConnectionIdFormat,
        timestamp: Timestamp,
    ) -> Result<(), ConnectionIdMapperRegistrationError>;

    /// Queries the connection for outgoing packets
    fn on_transmit<Tx: tx::Queue>(
        &mut self,
        shared_state: &mut SharedConnectionState<Self::Config>,
        context: &mut Tx,
        timestamp: Timestamp,
    ) -> Result<(), ConnectionOnTransmitError>;

    /// Handles all timeouts on the `Connection`.
    ///
    /// `timestamp` passes the current time.
    fn on_timeout(
        &mut self,
        shared_state: &mut SharedConnectionState<Self::Config>,
        timestamp: Timestamp,
    );

    /// Updates the per-connection timer based on individual component timers.
    /// This method is used in order to update the connection timer only once
    /// per interaction with the connection and thereby to batch timer updates.
    fn update_connection_timer(&mut self, shared_state: &mut SharedConnectionState<Self::Config>);

    /// Handles all external wakeups on the [`Connection`].
    fn on_wakeup(
        &mut self,
        shared_state: &mut SharedConnectionState<Self::Config>,
        timestamp: Timestamp,
    );

    // Packet handling

    /// Is called when a handshake packet had been received
    fn handle_handshake_packet(
        &mut self,
        shared_state: &mut SharedConnectionState<Self::Config>,
        datagram: &DatagramInfo,
        path_id: path::Id,
        packet: ProtectedHandshake,
    ) -> Result<(), TransportError>;

    /// Is called when a initial packet had been received
    fn handle_initial_packet(
        &mut self,
        shared_state: &mut SharedConnectionState<Self::Config>,
        datagram: &DatagramInfo,
        path_id: path::Id,
        packet: ProtectedInitial,
    ) -> Result<(), TransportError>;

    /// Is called when an unprotected initial packet had been received
    fn handle_cleartext_initial_packet(
        &mut self,
        shared_state: &mut SharedConnectionState<Self::Config>,
        datagram: &DatagramInfo,
        path_id: path::Id,
        packet: CleartextInitial,
    ) -> Result<(), TransportError>;

    /// Is called when a short packet had been received
    fn handle_short_packet(
        &mut self,
        shared_state: &mut SharedConnectionState<Self::Config>,
        datagram: &DatagramInfo,
        path_id: path::Id,
        packet: ProtectedShort,
    ) -> Result<(), TransportError>;

    /// Is called when a version negotiation packet had been received
    fn handle_version_negotiation_packet(
        &mut self,
        shared_state: &mut SharedConnectionState<Self::Config>,
        datagram: &DatagramInfo,
        path_id: path::Id,
        packet: ProtectedVersionNegotiation,
    ) -> Result<(), TransportError>;

    /// Is called when a zero rtt packet had been received
    fn handle_zero_rtt_packet(
        &mut self,
        shared_state: &mut SharedConnectionState<Self::Config>,
        datagram: &DatagramInfo,
        path_id: path::Id,
        packet: ProtectedZeroRTT,
    ) -> Result<(), TransportError>;

    /// Is called when a retry packet had been received
    fn handle_retry_packet(
        &mut self,
        shared_state: &mut SharedConnectionState<Self::Config>,
        datagram: &DatagramInfo,
        path_id: path::Id,
        packet: ProtectedRetry,
    ) -> Result<(), TransportError>;

    /// Handles a transport error that occurred during packet reception
    fn handle_transport_error(
        &mut self,
        shared_state: &mut SharedConnectionState<Self::Config>,
        datagram: &DatagramInfo,
        transport_error: TransportError,
    );

    /// Notifies a connection it has received a datagram from a peer
    fn on_datagram_received<
        CC: congestion_controller::Endpoint<
            CongestionController = <Self::Config as connection::Config>::CongestionController,
        >,
    >(
        &mut self,
        shared_state: &mut SharedConnectionState<Self::Config>,
        datagram: &DatagramInfo,
        peer_connection_id: &connection::Id,
        congestion_controller_endpoint: &mut CC,
    ) -> Result<path::Id, TransportError>;

    /// Returns the Connections interests
    fn interests(&self, shared_state: &SharedConnectionState<Self::Config>) -> ConnectionInterests;

    /// Handles reception of a single QUIC packet
    fn handle_packet(
        &mut self,
        shared_state: &mut SharedConnectionState<Self::Config>,
        datagram: &DatagramInfo,
        path_id: path::Id,
        packet: ProtectedPacket,
    ) -> Result<(), TransportError> {
        match packet {
            ProtectedPacket::Short(packet) => {
                self.handle_short_packet(shared_state, datagram, path_id, packet)
            }
            ProtectedPacket::VersionNegotiation(packet) => {
                self.handle_version_negotiation_packet(shared_state, datagram, path_id, packet)
            }
            ProtectedPacket::Initial(packet) => {
                self.handle_initial_packet(shared_state, datagram, path_id, packet)
            }
            ProtectedPacket::ZeroRTT(packet) => {
                self.handle_zero_rtt_packet(shared_state, datagram, path_id, packet)
            }
            ProtectedPacket::Handshake(packet) => {
                self.handle_handshake_packet(shared_state, datagram, path_id, packet)
            }
            ProtectedPacket::Retry(packet) => {
                self.handle_retry_packet(shared_state, datagram, path_id, packet)
            }
        }
    }

    /// This is called to handle the remaining and yet undecoded packets inside
    /// a datagram.
    fn handle_remaining_packets<Validator: connection::id::Validator>(
        &mut self,
        shared_state: &mut SharedConnectionState<Self::Config>,
        datagram: &DatagramInfo,
        path_id: path::Id,
        original_connection_id: connection::Id,
        connection_id_validator: &Validator,
        mut payload: DecoderBufferMut,
    ) -> Result<(), TransportError> {
        let connection_info = ConnectionInfo::new(&datagram.remote_address);

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
                if original_connection_id.as_bytes() != packet.destination_connection_id() {
                    break;
                }

                // Packet processing should silently discard packets that fail decryption
                // but this method could return an error on protocol violations which would result
                // in shutting down the connection anyway. In this case this will return early
                // without processing the remaining packets.
                self.handle_packet(shared_state, datagram, path_id, packet)?;
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
                break;
            }
        }

        Ok(())
    }
}

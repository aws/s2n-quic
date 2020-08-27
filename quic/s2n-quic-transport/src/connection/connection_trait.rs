//! This module contains the implementation of QUIC `Connections` and their management

use crate::{
    connection::{
        connection_interests::ConnectionInterests, internal_connection_id::InternalConnectionId,
        shared_state::SharedConnectionState, ConnectionCloseReason, ConnectionConfig,
        ConnectionParameters,
    },
    contexts::ConnectionOnTransmitError,
    processed_packet::ProcessedPacket,
    space::{PacketSpace, PacketSpaceHandler, PacketSpaceManager},
};
use s2n_codec::DecoderBufferMut;
use s2n_quic_core::{
    connection::ConnectionId,
    frame::{Frame, FrameMut},
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
    varint::VarInt,
};

/// A trait which represents an internally used `Connection`
pub trait ConnectionTrait: Sized {
    /// Static configuration of a connection
    type Config: ConnectionConfig;

    /// Creates a new `Connection` instance with the given configuration
    fn new(parameters: ConnectionParameters<Self::Config>) -> Self;

    /// Returns the connections configuration
    fn config(&self) -> &Self::Config;

    /// Returns the Connections internal ID
    fn internal_connection_id(&self) -> InternalConnectionId;

    /// Initiates closing the connection as described in
    /// https://tools.ietf.org/id/draft-ietf-quic-transport-25.txt#10
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
        packet: ProtectedHandshake,
    ) -> Result<(), TransportError>;

    /// Is called when a initial packet had been received
    fn handle_initial_packet(
        &mut self,
        shared_state: &mut SharedConnectionState<Self::Config>,
        datagram: &DatagramInfo,
        packet: ProtectedInitial,
    ) -> Result<(), TransportError>;

    /// Is called when an unprotected initial packet had been received
    fn handle_cleartext_initial_packet(
        &mut self,
        shared_state: &mut SharedConnectionState<Self::Config>,
        datagram: &DatagramInfo,
        packet: CleartextInitial,
    ) -> Result<(), TransportError>;

    /// Is called when a short packet had been received
    fn handle_short_packet(
        &mut self,
        shared_state: &mut SharedConnectionState<Self::Config>,
        datagram: &DatagramInfo,
        packet: ProtectedShort,
    ) -> Result<(), TransportError>;

    /// Is called when a version negotiation packet had been received
    fn handle_version_negotiation_packet(
        &mut self,
        shared_state: &mut SharedConnectionState<Self::Config>,
        datagram: &DatagramInfo,
        packet: ProtectedVersionNegotiation,
    ) -> Result<(), TransportError>;

    /// Is called when a zero rtt packet had been received
    fn handle_zero_rtt_packet(
        &mut self,
        shared_state: &mut SharedConnectionState<Self::Config>,
        datagram: &DatagramInfo,
        packet: ProtectedZeroRTT,
    ) -> Result<(), TransportError>;

    /// Is called when a retry packet had been received
    fn handle_retry_packet(
        &mut self,
        shared_state: &mut SharedConnectionState<Self::Config>,
        datagram: &DatagramInfo,
        packet: ProtectedRetry,
    ) -> Result<(), TransportError>;

    /// Handles a transport error that occurred during packet reception
    fn handle_transport_error(
        &mut self,
        shared_state: &mut SharedConnectionState<Self::Config>,
        datagram: &DatagramInfo,
        transport_error: TransportError,
    );

    /// Returns the Connections interests
    fn interests(&self, shared_state: &SharedConnectionState<Self::Config>) -> ConnectionInterests;

    /// Handles reception of a single QUIC packet
    fn handle_packet(
        &mut self,
        shared_state: &mut SharedConnectionState<Self::Config>,
        datagram: &DatagramInfo,
        packet: ProtectedPacket,
    ) -> Result<(), TransportError> {
        match packet {
            ProtectedPacket::Short(packet) => {
                self.handle_short_packet(shared_state, datagram, packet)
            }
            ProtectedPacket::VersionNegotiation(packet) => {
                self.handle_version_negotiation_packet(shared_state, datagram, packet)
            }
            ProtectedPacket::Initial(packet) => {
                self.handle_initial_packet(shared_state, datagram, packet)
            }
            ProtectedPacket::ZeroRTT(packet) => {
                self.handle_zero_rtt_packet(shared_state, datagram, packet)
            }
            ProtectedPacket::Handshake(packet) => {
                self.handle_handshake_packet(shared_state, datagram, packet)
            }
            ProtectedPacket::Retry(packet) => {
                self.handle_retry_packet(shared_state, datagram, packet)
            }
        }
    }

    fn handle_first_and_remaining_packets(
        &mut self,
        shared_state: &mut SharedConnectionState<Self::Config>,
        datagram: &DatagramInfo,
        first_packet: ProtectedPacket,
        original_connection_id: ConnectionId,
        payload: DecoderBufferMut,
    ) -> Result<(), ()> {
        if let Err(err) = self.handle_packet(shared_state, datagram, first_packet) {
            self.handle_transport_error(shared_state, datagram, err);
            return Err(());
        }
        if let Err(err) =
            self.handle_remaining_packets(shared_state, datagram, original_connection_id, payload)
        {
            self.handle_transport_error(shared_state, datagram, err);
            return Err(());
        }
        Ok(())
    }

    /// This is called to handle the remaining and yet undecoded packets inside
    /// a datagram.
    fn handle_remaining_packets(
        &mut self,
        shared_state: &mut SharedConnectionState<Self::Config>,
        datagram: &DatagramInfo,
        original_connection_id: ConnectionId,
        mut payload: DecoderBufferMut,
    ) -> Result<(), TransportError> {
        let destination_connnection_id_decoder = self.config().destination_connnection_id_decoder();

        while !payload.is_empty() {
            let (packet, remaining) =
                ProtectedPacket::decode(payload, destination_connnection_id_decoder)?;
            payload = remaining;

            //= https://tools.ietf.org/id/draft-ietf-quic-transport-24.txt#12.2
            //# Senders MUST NOT coalesce QUIC packets for different connections into
            //# a single UDP datagram.  Receivers SHOULD ignore any subsequent
            //# packets with a different Destination Connection ID than the first
            //# packet in the datagram.
            if original_connection_id.as_bytes() != packet.destination_connection_id() {
                break;
            }

            self.handle_packet(shared_state, datagram, packet)?;
        }

        Ok(())
    }

    fn handle_cleartext_packet<'a, Packet>(
        &mut self,
        shared_state: &mut SharedConnectionState<Self::Config>,
        datagram: &DatagramInfo,
        packet: Packet,
    ) -> Result<(), TransportError>
    where
        PacketSpaceManager<Self::Config>: PacketSpaceHandler<'a, Packet>,
    {
        let (space, packet_number, mut payload) =
            if let Some(result) = shared_state.space_manager.space_for_packet(packet) {
                result
            } else {
                // Packet space is not available, drop the packet
                return Ok(());
            };

        let mut processed_packet = ProcessedPacket::new(packet_number, datagram);

        macro_rules! with_frame_type {
            ($frame:ident) => {{
                let frame_type = $frame.tag();
                move |err: TransportError| err.with_frame_type(VarInt::from_u8(frame_type))
            }};
        }

        while !payload.is_empty() {
            let (frame, remaining) = payload.decode::<FrameMut>()?;

            match frame {
                Frame::Padding(frame) => {
                    //= https://tools.ietf.org/id/draft-ietf-quic-transport-27.txt#19.1
                    //# A PADDING frame has no content.  That is, a PADDING frame consists of
                    //# the single byte that identifies the frame as a PADDING frame.
                    processed_packet.on_processed_frame(&frame);
                }
                Frame::Ping(frame) => {
                    //= https://tools.ietf.org/id/draft-ietf-quic-transport-27.txt#19.2
                    //# The receiver of a PING frame simply needs to acknowledge the packet
                    //# containing this frame.
                    processed_packet.on_processed_frame(&frame);
                }
                Frame::Crypto(frame) => {
                    let on_error = with_frame_type!(frame);
                    processed_packet.on_processed_frame(&frame);
                    space
                        .handle_crypto_frame(datagram, frame.into())
                        .map_err(on_error)?;
                }
                Frame::Ack(frame) => {
                    let on_error = with_frame_type!(frame);
                    processed_packet.on_processed_frame(&frame);
                    space.handle_ack_frame(datagram, frame).map_err(on_error)?;
                }
                Frame::ConnectionClose(frame) => {
                    self.close(
                        shared_state,
                        ConnectionCloseReason::PeerImmediateClose(frame),
                        datagram.timestamp,
                    );
                    return Ok(());
                }
                Frame::Stream(frame) => {
                    let on_error = with_frame_type!(frame);
                    processed_packet.on_processed_frame(&frame);
                    space
                        .handle_stream_frame(datagram, frame.into())
                        .map_err(on_error)?;
                }
                Frame::DataBlocked(frame) => {
                    let on_error = with_frame_type!(frame);
                    processed_packet.on_processed_frame(&frame);
                    space
                        .handle_data_blocked_frame(datagram, frame)
                        .map_err(on_error)?;
                }
                Frame::MaxData(frame) => {
                    let on_error = with_frame_type!(frame);
                    processed_packet.on_processed_frame(&frame);
                    space
                        .handle_max_data_frame(datagram, frame)
                        .map_err(on_error)?;
                }
                Frame::MaxStreamData(frame) => {
                    let on_error = with_frame_type!(frame);
                    processed_packet.on_processed_frame(&frame);
                    space
                        .handle_max_stream_data_frame(datagram, frame)
                        .map_err(on_error)?;
                }
                Frame::MaxStreams(frame) => {
                    let on_error = with_frame_type!(frame);
                    processed_packet.on_processed_frame(&frame);
                    space
                        .handle_max_streams_frame(datagram, frame)
                        .map_err(on_error)?;
                }
                Frame::ResetStream(frame) => {
                    let on_error = with_frame_type!(frame);
                    processed_packet.on_processed_frame(&frame);
                    space
                        .handle_reset_stream_frame(datagram, frame)
                        .map_err(on_error)?;
                }
                Frame::StopSending(frame) => {
                    let on_error = with_frame_type!(frame);
                    processed_packet.on_processed_frame(&frame);
                    space
                        .handle_stop_sending_frame(datagram, frame)
                        .map_err(on_error)?;
                }
                Frame::StreamDataBlocked(frame) => {
                    let on_error = with_frame_type!(frame);
                    processed_packet.on_processed_frame(&frame);
                    space
                        .handle_stream_data_blocked_frame(datagram, frame)
                        .map_err(on_error)?;
                }
                Frame::StreamsBlocked(frame) => {
                    let on_error = with_frame_type!(frame);
                    processed_packet.on_processed_frame(&frame);
                    space
                        .handle_streams_blocked_frame(datagram, frame)
                        .map_err(on_error)?;
                }
                Frame::NewToken(frame) => {
                    let on_error = with_frame_type!(frame);
                    processed_packet.on_processed_frame(&frame);
                    space
                        .handle_new_token_frame(datagram, frame)
                        .map_err(on_error)?;
                }
                Frame::NewConnectionID(frame) => {
                    let on_error = with_frame_type!(frame);
                    processed_packet.on_processed_frame(&frame);
                    space
                        .handle_new_connection_id_frame(datagram, frame)
                        .map_err(on_error)?;
                }
                Frame::RetireConnectionID(frame) => {
                    let on_error = with_frame_type!(frame);
                    processed_packet.on_processed_frame(&frame);
                    space
                        .handle_retire_connection_id_frame(datagram, frame)
                        .map_err(on_error)?;
                }
                Frame::PathChallenge(frame) => {
                    let on_error = with_frame_type!(frame);
                    processed_packet.on_processed_frame(&frame);
                    space
                        .handle_path_challenge_frame(datagram, frame)
                        .map_err(on_error)?;
                }
                Frame::PathResponse(frame) => {
                    let on_error = with_frame_type!(frame);
                    processed_packet.on_processed_frame(&frame);
                    space
                        .handle_path_response_frame(datagram, frame)
                        .map_err(on_error)?;
                }
                Frame::HandshakeDone(frame) => {
                    let on_error = with_frame_type!(frame);
                    processed_packet.on_processed_frame(&frame);
                    space
                        .handle_handshake_done_frame(datagram, frame)
                        .map_err(on_error)?;
                }
            }

            payload = remaining;
        }

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-27.txt#13.1
        //# A packet MUST NOT be acknowledged until packet protection has been
        //# successfully removed and all frames contained in the packet have been
        //# processed.  For STREAM frames, this means the data has been enqueued
        //# in preparation to be received by the application protocol, but it
        //# does not require that data is delivered and consumed.
        //#
        //# Once the packet has been fully processed, a receiver acknowledges
        //# receipt by sending one or more ACK frames containing the packet
        //# number of the received packet.

        space.on_processed_packet(processed_packet)?;

        Ok(())
    }
}

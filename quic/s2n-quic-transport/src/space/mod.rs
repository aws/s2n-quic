use crate::{
    connection, path, processed_packet::ProcessedPacket, space::rx_packet_numbers::AckManager,
    transmission,
};
use s2n_codec::DecoderBufferMut;
use s2n_quic_core::{
    ack,
    connection::limits::Limits,
    crypto::{tls::Session as TLSSession, CryptoSuite},
    endpoint,
    frame::{
        self, ack::AckRanges, crypto::CryptoRef, stream::StreamRef, Ack, ConnectionClose,
        DataBlocked, HandshakeDone, MaxData, MaxStreamData, MaxStreams, NewConnectionID, NewToken,
        PathChallenge, PathResponse, ResetStream, RetireConnectionID, StopSending,
        StreamDataBlocked, StreamsBlocked,
    },
    inet::DatagramInfo,
    packet::number::{PacketNumber, PacketNumberSpace},
    path::Path,
    time::Timestamp,
    transport::error::TransportError,
};

mod application;
mod crypto_stream;
mod handshake;
mod handshake_status;
mod initial;
pub(crate) mod rx_packet_numbers;
mod session_context;
mod tx_packet_numbers;

pub(crate) use application::ApplicationSpace;
pub(crate) use crypto_stream::CryptoStream;
pub(crate) use handshake::HandshakeSpace;
pub(crate) use handshake_status::HandshakeStatus;
pub(crate) use initial::InitialSpace;
pub(crate) use session_context::SessionContext;
pub(crate) use tx_packet_numbers::TxPacketNumbers;

pub struct PacketSpaceManager<ConnectionConfigType: connection::Config> {
    session: Option<ConnectionConfigType::TLSSession>,
    initial: Option<Box<InitialSpace<ConnectionConfigType>>>,
    handshake: Option<Box<HandshakeSpace<ConnectionConfigType>>>,
    application: Option<Box<ApplicationSpace<ConnectionConfigType>>>,
    zero_rtt_crypto: Option<Box<<ConnectionConfigType::TLSSession as CryptoSuite>::ZeroRTTCrypto>>,
    handshake_status: HandshakeStatus,
}

macro_rules! packet_space_api {
    ($ty:ty, $field:ident, $get_mut:ident $(, $discard:ident)?) => {
        #[allow(dead_code)]
        pub fn $field(&self) -> Option<&$ty> {
            self.$field
                .as_ref()
                .map(Box::as_ref)
        }

        pub fn $get_mut(&mut self) -> Option<(&mut $ty, &mut HandshakeStatus)> {
            let space = self.$field
                .as_mut()
                .map(Box::as_mut)?;
            Some((space, &mut self.handshake_status))
        }

        $(
            pub fn $discard(&mut self, path: &mut Path<Config::CongestionController>) {
                //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.2.2
                //# When Initial or Handshake keys are discarded, the PTO and loss
                //# detection timers MUST be reset, because discarding keys indicates
                //# forward progress and the loss detection timer might have been set for
                //# a now discarded packet number space.
                path.reset_pto_backoff();
                if let Some(mut space) = self.$field.take() {
                    space.on_discard(path);
                }

                //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#4.9.1
                //# Endpoints MUST NOT send
                //# Initial packets after this point.
                // By discarding a space, we are no longer capable of sending packets with those
                // keys.

                debug_assert!(self.$field.is_none(), "space should have been discarded");
            }
        )?
    };
}

impl<Config: connection::Config> PacketSpaceManager<Config> {
    pub fn new(
        session: Config::TLSSession,
        initial: <Config::TLSSession as CryptoSuite>::InitialCrypto,
        now: Timestamp,
    ) -> Self {
        let ack_manager = AckManager::new(PacketNumberSpace::Initial, ack::Settings::EARLY);

        Self {
            session: Some(session),
            initial: Some(Box::new(InitialSpace::new(initial, now, ack_manager))),
            handshake: None,
            application: None,
            zero_rtt_crypto: None,
            handshake_status: HandshakeStatus::default(),
        }
    }

    packet_space_api!(InitialSpace<Config>, initial, initial_mut, discard_initial);

    packet_space_api!(
        HandshakeSpace<Config>,
        handshake,
        handshake_mut,
        discard_handshake
    );

    packet_space_api!(ApplicationSpace<Config>, application, application_mut);

    #[allow(dead_code)] // 0RTT hasn't been started yet
    pub fn zero_rtt_crypto(&self) -> Option<&<Config::TLSSession as CryptoSuite>::ZeroRTTCrypto> {
        self.zero_rtt_crypto.as_ref().map(Box::as_ref)
    }

    pub fn discard_zero_rtt_crypto(&mut self) {
        self.zero_rtt_crypto = None;
    }

    pub fn poll_crypto(
        &mut self,
        connection_config: &Config,
        path: &Path<Config::CongestionController>,
        local_id_registry: &mut connection::LocalIdRegistry,
        limits: &Limits,
        now: Timestamp,
    ) -> Result<(), TransportError> {
        if let Some(session) = self.session.as_mut() {
            let mut context: SessionContext<Config> = SessionContext {
                now,
                initial: &mut self.initial,
                handshake: &mut self.handshake,
                application: &mut self.application,
                zero_rtt_crypto: &mut self.zero_rtt_crypto,
                path,
                connection_config,
                handshake_status: &mut self.handshake_status,
                local_id_registry,
                limits,
            };

            session.poll(&mut context)?;

            // The TLS session is no longer needed
            if self.is_handshake_confirmed() {
                self.session = None;
            }
        }

        Ok(())
    }

    /// Returns all of the component timers
    pub fn timers(&self) -> impl Iterator<Item = &Timestamp> {
        // the spaces are `Option`s and can be iterated over, either returning
        // the value or `None`.
        core::iter::empty()
            .chain(self.initial.iter().flat_map(|space| space.timers()))
            .chain(self.handshake.iter().flat_map(|space| space.timers()))
            .chain(self.application.iter().flat_map(|space| space.timers()))
    }

    /// Called when the connection timer expired
    pub fn on_timeout(
        &mut self,
        local_id_registry: &mut connection::LocalIdRegistry,
        path_manager: &mut path::Manager<Config::CongestionController>,
        timestamp: Timestamp,
    ) {
        let path = path_manager.active_path_mut();

        // ensure the backoff doesn't grow too quickly
        let max_backoff = path.pto_backoff * 2;

        if let Some((space, handshake_status)) = self.initial_mut() {
            space.on_timeout(path, handshake_status, timestamp)
        }
        if let Some((space, handshake_status)) = self.handshake_mut() {
            space.on_timeout(path, handshake_status, timestamp)
        }
        if let Some((space, handshake_status)) = self.application_mut() {
            space.on_timeout(path_manager, handshake_status, local_id_registry, timestamp)
        }

        let path = path_manager.active_path_mut();
        path.pto_backoff = path.pto_backoff.min(max_backoff);
    }

    /// Signals the connection was previously blocked by anti-amplification limits
    /// but is now no longer limited.
    pub fn on_amplification_unblocked(
        &mut self,
        path: &Path<Config::CongestionController>,
        timestamp: Timestamp,
    ) {
        if let Some((space, handshake_status)) = self.initial_mut() {
            space.on_amplification_unblocked(path, timestamp, handshake_status.is_confirmed());
        }

        if let Some((space, handshake_status)) = self.handshake_mut() {
            space.on_amplification_unblocked(path, timestamp, handshake_status.is_confirmed());
        }

        if let Some((space, handshake_status)) = self.application_mut() {
            space.on_amplification_unblocked(path, timestamp, handshake_status.is_confirmed());
        }
    }

    pub fn requires_probe(&self) -> bool {
        core::iter::empty()
            .chain(self.initial.iter().map(|space| space.requires_probe()))
            .chain(self.handshake.iter().map(|space| space.requires_probe()))
            .chain(self.application.iter().map(|space| space.requires_probe()))
            .any(|requires_probe| requires_probe)
    }

    pub fn is_handshake_confirmed(&self) -> bool {
        self.handshake_status.is_confirmed()
    }

    pub fn is_handshake_complete(&self) -> bool {
        match Config::ENDPOINT_TYPE {
            endpoint::Type::Server => self.is_handshake_confirmed(),
            endpoint::Type::Client => {
                // TODO https://github.com/awslabs/s2n-quic/issues/338
                // Return true after the client has received the ServerFinished message
                self.is_handshake_confirmed()
            }
        }
    }
}

impl<Config: connection::Config> transmission::interest::Provider for PacketSpaceManager<Config> {
    fn transmission_interest(&self) -> transmission::Interest {
        core::iter::empty()
            .chain(
                self.initial
                    .iter()
                    .map(|space| space.transmission_interest()),
            )
            .chain(
                self.handshake
                    .iter()
                    .map(|space| space.transmission_interest()),
            )
            .chain(
                self.application
                    .iter()
                    .map(|space| space.transmission_interest()),
            )
            .chain(Some(self.handshake_status.transmission_interest()))
            .sum()
    }
}

impl<Config: connection::Config> connection::finalization::Provider for PacketSpaceManager<Config> {
    fn finalization_status(&self) -> connection::finalization::Status {
        core::iter::empty()
            .chain(self.initial.iter().map(|space| space.finalization_status()))
            .chain(
                self.handshake
                    .iter()
                    .map(|space| space.finalization_status()),
            )
            .chain(
                self.application
                    .iter()
                    .map(|space| space.finalization_status()),
            )
            .sum()
    }
}

macro_rules! default_frame_handler {
    ($name:ident, $frame:ty) => {
        fn $name(&mut self, frame: $frame) -> Result<(), TransportError> {
            Err(TransportError::PROTOCOL_VIOLATION
                .with_reason(Self::INVALID_FRAME_ERROR)
                .with_frame_type(frame.tag().into()))
        }
    };
}

pub trait PacketSpace<Config: connection::Config> {
    const INVALID_FRAME_ERROR: &'static str;

    fn handle_crypto_frame(
        &mut self,
        frame: CryptoRef,
        datagram: &DatagramInfo,
        path: &mut Path<Config::CongestionController>,
    ) -> Result<(), TransportError>;

    fn handle_ack_frame<A: AckRanges>(
        &mut self,
        frame: Ack<A>,
        datagram: &DatagramInfo,
        path_id: path::Id,
        path_manager: &mut path::Manager<Config::CongestionController>,
        handshake_status: &mut HandshakeStatus,
        local_id_registry: &mut connection::LocalIdRegistry,
    ) -> Result<(), TransportError>;

    fn handle_connection_close_frame(
        &mut self,
        frame: ConnectionClose,
        datagram: &DatagramInfo,
        path: &mut Path<Config::CongestionController>,
    ) -> Result<(), TransportError>;

    fn handle_handshake_done_frame(
        &mut self,
        frame: HandshakeDone,
        _datagram: &DatagramInfo,
        _path: &mut Path<Config::CongestionController>,
        _local_id_registry: &mut connection::LocalIdRegistry,
        _handshake_status: &mut HandshakeStatus,
    ) -> Result<(), TransportError> {
        Err(TransportError::PROTOCOL_VIOLATION
            .with_reason(Self::INVALID_FRAME_ERROR)
            .with_frame_type(frame.tag().into()))
    }

    fn handle_retire_connection_id_frame(
        &mut self,
        frame: RetireConnectionID,
        _datagram: &DatagramInfo,
        _path: &mut Path<Config::CongestionController>,
        _local_id_registry: &mut connection::LocalIdRegistry,
    ) -> Result<(), TransportError> {
        Err(TransportError::PROTOCOL_VIOLATION
            .with_reason(Self::INVALID_FRAME_ERROR)
            .with_frame_type(frame.tag().into()))
    }

    fn handle_new_connection_id_frame(
        &mut self,
        frame: NewConnectionID,
        _datagram: &DatagramInfo,
        _path_manager: &mut path::Manager<Config::CongestionController>,
    ) -> Result<(), TransportError> {
        Err(TransportError::PROTOCOL_VIOLATION
            .with_reason(Self::INVALID_FRAME_ERROR)
            .with_frame_type(frame.tag().into()))
    }

    default_frame_handler!(handle_stream_frame, StreamRef);
    default_frame_handler!(handle_data_blocked_frame, DataBlocked);
    default_frame_handler!(handle_max_data_frame, MaxData);
    default_frame_handler!(handle_max_stream_data_frame, MaxStreamData);
    default_frame_handler!(handle_max_streams_frame, MaxStreams);
    default_frame_handler!(handle_reset_stream_frame, ResetStream);
    default_frame_handler!(handle_stop_sending_frame, StopSending);
    default_frame_handler!(handle_stream_data_blocked_frame, StreamDataBlocked);
    default_frame_handler!(handle_streams_blocked_frame, StreamsBlocked);
    default_frame_handler!(handle_new_token_frame, NewToken);
    default_frame_handler!(handle_path_challenge_frame, PathChallenge);
    default_frame_handler!(handle_path_response_frame, PathResponse);

    fn on_processed_packet(
        &mut self,
        processed_packet: ProcessedPacket,
    ) -> Result<(), TransportError>;

    // TODO: Reduce arguments, https://github.com/awslabs/s2n-quic/issues/312
    #[allow(clippy::too_many_arguments)]
    fn handle_cleartext_payload<'a>(
        &mut self,
        packet_number: PacketNumber,
        mut payload: DecoderBufferMut<'a>,
        datagram: &DatagramInfo,
        path_id: path::Id,
        path_manager: &mut path::Manager<Config::CongestionController>,
        handshake_status: &mut HandshakeStatus,
        local_id_registry: &mut connection::LocalIdRegistry,
    ) -> Result<Option<frame::ConnectionClose<'a>>, TransportError> {
        use s2n_quic_core::{
            frame::{Frame, FrameMut},
            varint::VarInt,
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
                    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#19.1
                    //# A PADDING frame (type=0x00) has no semantic value.  PADDING frames
                    //# can be used to increase the size of a packet.  Padding can be used to
                    //# increase an initial client packet to the minimum required size, or to
                    //# provide protection against traffic analysis for protected packets.
                    processed_packet.on_processed_frame(&frame);
                }
                Frame::Ping(frame) => {
                    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#19.2
                    //# Endpoints can use PING frames (type=0x01) to verify that their peers
                    //# are still alive or to check reachability to the peer.
                    processed_packet.on_processed_frame(&frame);
                }
                Frame::Crypto(frame) => {
                    let on_error = with_frame_type!(frame);

                    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#7.5
                    //# Packets containing
                    //# discarded CRYPTO frames MUST be acknowledged because the packet has
                    //# been received and processed by the transport even though the CRYPTO
                    //# frame was discarded.
                    processed_packet.on_processed_frame(&frame);

                    self.handle_crypto_frame(frame.into(), datagram, &mut path_manager[path_id])
                        .map_err(on_error)?;
                }
                Frame::Ack(frame) => {
                    let on_error = with_frame_type!(frame);
                    processed_packet.on_processed_frame(&frame);
                    self.handle_ack_frame(
                        frame,
                        datagram,
                        path_id,
                        path_manager,
                        handshake_status,
                        local_id_registry,
                    )
                    .map_err(on_error)?;
                }
                Frame::ConnectionClose(frame) => {
                    let on_error = with_frame_type!(frame);
                    processed_packet.on_processed_frame(&frame);
                    self.handle_connection_close_frame(frame, datagram, &mut path_manager[path_id])
                        .map_err(on_error)?;

                    // skip processing any other frames
                    // TODO is this actually OK to do?
                    // https://github.com/awslabs/s2n-quic/issues/216
                    return Ok(Some(frame));
                }
                Frame::Stream(frame) => {
                    let on_error = with_frame_type!(frame);
                    processed_packet.on_processed_frame(&frame);
                    self.handle_stream_frame(frame.into()).map_err(on_error)?;
                }
                Frame::DataBlocked(frame) => {
                    let on_error = with_frame_type!(frame);
                    processed_packet.on_processed_frame(&frame);
                    self.handle_data_blocked_frame(frame).map_err(on_error)?;
                }
                Frame::MaxData(frame) => {
                    let on_error = with_frame_type!(frame);
                    processed_packet.on_processed_frame(&frame);
                    self.handle_max_data_frame(frame).map_err(on_error)?;
                }
                Frame::MaxStreamData(frame) => {
                    let on_error = with_frame_type!(frame);
                    processed_packet.on_processed_frame(&frame);
                    self.handle_max_stream_data_frame(frame).map_err(on_error)?;
                }
                Frame::MaxStreams(frame) => {
                    let on_error = with_frame_type!(frame);
                    processed_packet.on_processed_frame(&frame);
                    self.handle_max_streams_frame(frame).map_err(on_error)?;
                }
                Frame::ResetStream(frame) => {
                    let on_error = with_frame_type!(frame);
                    processed_packet.on_processed_frame(&frame);
                    self.handle_reset_stream_frame(frame).map_err(on_error)?;
                }
                Frame::StopSending(frame) => {
                    let on_error = with_frame_type!(frame);
                    processed_packet.on_processed_frame(&frame);
                    self.handle_stop_sending_frame(frame).map_err(on_error)?;
                }
                Frame::StreamDataBlocked(frame) => {
                    let on_error = with_frame_type!(frame);
                    processed_packet.on_processed_frame(&frame);
                    self.handle_stream_data_blocked_frame(frame)
                        .map_err(on_error)?;
                }
                Frame::StreamsBlocked(frame) => {
                    let on_error = with_frame_type!(frame);
                    processed_packet.on_processed_frame(&frame);
                    self.handle_streams_blocked_frame(frame).map_err(on_error)?;
                }
                Frame::NewToken(frame) => {
                    let on_error = with_frame_type!(frame);
                    processed_packet.on_processed_frame(&frame);
                    self.handle_new_token_frame(frame).map_err(on_error)?;
                }
                Frame::NewConnectionID(frame) => {
                    let on_error = with_frame_type!(frame);
                    processed_packet.on_processed_frame(&frame);
                    self.handle_new_connection_id_frame(frame, datagram, path_manager)
                        .map_err(on_error)?;
                }
                Frame::RetireConnectionID(frame) => {
                    let on_error = with_frame_type!(frame);
                    processed_packet.on_processed_frame(&frame);
                    self.handle_retire_connection_id_frame(
                        frame,
                        datagram,
                        &mut path_manager[path_id],
                        local_id_registry,
                    )
                    .map_err(on_error)?;
                }
                Frame::PathChallenge(frame) => {
                    let on_error = with_frame_type!(frame);
                    processed_packet.on_processed_frame(&frame);
                    self.handle_path_challenge_frame(frame).map_err(on_error)?;
                }
                Frame::PathResponse(frame) => {
                    let on_error = with_frame_type!(frame);
                    processed_packet.on_processed_frame(&frame);
                    self.handle_path_response_frame(frame).map_err(on_error)?;
                }
                Frame::HandshakeDone(frame) => {
                    let on_error = with_frame_type!(frame);
                    processed_packet.on_processed_frame(&frame);
                    self.handle_handshake_done_frame(
                        frame,
                        datagram,
                        &mut path_manager[path_id],
                        local_id_registry,
                        handshake_status,
                    )
                    .map_err(on_error)?;
                }
            }

            payload = remaining;
        }

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#9.3
        //# Receiving a packet from a new peer address containing a non-probing
        //# frame indicates that the peer has migrated to that address.

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#9.3
        //= type=TODO
        //= tracking-issue=https://github.com/awslabs/s2n-quic/issues/568
        //# If the recipient permits the migration, it MUST send subsequent
        //# packets to the new peer address and MUST initiate path validation
        //# (Section 8.2) to verify the peer's ownership of the address if
        //# validation is not already underway.

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#13.1
        //# A packet MUST NOT be acknowledged until packet protection has been
        //# successfully removed and all frames contained in the packet have been
        //# processed.  For STREAM frames, this means the data has been enqueued
        //# in preparation to be received by the application protocol, but it
        //# does not require that data is delivered and consumed.
        //#
        //# Once the packet has been fully processed, a receiver acknowledges
        //# receipt by sending one or more ACK frames containing the packet
        //# number of the received packet.

        self.on_processed_packet(processed_packet)?;

        Ok(None)
    }
}

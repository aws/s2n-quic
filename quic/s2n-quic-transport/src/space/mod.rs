// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    connection, endpoint, path, path::Path, processed_packet::ProcessedPacket,
    recovery::congestion_controller, space::rx_packet_numbers::AckManager, transmission,
};
use bytes::Bytes;
use s2n_codec::DecoderBufferMut;
use s2n_quic_core::{
    ack,
    connection::limits::Limits,
    crypto::{tls, tls::Session, CryptoSuite},
    event,
    frame::{
        ack::AckRanges,
        crypto::CryptoRef,
        path_validation::{self, Probing},
        stream::StreamRef,
        Ack, ConnectionClose, DataBlocked, HandshakeDone, MaxData, MaxStreamData, MaxStreams,
        NewConnectionId, NewToken, PathChallenge, PathResponse, ResetStream, RetireConnectionId,
        StopSending, StreamDataBlocked, StreamsBlocked,
    },
    inet::DatagramInfo,
    packet::number::{PacketNumber, PacketNumberSpace},
    random,
    time::Timestamp,
    transport,
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

pub struct PacketSpaceManager<Config: endpoint::Config> {
    session: Option<<Config::TLSEndpoint as tls::Endpoint>::Session>,
    initial: Option<Box<InitialSpace<Config>>>,
    handshake: Option<Box<HandshakeSpace<Config>>>,
    application: Option<Box<ApplicationSpace<Config>>>,
    zero_rtt_crypto:
        Option<Box<<<Config::TLSEndpoint as tls::Endpoint>::Session as CryptoSuite>::ZeroRttKey>>,
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
            pub fn $discard<Pub: event::Publisher>(
                &mut self,
                path: &mut Path<<Config::CongestionControllerEndpoint as congestion_controller::Endpoint>::CongestionController>,
        path_id: path::Id,
        publisher: &mut Pub,
            ) {
                //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.2.2
                //# When Initial or Handshake keys are discarded, the PTO and loss
                //# detection timers MUST be reset, because discarding keys indicates
                //# forward progress and the loss detection timer might have been set for
                //# a now discarded packet number space.
                path.reset_pto_backoff();
                if let Some(mut space) = self.$field.take() {
                    space.on_discard(path,  path_id, publisher);
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

impl<Config: endpoint::Config> PacketSpaceManager<Config> {
    pub fn new(
        session: <Config::TLSEndpoint as tls::Endpoint>::Session,
        initial_key: <<Config::TLSEndpoint as tls::Endpoint>::Session as CryptoSuite>::InitialKey,
        header_key: <<Config::TLSEndpoint as tls::Endpoint>::Session as CryptoSuite>::InitialHeaderKey,
        now: Timestamp,
    ) -> Self {
        let ack_manager = AckManager::new(PacketNumberSpace::Initial, ack::Settings::EARLY);

        Self {
            session: Some(session),
            initial: Some(Box::new(InitialSpace::new(
                initial_key,
                header_key,
                now,
                ack_manager,
            ))),
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
    pub fn zero_rtt_crypto(
        &self,
    ) -> Option<&<<Config::TLSEndpoint as tls::Endpoint>::Session as CryptoSuite>::ZeroRttKey> {
        self.zero_rtt_crypto.as_ref().map(Box::as_ref)
    }

    pub fn discard_zero_rtt_crypto(&mut self) {
        self.zero_rtt_crypto = None;
    }

    pub fn poll_crypto(
        &mut self,
        path: &Path<<Config::CongestionControllerEndpoint as congestion_controller::Endpoint>::CongestionController>,
        local_id_registry: &mut connection::LocalIdRegistry,
        limits: &mut Limits,
        now: Timestamp,
    ) -> Result<(), transport::Error> {
        if let Some(session) = self.session.as_mut() {
            let mut context: SessionContext<Config> = SessionContext {
                now,
                initial: &mut self.initial,
                handshake: &mut self.handshake,
                application: &mut self.application,
                zero_rtt_crypto: &mut self.zero_rtt_crypto,
                path,
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
    pub fn timers(&self) -> impl Iterator<Item = Timestamp> {
        // the spaces are `Option`s and can be iterated over, either returning
        // the value or `None`.
        core::iter::empty()
            .chain(self.initial.iter().flat_map(|space| space.timers()))
            .chain(self.handshake.iter().flat_map(|space| space.timers()))
            .chain(self.application.iter().flat_map(|space| space.timers()))
            .min()
            .into_iter()
    }

    /// Called when the connection timer expired
    pub fn on_timeout<Pub: event::Publisher>(
        &mut self,
        local_id_registry: &mut connection::LocalIdRegistry,
        path_manager: &mut path::Manager<Config::CongestionControllerEndpoint>,
        timestamp: Timestamp,
        publisher: &mut Pub,
    ) {
        let path_id = path_manager.active_path_id();
        let path = path_manager.active_path_mut();

        // ensure the backoff doesn't grow too quickly
        let max_backoff = path.pto_backoff * 2;

        if let Some((space, handshake_status)) = self.initial_mut() {
            space.on_timeout(
                handshake_status,
                path_id,
                path_manager,
                timestamp,
                publisher,
            )
        }
        if let Some((space, handshake_status)) = self.handshake_mut() {
            space.on_timeout(
                handshake_status,
                path_id,
                path_manager,
                timestamp,
                publisher,
            )
        }
        if let Some((space, handshake_status)) = self.application_mut() {
            space.on_timeout(
                path_manager,
                handshake_status,
                local_id_registry,
                timestamp,
                publisher,
            )
        }

        let path = path_manager.active_path_mut();
        path.pto_backoff = path.pto_backoff.min(max_backoff);
    }

    /// Signals the connection was previously blocked by anti-amplification limits
    /// but is now no longer limited.
    pub fn on_amplification_unblocked(
        &mut self,
        path: &Path<<Config::CongestionControllerEndpoint as congestion_controller::Endpoint>::CongestionController>,
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

    pub fn on_transmit_close(
        &mut self,
        early_connection_close: &ConnectionClose,
        connection_close: &ConnectionClose,
        context: &mut connection::ConnectionTransmissionContext<Config>,
        packet_buffer: &mut endpoint::PacketBuffer,
    ) -> Option<Bytes> {
        //= https://tools.ietf.org/id/draft-ietf-quic-transport-34.txt#10.2.3
        //# When sending CONNECTION_CLOSE, the goal is to ensure that the peer
        //# will process the frame.  Generally, this means sending the frame in a
        //# packet with the highest level of packet protection to avoid the
        //# packet being discarded.
        let mut can_send_initial = self.initial.is_some();
        let mut can_send_handshake = self.handshake.is_some();
        let can_send_application = self.application.is_some();

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-34.txt#10.2.3
        //# After the handshake is confirmed (see
        //# Section 4.1.2 of [QUIC-TLS]), an endpoint MUST send any
        //# CONNECTION_CLOSE frames in a 1-RTT packet.
        if self.is_handshake_confirmed() {
            can_send_initial = false;
            can_send_handshake = false;
            debug_assert!(
                can_send_application,
                "if the handshake is confirmed, 1rtt keys should be available"
            );
        }

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-34.txt#10.2.3
        //# A client will always know whether the server has Handshake keys
        //# (see Section 17.2.2.1), but it is possible that a server does not
        //# know whether the client has Handshake keys.  Under these
        //# circumstances, a server SHOULD send a CONNECTION_CLOSE frame in
        //# both Handshake and Initial packets to ensure that at least one of
        //# them is processable by the client.
        if can_send_handshake {
            match Config::ENDPOINT_TYPE {
                endpoint::Type::Client => {
                    // if we are a client and have handshake keys, we know the server
                    // has handshake keys as well, so no need to transmit in initial.
                    can_send_initial = false;
                }
                endpoint::Type::Server => {
                    // try to send an initial packet if the space is still available
                    //
                    // Note: this assignment isn't actually needed; it's mostly to make
                    //       the code easier to follow
                    can_send_initial &= true;
                }
            }
        }

        packet_buffer.write(|buffer| {
            macro_rules! write_packet {
                ($buffer:expr, $space:ident, $check:expr, $frame:expr) => {
                    if let Some((space, _handshake_status)) = self.$space().filter(|_| $check) {
                        let result = space.on_transmit_close(context, &$frame, $buffer);

                        match result {
                            Ok(buffer) => buffer,
                            Err(err) => err.take_buffer(),
                        }
                    } else {
                        $buffer
                    }
                };
            }

            let buffer = write_packet!(
                buffer,
                initial_mut,
                can_send_initial,
                early_connection_close
            );
            let buffer = write_packet!(
                buffer,
                handshake_mut,
                can_send_handshake,
                early_connection_close
            );
            let buffer = write_packet!(
                buffer,
                application_mut,
                can_send_application,
                connection_close
            );

            buffer
        })
    }
}

impl<Config: endpoint::Config> transmission::interest::Provider for PacketSpaceManager<Config> {
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

impl<Config: endpoint::Config> connection::finalization::Provider for PacketSpaceManager<Config> {
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
        fn $name(&mut self, frame: $frame) -> Result<(), transport::Error> {
            Err(transport::Error::PROTOCOL_VIOLATION
                .with_reason(Self::INVALID_FRAME_ERROR)
                .with_frame_type(frame.tag().into()))
        }
    };
}

pub trait PacketSpace<Config: endpoint::Config> {
    const INVALID_FRAME_ERROR: &'static str;

    fn handle_crypto_frame(
        &mut self,
        frame: CryptoRef,
        datagram: &DatagramInfo,
        path: &mut Path<<Config::CongestionControllerEndpoint as congestion_controller::Endpoint>::CongestionController>,
    ) -> Result<(), transport::Error>;

    #[allow(clippy::too_many_arguments)]
    fn handle_ack_frame<A: AckRanges, Pub: event::Publisher>(
        &mut self,
        frame: Ack<A>,
        datagram: &DatagramInfo,
        path_id: path::Id,
        path_manager: &mut path::Manager<Config::CongestionControllerEndpoint>,
        handshake_status: &mut HandshakeStatus,
        local_id_registry: &mut connection::LocalIdRegistry,
        publisher: &mut Pub,
    ) -> Result<(), transport::Error>;

    fn handle_connection_close_frame(
        &mut self,
        frame: ConnectionClose,
        datagram: &DatagramInfo,
        path: &mut Path<<Config::CongestionControllerEndpoint as congestion_controller::Endpoint>::CongestionController>,
    ) -> Result<(), transport::Error>;

    fn handle_handshake_done_frame(
        &mut self,
        frame: HandshakeDone,
        _datagram: &DatagramInfo,
        _path: &mut Path<<Config::CongestionControllerEndpoint as congestion_controller::Endpoint>::CongestionController>,
        _local_id_registry: &mut connection::LocalIdRegistry,
        _handshake_status: &mut HandshakeStatus,
    ) -> Result<(), transport::Error> {
        Err(transport::Error::PROTOCOL_VIOLATION
            .with_reason(Self::INVALID_FRAME_ERROR)
            .with_frame_type(frame.tag().into()))
    }

    fn handle_retire_connection_id_frame(
        &mut self,
        frame: RetireConnectionId,
        _datagram: &DatagramInfo,
        _path: &mut Path<<Config::CongestionControllerEndpoint as congestion_controller::Endpoint>::CongestionController>,
        _local_id_registry: &mut connection::LocalIdRegistry,
    ) -> Result<(), transport::Error> {
        Err(transport::Error::PROTOCOL_VIOLATION
            .with_reason(Self::INVALID_FRAME_ERROR)
            .with_frame_type(frame.tag().into()))
    }

    fn handle_new_connection_id_frame(
        &mut self,
        frame: NewConnectionId,
        _datagram: &DatagramInfo,
        _path_manager: &mut path::Manager<Config::CongestionControllerEndpoint>,
    ) -> Result<(), transport::Error> {
        Err(transport::Error::PROTOCOL_VIOLATION
            .with_reason(Self::INVALID_FRAME_ERROR)
            .with_frame_type(frame.tag().into()))
    }

    fn handle_path_response_frame(
        &mut self,
        frame: PathResponse,
        _path_manager: &mut path::Manager<Config::CongestionControllerEndpoint>,
    ) -> Result<(), transport::Error> {
        Err(transport::Error::PROTOCOL_VIOLATION
            .with_reason(Self::INVALID_FRAME_ERROR)
            .with_frame_type(frame.tag().into()))
    }

    fn handle_path_challenge_frame(
        &mut self,
        frame: PathChallenge,
        _datagram: &DatagramInfo,
        _path_manager: &mut path::Manager<Config::CongestionControllerEndpoint>,
    ) -> Result<(), transport::Error> {
        Err(transport::Error::PROTOCOL_VIOLATION
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

    fn on_processed_packet(
        &mut self,
        processed_packet: ProcessedPacket,
    ) -> Result<(), transport::Error>;

    // TODO: Reduce arguments, https://github.com/awslabs/s2n-quic/issues/312
    #[allow(clippy::too_many_arguments)]
    fn handle_cleartext_payload<'a, Rnd: random::Generator, Pub: event::Publisher>(
        &mut self,
        packet_number: PacketNumber,
        mut payload: DecoderBufferMut<'a>,
        datagram: &DatagramInfo,
        path_id: path::Id,
        path_manager: &mut path::Manager<Config::CongestionControllerEndpoint>,
        handshake_status: &mut HandshakeStatus,
        local_id_registry: &mut connection::LocalIdRegistry,
        random_generator: &mut Rnd,
        publisher: &mut Pub,
    ) -> Result<(), connection::Error> {
        use s2n_quic_core::{
            frame::{Frame, FrameMut},
            varint::VarInt,
        };

        let mut processed_packet = ProcessedPacket::new(packet_number, datagram);

        macro_rules! with_frame_type {
            ($frame:ident) => {{
                let frame_type = $frame.tag();
                move |err: transport::Error| err.with_frame_type(VarInt::from_u8(frame_type))
            }};
        }

        let mut is_path_validation_probing = path_validation::Probe::Probing;
        while !payload.is_empty() {
            let (frame, remaining) = payload
                .decode::<FrameMut>()
                .map_err(transport::Error::from)?;
            is_path_validation_probing |= frame.path_validation();

            publisher.on_frame_received(event::builders::FrameReceived {
                packet_header: event::builders::PacketHeader {
                    packet_type: packet_number.space().into(),
                    packet_number: packet_number.as_u64(),
                    version: publisher.quic_version(),
                }
                .into(),
                path_id: path_id.as_u8() as u64,
                frame: frame.as_event(),
            });
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
                        publisher,
                    )
                    .map_err(on_error)?;
                }
                Frame::ConnectionClose(frame) => {
                    let on_error = with_frame_type!(frame);
                    processed_packet.on_processed_frame(&frame);
                    self.handle_connection_close_frame(frame, datagram, &mut path_manager[path_id])
                        .map_err(on_error)?;

                    // skip processing any other frames and return an error
                    return Err(frame.into());
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
                Frame::NewConnectionId(frame) => {
                    let on_error = with_frame_type!(frame);
                    processed_packet.on_processed_frame(&frame);
                    self.handle_new_connection_id_frame(frame, datagram, path_manager)
                        .map_err(on_error)?;
                }
                Frame::RetireConnectionId(frame) => {
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

                    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#9.3.3
                    //# An endpoint that receives a PATH_CHALLENGE on an active path SHOULD
                    //# send a non-probing packet in response.
                    if path_manager.active_path_id() == path_id {
                        processed_packet.path_challenge_on_active_path = true;
                    }
                    self.handle_path_challenge_frame(frame, datagram, path_manager)
                        .map_err(on_error)?;
                }
                Frame::PathResponse(frame) => {
                    let on_error = with_frame_type!(frame);
                    processed_packet.on_processed_frame(&frame);

                    self.handle_path_response_frame(frame, path_manager)
                        .map_err(on_error)?;
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
        if is_path_validation_probing.is_probing() {
            path_manager.on_non_path_validation_probing_packet(
                path_id,
                random_generator,
                publisher,
            )?;
        }

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

        Ok(())
    }
}

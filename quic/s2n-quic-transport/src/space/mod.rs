// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    ack,
    ack::AckManager,
    connection, endpoint, path,
    path::{path_event, Path},
    processed_packet::ProcessedPacket,
    transmission,
};
use bytes::Bytes;
use core::{
    fmt,
    task::{Poll, Waker},
};
use s2n_codec::DecoderBufferMut;
use s2n_quic_core::{
    application::ServerName,
    connection::{limits::Limits, InitialId, PeerId},
    crypto::{tls, tls::Session, CryptoSuite, Key},
    event::{self, IntoEvent},
    frame::{
        ack::AckRanges, crypto::CryptoRef, datagram::DatagramRef, stream::StreamRef, Ack,
        ConnectionClose, DataBlocked, HandshakeDone, MaxData, MaxStreamData, MaxStreams,
        NewConnectionId, NewToken, PathChallenge, PathResponse, ResetStream, RetireConnectionId,
        StopSending, StreamDataBlocked, StreamsBlocked,
    },
    inet::DatagramInfo,
    packet::number::{PacketNumber, PacketNumberSpace},
    time::{timer, Timestamp},
    transport,
};

mod application;
mod crypto_stream;
pub(crate) mod datagram;
mod handshake;
mod handshake_status;
mod initial;
mod keep_alive;
mod session_context;
mod tx_packet_numbers;

pub(crate) use application::ApplicationSpace;
pub(crate) use crypto_stream::CryptoStream;
pub(crate) use handshake::HandshakeSpace;
pub(crate) use handshake_status::HandshakeStatus;
pub(crate) use initial::InitialSpace;
pub(crate) use session_context::SessionContext;
pub(crate) use tx_packet_numbers::TxPacketNumbers;

struct SessionInfo<Config: endpoint::Config> {
    session: <Config::TLSEndpoint as tls::Endpoint>::Session,
    initial_cid: InitialId,
}

pub struct PacketSpaceManager<Config: endpoint::Config> {
    session_info: Option<SessionInfo<Config>>,
    retry_cid: Option<Box<PeerId>>,
    initial: Option<Box<InitialSpace<Config>>>,
    handshake: Option<Box<HandshakeSpace<Config>>>,
    application: Option<Box<ApplicationSpace<Config>>>,
    zero_rtt_crypto:
        Option<Box<<<Config::TLSEndpoint as tls::Endpoint>::Session as CryptoSuite>::ZeroRttKey>>,
    handshake_status: HandshakeStatus,
    /// Server Name Indication
    pub server_name: Option<ServerName>,
    //= https://www.rfc-editor.org/rfc/rfc9000#section-7
    //# Endpoints MUST explicitly negotiate an application protocol.

    //= https://www.rfc-editor.org/rfc/rfc9001#section-8.1
    //# Unless
    //# another mechanism is used for agreeing on an application protocol,
    //# endpoints MUST use ALPN for this purpose.
    pub application_protocol: Bytes,
}

impl<Config: endpoint::Config> fmt::Debug for PacketSpaceManager<Config> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PacketSpaceManager")
            .field("initial", &self.initial)
            .field("handshake", &self.handshake)
            .field("application", &self.application)
            .field("handshake_status", &self.handshake_status)
            .finish()
    }
}

macro_rules! packet_space_api {
    ($ty:ty, $field:ident, $get_mut:ident $(, $discard:ident $(, $assert_discard:ident)? )?) => {
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
            pub fn $discard<Pub: event::ConnectionPublisher>(
                &mut self,
                path: &mut Path<Config>,
                path_id: path::Id,
                publisher: &mut Pub,
            ) {
                //= https://www.rfc-editor.org/rfc/rfc9002#section-6.2.2
                //# When Initial or Handshake keys are discarded, the PTO and loss
                //# detection timers MUST be reset, because discarding keys indicates
                //# forward progress and the loss detection timer might have been set for
                //# a now discarded packet number space.
                path.reset_pto_backoff();
                if let Some(mut space) = self.$field.take() {
                    space.on_discard(path, path_id, publisher);
                }

                //= https://www.rfc-editor.org/rfc/rfc9001#section-4.9.1
                //# Endpoints MUST NOT send
                //# Initial packets after this point.
                // By discarding a space, we are no longer capable of sending packets with those
                // keys.

                debug_assert!(self.$field.is_none(), "space should have been discarded");
                $(
                    debug_assert!(self.$assert_discard.is_none(), "space should have been discarded");
                )?
            }
        )?
    };
}

impl<Config: endpoint::Config> PacketSpaceManager<Config> {
    pub fn new<Pub: event::ConnectionPublisher>(
        initial_cid: InitialId,
        session: <Config::TLSEndpoint as tls::Endpoint>::Session,
        initial_key: <<Config::TLSEndpoint as tls::Endpoint>::Session as CryptoSuite>::InitialKey,
        header_key: <<Config::TLSEndpoint as tls::Endpoint>::Session as CryptoSuite>::InitialHeaderKey,
        now: Timestamp,
        publisher: &mut Pub,
    ) -> Self {
        let ack_manager = AckManager::new(PacketNumberSpace::Initial, ack::Settings::EARLY);

        publisher.on_key_update(event::builder::KeyUpdate {
            key_type: event::builder::KeyType::Initial,
            cipher_suite: initial_key.cipher_suite().into_event(),
        });
        Self {
            session_info: Some(SessionInfo {
                session,
                initial_cid,
            }),
            retry_cid: None,
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
            server_name: None,
            application_protocol: Bytes::new(),
        }
    }

    packet_space_api!(InitialSpace<Config>, initial, initial_mut, discard_initial);

    packet_space_api!(
        HandshakeSpace<Config>,
        handshake,
        handshake_mut,
        discard_handshake,
        initial
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

    #[allow(clippy::too_many_arguments)]
    pub fn poll_crypto<Pub: event::ConnectionPublisher>(
        &mut self,
        path_manager: &mut path::Manager<Config>,
        local_id_registry: &mut connection::LocalIdRegistry,
        limits: &mut Limits,
        now: Timestamp,
        waker: &Waker,
        publisher: &mut Pub,
        datagram: &mut Config::DatagramEndpoint,
    ) -> Poll<Result<(), transport::Error>> {
        if let Some(session_info) = self.session_info.as_mut() {
            let mut context: SessionContext<Config, Pub> = SessionContext {
                now,
                initial_cid: &session_info.initial_cid,
                retry_cid: self.retry_cid.as_deref(),
                initial: &mut self.initial,
                handshake: &mut self.handshake,
                application: &mut self.application,
                zero_rtt_crypto: &mut self.zero_rtt_crypto,
                path_manager,
                handshake_status: &mut self.handshake_status,
                local_id_registry,
                limits,
                server_name: &mut self.server_name,
                application_protocol: &mut self.application_protocol,
                waker,
                publisher,
                datagram,
            };

            match session_info.session.poll(&mut context)? {
                Poll::Ready(_success) => {
                    // The TLS session and retry_cid is no longer needed
                    self.session_info = None;
                    self.retry_cid = None;
                }
                Poll::Pending => return Poll::Pending,
            };
        }

        Poll::Ready(Ok(()))
    }

    /// Called when the connection timer expired
    pub fn on_timeout<Pub: event::ConnectionPublisher>(
        &mut self,
        local_id_registry: &mut connection::LocalIdRegistry,
        path_manager: &mut path::Manager<Config>,
        random_generator: &mut Config::RandomGenerator,
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
                random_generator,
                timestamp,
                publisher,
            )
        }
        if let Some((space, handshake_status)) = self.handshake_mut() {
            space.on_timeout(
                handshake_status,
                path_id,
                path_manager,
                random_generator,
                timestamp,
                publisher,
            )
        }
        if let Some((space, handshake_status)) = self.application_mut() {
            space.on_timeout(
                path_manager,
                handshake_status,
                local_id_registry,
                random_generator,
                timestamp,
                publisher,
            )
        }

        let path = path_manager.active_path_mut();
        path.pto_backoff = path.pto_backoff.min(max_backoff);
    }

    /// Signals the connection was previously blocked by anti-amplification limits
    /// but is now no longer limited.
    pub fn on_amplification_unblocked(&mut self, path: &Path<Config>, timestamp: Timestamp) {
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
        self.handshake_status.is_complete()
    }

    pub(crate) fn on_transmit_close(
        &mut self,
        early_connection_close: &ConnectionClose,
        connection_close: &ConnectionClose,
        context: &mut connection::ConnectionTransmissionContext<Config>,
        packet_buffer: &mut endpoint::PacketBuffer,
    ) -> Option<Bytes> {
        //= https://www.rfc-editor.org/rfc/rfc9000#section-10.2.3
        //# When sending a CONNECTION_CLOSE frame, the goal is to ensure that the
        //# peer will process the frame.  Generally, this means sending the frame
        //# in a packet with the highest level of packet protection to avoid the
        //# packet being discarded.
        let mut can_send_initial = self.initial.is_some();
        let mut can_send_handshake = self.handshake.is_some();
        let can_send_application = self.application.is_some();

        //= https://www.rfc-editor.org/rfc/rfc9000#section-10.2.3
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

        //= https://www.rfc-editor.org/rfc/rfc9000#section-10.2.3
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
                            Ok((outcome, buffer)) => {
                                *context.outcome += outcome;
                                buffer
                            }
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

    pub fn close<Pub: event::ConnectionPublisher>(
        &mut self,
        error: connection::Error,
        path: &mut path::Path<Config>,
        path_id: path::Id,
        publisher: &mut Pub,
    ) {
        self.session_info = None;
        self.retry_cid = None;
        self.discard_initial(path, path_id, publisher);
        self.discard_handshake(path, path_id, publisher);
        self.discard_zero_rtt_crypto();

        // Don't discard the application space until the application has read the error
        if let Some((application, _handshake_status)) = self.application_mut() {
            //= https://www.rfc-editor.org/rfc/rfc9000#section-10.2
            //# A CONNECTION_CLOSE frame
            //# causes all streams to immediately become closed; open streams can be
            //# assumed to be implicitly reset.

            // Close all streams with the derived error
            application.stream_manager.close(error);
        }
    }

    pub fn on_retry_packet(&mut self, retry_source_connection_id: PeerId) {
        debug_assert!(Config::ENDPOINT_TYPE.is_client());
        self.retry_cid = Some(Box::new(retry_source_connection_id));
    }

    pub fn retry_cid(&self) -> Option<&PeerId> {
        self.retry_cid.as_deref()
    }

    pub fn on_pending_ack_ranges<Pub: event::ConnectionPublisher>(
        &mut self,
        timestamp: Timestamp,
        path_id: path::Id,
        path_manager: &mut path::Manager<Config>,
        local_id_registry: &mut connection::LocalIdRegistry,
        random_generator: &mut Config::RandomGenerator,
        publisher: &mut Pub,
    ) -> Result<(), transport::Error> {
        debug_assert!(
            self.application().is_some(),
            "application space should exists since delay ACK processing is only enabled\
            post handshake complete and connection indicated ACK interest"
        );
        debug_assert!(
            !self.application().unwrap().pending_ack_ranges.is_empty(),
            "pending_ack_ranges should be non-empty since connection indicated ACK interest"
        );

        if let Some((space, handshake_status)) = self.application_mut() {
            space.on_pending_ack_ranges(
                timestamp,
                path_id,
                path_manager,
                handshake_status,
                local_id_registry,
                random_generator,
                publisher,
            )?;
        }

        Ok(())
    }
}

impl<Config: endpoint::Config> ack::interest::Provider for PacketSpaceManager<Config> {
    #[inline]
    fn ack_interest<Q: ack::interest::Query>(&self, query: &mut Q) -> ack::interest::Result {
        if let Some(space) = self.application() {
            if !space.pending_ack_ranges.is_empty() {
                return query.on_interest(ack::interest::Interest::Immediate);
            }
        }
        query.on_interest(ack::interest::Interest::None)
    }
}

impl<Config: endpoint::Config> timer::Provider for PacketSpaceManager<Config> {
    #[inline]
    fn timers<Q: timer::Query>(&self, query: &mut Q) -> timer::Result {
        //= https://www.rfc-editor.org/rfc/rfc9002#section-6.2.1
        //# When ack-eliciting packets in multiple packet number spaces are in
        //# flight, the timer MUST be set to the earlier value of the Initial and
        //# Handshake packet number spaces.

        if let Some(space) = self.application.as_ref() {
            space.timers(query)?;
        }
        if let Some(space) = self.handshake.as_ref() {
            space.timers(query)?;
        }
        if let Some(space) = self.initial.as_ref() {
            space.timers(query)?;
        }
        Ok(())
    }
}

impl<Config: endpoint::Config> transmission::interest::Provider for PacketSpaceManager<Config> {
    #[inline]
    fn transmission_interest<Q: transmission::interest::Query>(
        &self,
        query: &mut Q,
    ) -> transmission::interest::Result {
        if let Some(space) = self.application.as_ref() {
            space.transmission_interest(query)?;
        }

        self.handshake_status.transmission_interest(query)?;

        if let Some(space) = self.initial.as_ref() {
            space.transmission_interest(query)?;
        }

        if let Some(space) = self.handshake.as_ref() {
            space.transmission_interest(query)?;
        }

        Ok(())
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

    fn handle_crypto_frame<Pub: event::ConnectionPublisher>(
        &mut self,
        frame: CryptoRef,
        datagram: &DatagramInfo,
        path: &mut Path<Config>,
        publisher: &mut Pub,
    ) -> Result<(), transport::Error>;

    #[allow(clippy::too_many_arguments)]
    fn handle_ack_frame<A: AckRanges, Pub: event::ConnectionPublisher>(
        &mut self,
        frame: Ack<A>,
        timestamp: Timestamp,
        path_id: path::Id,
        path_manager: &mut path::Manager<Config>,
        handshake_status: &mut HandshakeStatus,
        local_id_registry: &mut connection::LocalIdRegistry,
        random_generator: &mut Config::RandomGenerator,
        publisher: &mut Pub,
    ) -> Result<(), transport::Error>;

    fn handle_connection_close_frame(
        &mut self,
        frame: ConnectionClose,
        timestamp: Timestamp,
        path: &mut Path<Config>,
    ) -> Result<(), transport::Error>;

    fn handle_handshake_done_frame<Pub: event::ConnectionPublisher>(
        &mut self,
        frame: HandshakeDone,
        _timestamp: Timestamp,
        _path: &mut Path<Config>,
        _local_id_registry: &mut connection::LocalIdRegistry,
        _handshake_status: &mut HandshakeStatus,
        _publisher: &mut Pub,
    ) -> Result<(), transport::Error> {
        Err(transport::Error::PROTOCOL_VIOLATION
            .with_reason(Self::INVALID_FRAME_ERROR)
            .with_frame_type(frame.tag().into()))
    }

    fn handle_retire_connection_id_frame(
        &mut self,
        frame: RetireConnectionId,
        _datagram: &DatagramInfo,
        _path: &mut Path<Config>,
        _local_id_registry: &mut connection::LocalIdRegistry,
    ) -> Result<(), transport::Error> {
        Err(transport::Error::PROTOCOL_VIOLATION
            .with_reason(Self::INVALID_FRAME_ERROR)
            .with_frame_type(frame.tag().into()))
    }

    fn handle_new_connection_id_frame<Pub: event::ConnectionPublisher>(
        &mut self,
        frame: NewConnectionId,
        _datagram: &DatagramInfo,
        _path_manager: &mut path::Manager<Config>,
        _publisher: &mut Pub,
    ) -> Result<(), transport::Error> {
        Err(transport::Error::PROTOCOL_VIOLATION
            .with_reason(Self::INVALID_FRAME_ERROR)
            .with_frame_type(frame.tag().into()))
    }

    fn handle_path_response_frame<Pub: event::ConnectionPublisher>(
        &mut self,
        frame: PathResponse,
        _path_manager: &mut path::Manager<Config>,
        _publisher: &mut Pub,
    ) -> Result<(), transport::Error> {
        Err(transport::Error::PROTOCOL_VIOLATION
            .with_reason(Self::INVALID_FRAME_ERROR)
            .with_frame_type(frame.tag().into()))
    }

    fn handle_path_challenge_frame(
        &mut self,
        frame: PathChallenge,
        _path_id: path::Id,
        _path_manager: &mut path::Manager<Config>,
    ) -> Result<(), transport::Error> {
        Err(transport::Error::PROTOCOL_VIOLATION
            .with_reason(Self::INVALID_FRAME_ERROR)
            .with_frame_type(frame.tag().into()))
    }

    fn handle_stream_frame(
        &mut self,
        frame: StreamRef,
        _packet: &mut ProcessedPacket,
    ) -> Result<(), transport::Error> {
        Err(transport::Error::PROTOCOL_VIOLATION
            .with_reason(Self::INVALID_FRAME_ERROR)
            .with_frame_type(frame.tag().into()))
    }

    fn handle_datagram_frame(&mut self, frame: DatagramRef) -> Result<(), transport::Error> {
        Err(transport::Error::PROTOCOL_VIOLATION
            .with_reason(Self::INVALID_FRAME_ERROR)
            .with_frame_type(frame.tag().into()))
    }

    default_frame_handler!(handle_data_blocked_frame, DataBlocked);
    default_frame_handler!(handle_max_data_frame, MaxData);
    default_frame_handler!(handle_max_stream_data_frame, MaxStreamData);
    default_frame_handler!(handle_max_streams_frame, MaxStreams);
    default_frame_handler!(handle_reset_stream_frame, ResetStream);
    default_frame_handler!(handle_stop_sending_frame, StopSending);
    default_frame_handler!(handle_stream_data_blocked_frame, StreamDataBlocked);
    default_frame_handler!(handle_streams_blocked_frame, StreamsBlocked);
    default_frame_handler!(handle_new_token_frame, NewToken);

    fn on_processed_packet<Pub: event::ConnectionPublisher>(
        &mut self,
        processed_packet: ProcessedPacket,
        path_id: path::Id,
        path: &Path<Config>,
        publisher: &mut Pub,
    ) -> Result<(), transport::Error>;

    // TODO: Reduce arguments, https://github.com/aws/s2n-quic/issues/312
    #[allow(clippy::too_many_arguments)]
    fn handle_cleartext_payload<'a, Pub: event::ConnectionPublisher>(
        &mut self,
        packet_number: PacketNumber,
        payload: DecoderBufferMut<'a>,
        datagram: &'a DatagramInfo,
        path_id: path::Id,
        path_manager: &mut path::Manager<Config>,
        handshake_status: &mut HandshakeStatus,
        local_id_registry: &mut connection::LocalIdRegistry,
        random_generator: &mut Config::RandomGenerator,
        publisher: &mut Pub,
        packet_interceptor: &mut Config::PacketInterceptor,
    ) -> Result<ProcessedPacket<'a>, connection::Error> {
        use s2n_quic_core::{
            frame::{Frame, FrameMut},
            varint::VarInt,
        };

        let mut payload = {
            use s2n_quic_core::packet::interceptor::{Interceptor, Packet};

            // intercept the payload after it is decrypted, but before we process the frames
            packet_interceptor.intercept_rx_payload(
                publisher.subject(),
                Packet {
                    number: packet_number,
                    timestamp: datagram.timestamp,
                },
                payload,
            )
        };

        let mut processed_packet = ProcessedPacket::new(packet_number, datagram);

        macro_rules! on_frame_processed {
            ($frame:ident) => {{
                let frame_type = $frame.tag();
                processed_packet.on_processed_frame(&$frame);
                move |err: transport::Error| err.with_frame_type(VarInt::from_u8(frame_type))
            }};
        }

        while !payload.is_empty() {
            let (frame, remaining) = payload
                .decode::<FrameMut>()
                .map_err(transport::Error::from)?;

            let path = &path_manager[path_id];
            publisher.on_frame_received(event::builder::FrameReceived {
                packet_header: event::builder::PacketHeader::new(
                    packet_number,
                    publisher.quic_version(),
                ),
                path: path_event!(path, path_id),
                frame: frame.into_event(),
            });
            match frame {
                Frame::Padding(frame) => {
                    //= https://www.rfc-editor.org/rfc/rfc9000#section-19.1
                    //# A PADDING frame (type=0x00) has no semantic value.  PADDING frames
                    //# can be used to increase the size of a packet.  Padding can be used to
                    //# increase an Initial packet to the minimum required size or to provide
                    //# protection against traffic analysis for protected packets.
                    let _ = on_frame_processed!(frame);
                }
                Frame::Ping(frame) => {
                    //= https://www.rfc-editor.org/rfc/rfc9000#section-19.2
                    //# Endpoints can use PING frames (type=0x01) to verify that their peers
                    //# are still alive or to check reachability to the peer.
                    let _ = on_frame_processed!(frame);
                }
                Frame::Crypto(frame) => {
                    let on_error = on_frame_processed!(frame);

                    //= https://www.rfc-editor.org/rfc/rfc9000#section-7.5
                    //# Packets containing
                    //# discarded CRYPTO frames MUST be acknowledged because the packet has
                    //# been received and processed by the transport even though the CRYPTO
                    //# frame was discarded.

                    self.handle_crypto_frame(
                        frame.into(),
                        datagram,
                        &mut path_manager[path_id],
                        publisher,
                    )
                    .map_err(on_error)?;
                }
                Frame::Ack(frame) => {
                    let on_error = on_frame_processed!(frame);
                    self.handle_ack_frame(
                        frame,
                        datagram.timestamp,
                        path_id,
                        path_manager,
                        handshake_status,
                        local_id_registry,
                        random_generator,
                        publisher,
                    )
                    .map_err(on_error)?;
                }
                Frame::ConnectionClose(frame) => {
                    let on_error = on_frame_processed!(frame);
                    self.handle_connection_close_frame(
                        frame,
                        datagram.timestamp,
                        &mut path_manager[path_id],
                    )
                    .map_err(on_error)?;

                    // skip processing any other frames and return an error
                    return Err(frame.into());
                }
                Frame::Stream(frame) => {
                    let on_error = on_frame_processed!(frame);
                    self.handle_stream_frame(frame.into(), &mut processed_packet)
                        .map_err(on_error)?;
                }
                Frame::Datagram(frame) => {
                    let on_error = on_frame_processed!(frame);
                    self.handle_datagram_frame(frame.into()).map_err(on_error)?;
                }
                Frame::DataBlocked(frame) => {
                    let on_error = on_frame_processed!(frame);
                    self.handle_data_blocked_frame(frame).map_err(on_error)?;
                }
                Frame::MaxData(frame) => {
                    let on_error = on_frame_processed!(frame);
                    self.handle_max_data_frame(frame).map_err(on_error)?;
                }
                Frame::MaxStreamData(frame) => {
                    let on_error = on_frame_processed!(frame);
                    self.handle_max_stream_data_frame(frame).map_err(on_error)?;
                }
                Frame::MaxStreams(frame) => {
                    let on_error = on_frame_processed!(frame);
                    self.handle_max_streams_frame(frame).map_err(on_error)?;
                }
                Frame::ResetStream(frame) => {
                    let on_error = on_frame_processed!(frame);
                    self.handle_reset_stream_frame(frame).map_err(on_error)?;
                }
                Frame::StopSending(frame) => {
                    let on_error = on_frame_processed!(frame);
                    self.handle_stop_sending_frame(frame).map_err(on_error)?;
                }
                Frame::StreamDataBlocked(frame) => {
                    let on_error = on_frame_processed!(frame);
                    self.handle_stream_data_blocked_frame(frame)
                        .map_err(on_error)?;
                }
                Frame::StreamsBlocked(frame) => {
                    let on_error = on_frame_processed!(frame);
                    self.handle_streams_blocked_frame(frame).map_err(on_error)?;
                }
                Frame::NewToken(frame) => {
                    let on_error = on_frame_processed!(frame);
                    self.handle_new_token_frame(frame).map_err(on_error)?;
                }
                Frame::NewConnectionId(frame) => {
                    let on_error = on_frame_processed!(frame);
                    self.handle_new_connection_id_frame(frame, datagram, path_manager, publisher)
                        .map_err(on_error)?;
                }
                Frame::RetireConnectionId(frame) => {
                    let on_error = on_frame_processed!(frame);
                    self.handle_retire_connection_id_frame(
                        frame,
                        datagram,
                        &mut path_manager[path_id],
                        local_id_registry,
                    )
                    .map_err(on_error)?;
                }
                Frame::PathChallenge(frame) => {
                    let on_error = on_frame_processed!(frame);

                    //= https://www.rfc-editor.org/rfc/rfc9000#section-9.3.3
                    //# An endpoint that receives a PATH_CHALLENGE on an active path SHOULD
                    //# send a non-probing packet in response.
                    if path_manager.active_path_id() == path_id {
                        processed_packet.path_challenge_on_active_path = true;
                    }
                    self.handle_path_challenge_frame(frame, path_id, path_manager)
                        .map_err(on_error)?;
                }
                Frame::PathResponse(frame) => {
                    let on_error = on_frame_processed!(frame);

                    self.handle_path_response_frame(frame, path_manager, publisher)
                        .map_err(on_error)?;
                }
                Frame::HandshakeDone(frame) => {
                    let on_error = on_frame_processed!(frame);
                    self.handle_handshake_done_frame(
                        frame,
                        datagram.timestamp,
                        &mut path_manager[path_id],
                        local_id_registry,
                        handshake_status,
                        publisher,
                    )
                    .map_err(on_error)?;
                }
            }

            payload = remaining;
        }

        //= https://www.rfc-editor.org/rfc/rfc9000#section-12.4
        //# The payload of a packet that contains frames MUST contain at least
        //# one frame, and MAY contain multiple frames and multiple frame types.
        //# An endpoint MUST treat receipt of a packet containing no frames as a
        //# connection error of type PROTOCOL_VIOLATION.
        if processed_packet.frames == 0 {
            return Err(transport::Error::PROTOCOL_VIOLATION
                .with_reason("packet contained no frames")
                .into());
        }

        path_manager.on_processed_packet(
            path_id,
            datagram.source_connection_id,
            processed_packet.path_validation_probing,
            random_generator,
            publisher,
        )?;

        //= https://www.rfc-editor.org/rfc/rfc9000#section-13.1
        //# A packet MUST NOT be acknowledged until packet protection has been
        //# successfully removed and all frames contained in the packet have been
        //# processed.  For STREAM frames, this means the data has been enqueued
        //# in preparation to be received by the application protocol, but it
        //# does not require that data be delivered and consumed.
        //#
        //# Once the packet has been fully processed, a receiver acknowledges
        //# receipt by sending one or more ACK frames containing the packet
        //# number of the received packet.

        self.on_processed_packet(processed_packet, path_id, &path_manager[path_id], publisher)?;

        Ok(processed_packet)
    }
}

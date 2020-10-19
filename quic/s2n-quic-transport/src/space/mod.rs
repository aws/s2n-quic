use crate::{
    connection::{self, ConnectionInterests},
    frame_exchange_interests::FrameExchangeInterestProvider,
    processed_packet::ProcessedPacket,
    space::rx_packet_numbers::{AckManager, DEFAULT_ACK_RANGES_LIMIT},
};
use s2n_codec::DecoderBufferMut;
use s2n_quic_core::{
    crypto::{tls::Session as TLSSession, CryptoSuite},
    frame::{
        self, ack::AckRanges, crypto::CryptoRef, stream::StreamRef, Ack, DataBlocked,
        HandshakeDone, MaxData, MaxStreamData, MaxStreams, NewConnectionID, NewToken,
        PathChallenge, PathResponse, ResetStream, RetireConnectionID, StopSending,
        StreamDataBlocked, StreamsBlocked,
    },
    inet::DatagramInfo,
    packet::number::{PacketNumber, PacketNumberSpace},
    path::Path,
    recovery::loss_info::LossInfo,
    time::Timestamp,
    transport::error::TransportError,
};

mod application;
mod application_transmission;
mod crypto_stream;
mod early_transmission;
mod handshake;
mod handshake_status;
mod initial;
pub(crate) mod rx_packet_numbers;
mod session_context;
mod tx_packet_numbers;

pub(crate) use application::ApplicationSpace;
pub(crate) use application_transmission::ApplicationTransmission;
pub(crate) use crypto_stream::CryptoStream;
pub(crate) use early_transmission::EarlyTransmission;
pub(crate) use handshake::HandshakeSpace;
pub(crate) use handshake_status::HandshakeStatus;
pub(crate) use initial::InitialSpace;
pub(crate) use rx_packet_numbers::EARLY_ACK_SETTINGS;
pub(crate) use session_context::SessionContext;
pub(crate) use tx_packet_numbers::TxPacketNumbers;

pub const INITIAL_PTO_BACKOFF: u32 = 1;

pub struct PacketSpaceManager<ConnectionConfigType: connection::Config> {
    session: Option<ConnectionConfigType::TLSSession>,
    initial: Option<Box<InitialSpace<ConnectionConfigType>>>,
    handshake: Option<Box<HandshakeSpace<ConnectionConfigType>>>,
    application: Option<Box<ApplicationSpace<ConnectionConfigType>>>,
    zero_rtt_crypto: Option<Box<<ConnectionConfigType::TLSSession as CryptoSuite>::ZeroRTTCrypto>>,
    pto_backoff: u32,
}

macro_rules! packet_space_api {
    ($ty:ty, $get:ident, $get_mut:ident $(, $discard:ident)?) => {
        pub fn $get(&self) -> Option<&$ty> {
            self.$get
                .as_ref()
                .map(Box::as_ref)
        }

        pub fn $get_mut(&mut self) -> Option<&mut $ty> {
            self.$get
                .as_mut()
                .map(Box::as_mut)
        }

        $(
            pub fn $discard(&mut self, path: &mut Path<Config::CongestionController>) {
                //= https://tools.ietf.org/id/draft-ietf-quic-recovery-30.txt#6.2.2
                //# When Initial or Handshake keys are discarded, the PTO and loss
                //# detection timers MUST be reset, because discarding keys indicates
                //# forward progress and the loss detection timer might have been set for
                //# a now discarded packet number space.
                self.pto_backoff = INITIAL_PTO_BACKOFF;
                self.$get_mut().iter_mut().for_each(|space| space.on_discard(path));
                self.$get = None;
            }
        )?
    };
}

impl<Config: connection::Config> PacketSpaceManager<Config> {
    packet_space_api!(InitialSpace<Config>, initial, initial_mut, discard_initial);

    packet_space_api!(
        HandshakeSpace<Config>,
        handshake,
        handshake_mut,
        discard_handshake
    );

    packet_space_api!(ApplicationSpace<Config>, application, application_mut);

    pub fn zero_rtt_crypto(&self) -> Option<&<Config::TLSSession as CryptoSuite>::ZeroRTTCrypto> {
        self.zero_rtt_crypto.as_ref().map(Box::as_ref)
    }

    pub fn discard_zero_rtt_crypto(&mut self) {
        self.zero_rtt_crypto = None;
    }

    pub fn new(
        session: Config::TLSSession,
        initial: <Config::TLSSession as CryptoSuite>::InitialCrypto,
        now: Timestamp,
    ) -> Self {
        let ack_manager = AckManager::new(
            PacketNumberSpace::Initial,
            EARLY_ACK_SETTINGS,
            DEFAULT_ACK_RANGES_LIMIT,
        );

        Self {
            session: Some(session),
            initial: Some(Box::new(InitialSpace::new(initial, now, ack_manager))),
            handshake: None,
            application: None,
            zero_rtt_crypto: None,
            pto_backoff: INITIAL_PTO_BACKOFF,
        }
    }

    pub fn poll_crypto(
        &mut self,
        connection_config: &Config,
        path: &Path<Config::CongestionController>,
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
                pto_backoff: self.pto_backoff,
                connection_config,
            };

            session.poll(&mut context)?;

            // The TLS session is no longer needed
            if self.application.is_some() {
                self.session = None;
            }
        }

        Ok(())
    }

    pub fn interests(&self) -> ConnectionInterests {
        // TODO: Will default() prevent finalization, since it might set finalization to false?
        let mut interests = ConnectionInterests::default();

        if let Some(space) = self.initial() {
            interests += space.frame_exchange_interests();
        }
        if let Some(space) = self.handshake() {
            interests += space.frame_exchange_interests();
        }
        if let Some(space) = self.application() {
            interests += space.interests();
        }

        interests
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
    pub fn on_timeout(&mut self, timestamp: Timestamp) -> LossInfo {
        let mut loss_info = LossInfo::default();

        if let Some(space) = self.initial_mut() {
            loss_info += space.on_timeout(timestamp);
        }

        if let Some(space) = self.handshake_mut() {
            loss_info += space.on_timeout(timestamp);
        }

        if let Some(space) = self.application_mut() {
            loss_info += space.on_timeout(timestamp);
        }

        loss_info
    }

    pub fn on_loss_info(
        &mut self,
        loss_info: &LossInfo,
        path: &Path<Config::CongestionController>,
        timestamp: Timestamp,
    ) {
        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-30.txt#6.2.1
        //# When a PTO timer expires, the PTO backoff MUST be increased,
        //# resulting in the PTO period being set to twice its current value.
        if loss_info.pto_expired {
            self.pto_backoff *= 2;
        }

        if loss_info.pto_reset {
            self.pto_backoff = INITIAL_PTO_BACKOFF;
        }

        self.update_recovery(path, timestamp);
    }

    pub fn pto_backoff(&self) -> u32 {
        self.pto_backoff
    }

    pub fn update_recovery(
        &mut self,
        path: &Path<Config::CongestionController>,
        timestamp: Timestamp,
    ) {
        let pto_backoff = self.pto_backoff;
        let is_handshake_confirmed = self.is_handshake_confirmed();

        if let Some(space) = self.initial_mut() {
            space.update_recovery(path, pto_backoff, timestamp, is_handshake_confirmed);
        }

        if let Some(space) = self.handshake_mut() {
            space.update_recovery(path, pto_backoff, timestamp, is_handshake_confirmed);
        }

        if let Some(space) = self.application_mut() {
            space.update_recovery(path, pto_backoff, timestamp, is_handshake_confirmed);
        }
    }

    pub fn is_handshake_confirmed(&self) -> bool {
        self.application()
            .map(|space| space.handshake_status.is_confirmed())
            .unwrap_or(false)
    }
}

macro_rules! default_frame_handler {
    ($name:ident, $frame:ty) => {
        fn $name(
            &mut self,
            frame: $frame,
            _datagram: &DatagramInfo,
            _path: &mut Path<Config::CongestionController>,
        ) -> Result<(), TransportError> {
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
        path: &mut Path<Config::CongestionController>,
        pto_backoff: u32,
    ) -> Result<LossInfo, TransportError>;

    fn handle_handshake_done_frame(
        &mut self,
        frame: HandshakeDone,
        _datagram: &DatagramInfo,
        _path: &mut Path<Config::CongestionController>,
        _pto_backoff: u32,
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
    default_frame_handler!(handle_new_connection_id_frame, NewConnectionID);
    default_frame_handler!(handle_retire_connection_id_frame, RetireConnectionID);
    default_frame_handler!(handle_path_challenge_frame, PathChallenge);
    default_frame_handler!(handle_path_response_frame, PathResponse);

    fn on_processed_packet(
        &mut self,
        processed_packet: ProcessedPacket,
    ) -> Result<(), TransportError>;

    fn handle_cleartext_payload<'a>(
        &mut self,
        packet_number: PacketNumber,
        mut payload: DecoderBufferMut<'a>,
        datagram: &DatagramInfo,
        path: &mut Path<Config::CongestionController>,
        pto_backoff: u32,
    ) -> Result<(LossInfo, Option<frame::ConnectionClose<'a>>), TransportError> {
        let mut loss_info = LossInfo::default();

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
                    self.handle_crypto_frame(frame.into(), datagram, path)
                        .map_err(on_error)?;
                }
                Frame::Ack(frame) => {
                    let on_error = with_frame_type!(frame);
                    processed_packet.on_processed_frame(&frame);
                    loss_info += self
                        .handle_ack_frame(frame, datagram, path, pto_backoff)
                        .map_err(on_error)?;
                }
                Frame::ConnectionClose(frame) => {
                    return Ok((loss_info, Some(frame)));
                }
                Frame::Stream(frame) => {
                    let on_error = with_frame_type!(frame);
                    processed_packet.on_processed_frame(&frame);
                    self.handle_stream_frame(frame.into(), datagram, path)
                        .map_err(on_error)?;
                }
                Frame::DataBlocked(frame) => {
                    let on_error = with_frame_type!(frame);
                    processed_packet.on_processed_frame(&frame);
                    self.handle_data_blocked_frame(frame, datagram, path)
                        .map_err(on_error)?;
                }
                Frame::MaxData(frame) => {
                    let on_error = with_frame_type!(frame);
                    processed_packet.on_processed_frame(&frame);
                    self.handle_max_data_frame(frame, datagram, path)
                        .map_err(on_error)?;
                }
                Frame::MaxStreamData(frame) => {
                    let on_error = with_frame_type!(frame);
                    processed_packet.on_processed_frame(&frame);
                    self.handle_max_stream_data_frame(frame, datagram, path)
                        .map_err(on_error)?;
                }
                Frame::MaxStreams(frame) => {
                    let on_error = with_frame_type!(frame);
                    processed_packet.on_processed_frame(&frame);
                    self.handle_max_streams_frame(frame, datagram, path)
                        .map_err(on_error)?;
                }
                Frame::ResetStream(frame) => {
                    let on_error = with_frame_type!(frame);
                    processed_packet.on_processed_frame(&frame);
                    self.handle_reset_stream_frame(frame, datagram, path)
                        .map_err(on_error)?;
                }
                Frame::StopSending(frame) => {
                    let on_error = with_frame_type!(frame);
                    processed_packet.on_processed_frame(&frame);
                    self.handle_stop_sending_frame(frame, datagram, path)
                        .map_err(on_error)?;
                }
                Frame::StreamDataBlocked(frame) => {
                    let on_error = with_frame_type!(frame);
                    processed_packet.on_processed_frame(&frame);
                    self.handle_stream_data_blocked_frame(frame, datagram, path)
                        .map_err(on_error)?;
                }
                Frame::StreamsBlocked(frame) => {
                    let on_error = with_frame_type!(frame);
                    processed_packet.on_processed_frame(&frame);
                    self.handle_streams_blocked_frame(frame, datagram, path)
                        .map_err(on_error)?;
                }
                Frame::NewToken(frame) => {
                    let on_error = with_frame_type!(frame);
                    processed_packet.on_processed_frame(&frame);
                    self.handle_new_token_frame(frame, datagram, path)
                        .map_err(on_error)?;
                }
                Frame::NewConnectionID(frame) => {
                    let on_error = with_frame_type!(frame);
                    processed_packet.on_processed_frame(&frame);
                    self.handle_new_connection_id_frame(frame, datagram, path)
                        .map_err(on_error)?;
                }
                Frame::RetireConnectionID(frame) => {
                    let on_error = with_frame_type!(frame);
                    processed_packet.on_processed_frame(&frame);
                    self.handle_retire_connection_id_frame(frame, datagram, path)
                        .map_err(on_error)?;
                }
                Frame::PathChallenge(frame) => {
                    let on_error = with_frame_type!(frame);
                    processed_packet.on_processed_frame(&frame);
                    self.handle_path_challenge_frame(frame, datagram, path)
                        .map_err(on_error)?;
                }
                Frame::PathResponse(frame) => {
                    let on_error = with_frame_type!(frame);
                    processed_packet.on_processed_frame(&frame);
                    self.handle_path_response_frame(frame, datagram, path)
                        .map_err(on_error)?;
                }
                Frame::HandshakeDone(frame) => {
                    let on_error = with_frame_type!(frame);
                    processed_packet.on_processed_frame(&frame);
                    self.handle_handshake_done_frame(frame, datagram, path, pto_backoff)
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

        self.on_processed_packet(processed_packet)?;

        Ok((loss_info, None))
    }
}

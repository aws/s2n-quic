use crate::{
    connection::{self, ConnectionInterests},
    frame_exchange_interests::FrameExchangeInterestProvider,
    processed_packet::ProcessedPacket,
    recovery,
    space::rx_packet_numbers::{AckManager, DEFAULT_ACK_RANGES_LIMIT, EARLY_ACK_SETTINGS},
};
use s2n_codec::DecoderBufferMut;
use s2n_quic_core::{
    crypto::{tls::Session as TLSSession, CryptoSuite},
    frame::{
        ack::AckRanges, crypto::CryptoRef, stream::StreamRef, Ack, DataBlocked, HandshakeDone,
        MaxData, MaxStreamData, MaxStreams, NewConnectionID, NewToken, PathChallenge, PathResponse,
        ResetStream, RetireConnectionID, StopSending, StreamDataBlocked, StreamsBlocked,
    },
    inet::DatagramInfo,
    packet::{
        handshake::CleartextHandshake,
        initial::CleartextInitial,
        number::{PacketNumber, PacketNumberSpace},
        short::CleartextShort,
    },
    path::Path,
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
mod rx_packet_numbers;
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
            pub fn $discard(&mut self) {
                //= https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#6.2.1
                //# When Initial or Handshake keys are discarded, the PTO and loss
                //# detection timers MUST be reset, because discarding keys indicates
                //# forward progress and the loss detection timer might have been set for
                //# a now discarded packet number space.
                self.pto_backoff = 1;
                self.$get = None;
            }
        )?
    };
}

impl<ConnectionConfigType: connection::Config> PacketSpaceManager<ConnectionConfigType> {
    packet_space_api!(
        InitialSpace<ConnectionConfigType>,
        initial,
        initial_mut,
        discard_initial
    );

    packet_space_api!(
        HandshakeSpace<ConnectionConfigType>,
        handshake,
        handshake_mut,
        discard_handshake
    );

    packet_space_api!(
        ApplicationSpace<ConnectionConfigType>,
        application,
        application_mut
    );

    pub fn zero_rtt_crypto(
        &self,
    ) -> Option<&<ConnectionConfigType::TLSSession as CryptoSuite>::ZeroRTTCrypto> {
        self.zero_rtt_crypto.as_ref().map(Box::as_ref)
    }

    pub fn discard_zero_rtt_crypto(&mut self) {
        self.zero_rtt_crypto = None;
    }

    pub fn new(
        session: ConnectionConfigType::TLSSession,
        initial: <ConnectionConfigType::TLSSession as CryptoSuite>::InitialCrypto,
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
            pto_backoff: 1,
        }
    }

    pub fn poll_crypto(
        &mut self,
        connection_config: &ConnectionConfigType,
        now: Timestamp,
    ) -> Result<(), TransportError> {
        if let Some(session) = self.session.as_mut() {
            let mut context: SessionContext<ConnectionConfigType> = SessionContext {
                now,
                initial: &mut self.initial,
                handshake: &mut self.handshake,
                application: &mut self.application,
                zero_rtt_crypto: &mut self.zero_rtt_crypto,
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
    pub fn on_timeout(&mut self, timestamp: Timestamp) -> recovery::LossInfo {
        let mut loss_info = recovery::LossInfo::default();

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

    /// Gets the total number of bytes in flight
    /// TODO: should this get bytes_in_flight from path.congestion_controller.bytes_in_flight?
    pub fn bytes_in_flight(&self) -> u64 {
        core::iter::empty()
            .chain(self.initial.iter().map(|space| space.bytes_in_flight()))
            .chain(self.handshake.iter().map(|space| space.bytes_in_flight()))
            .chain(self.application.iter().map(|space| space.bytes_in_flight()))
            .sum::<u64>()
    }

    pub fn on_loss_info(&mut self, loss_info: &recovery::LossInfo) {
        if loss_info.pto_expired {
            self.pto_backoff *= 2;
        }

        if loss_info.pto_reset {
            self.pto_backoff = 1;
        }
    }

    pub fn on_packets_sent(&mut self, path: &Path, timestamp: Timestamp) {
        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#6.2.1
        //# When a PTO timer expires, the PTO backoff MUST be increased,
        //# resulting in the PTO period being set to twice its current value.
        let pto_backoff = self.pto_backoff;

        if let Some(space) = self.initial_mut() {
            space.on_packets_sent(path, pto_backoff, timestamp);
        }

        if let Some(space) = self.handshake_mut() {
            space.on_packets_sent(path, pto_backoff, timestamp);
        }

        if let Some(space) = self.application_mut() {
            space.on_packets_sent(path, pto_backoff, timestamp);
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
        fn $name(&mut self, _datagram: &DatagramInfo, frame: $frame) -> Result<(), TransportError> {
            Err(TransportError::PROTOCOL_VIOLATION
                .with_reason(Self::INVALID_FRAME_ERROR)
                .with_frame_type(frame.tag().into()))
        }
    };
}

pub trait PacketSpace {
    const INVALID_FRAME_ERROR: &'static str;

    fn handle_crypto_frame(
        &mut self,
        datagram: &DatagramInfo,
        frame: CryptoRef,
    ) -> Result<(), TransportError>;

    fn handle_ack_frame<A: AckRanges>(
        &mut self,
        datagram: &DatagramInfo,
        frame: Ack<A>,
        path: &mut Path,
        pto_backoff: u32,
    ) -> Result<crate::recovery::LossInfo, TransportError>;

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
    default_frame_handler!(handle_handshake_done_frame, HandshakeDone);

    fn on_processed_packet(
        &mut self,
        processed_packet: ProcessedPacket,
    ) -> Result<(), TransportError>;
}

pub struct Handler<'payload, 'space, Space: PacketSpace> {
    pub space: &'space mut Space,
    pub packet_number: PacketNumber,
    pub payload: DecoderBufferMut<'payload>,
    pub pto_backoff: u32,
}

pub trait PacketSpaceHandler<'a, Packet> {
    type Space: PacketSpace;

    fn space_for_packet(&mut self, packet: Packet) -> Option<Handler<'a, '_, Self::Space>>;
}

impl<'a, Config: connection::Config> PacketSpaceHandler<'a, CleartextInitial<'a>>
    for PacketSpaceManager<Config>
{
    type Space = InitialSpace<Config>;

    fn space_for_packet(
        &mut self,
        packet: CleartextInitial<'a>,
    ) -> Option<Handler<'a, '_, Self::Space>> {
        Some(Handler {
            pto_backoff: self.pto_backoff,
            space: self.initial_mut()?,
            packet_number: packet.packet_number,
            payload: packet.payload,
        })
    }
}

impl<'a, Config: connection::Config> PacketSpaceHandler<'a, CleartextHandshake<'a>>
    for PacketSpaceManager<Config>
{
    type Space = HandshakeSpace<Config>;

    fn space_for_packet(
        &mut self,
        packet: CleartextHandshake<'a>,
    ) -> Option<Handler<'a, '_, Self::Space>> {
        Some(Handler {
            pto_backoff: self.pto_backoff,
            space: self.handshake_mut()?,
            packet_number: packet.packet_number,
            payload: packet.payload,
        })
    }
}

impl<'a, Config: connection::Config> PacketSpaceHandler<'a, CleartextShort<'a>>
    for PacketSpaceManager<Config>
{
    type Space = ApplicationSpace<Config>;

    fn space_for_packet(
        &mut self,
        packet: CleartextShort<'a>,
    ) -> Option<Handler<'a, '_, Self::Space>> {
        Some(Handler {
            pto_backoff: self.pto_backoff,
            space: self.application_mut()?,
            packet_number: packet.packet_number,
            payload: packet.payload,
        })
    }
}

use crate::{
    connection::{self, ConnectionInterests},
    frame_exchange_interests::FrameExchangeInterestProvider,
    processed_packet::ProcessedPacket,
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

use crate::timer::VirtualTimer;
pub(crate) use application::ApplicationSpace;
pub(crate) use application_transmission::ApplicationTransmission;
pub(crate) use crypto_stream::CryptoStream;
pub(crate) use early_transmission::EarlyTransmission;
pub(crate) use handshake::HandshakeSpace;
pub(crate) use handshake_status::HandshakeStatus;
pub(crate) use initial::InitialSpace;
use s2n_quic_core::path::Path;
pub(crate) use session_context::SessionContext;
pub(crate) use tx_packet_numbers::TxPacketNumbers;

pub struct PacketSpaceManager<ConnectionConfigType: connection::Config> {
    session: Option<ConnectionConfigType::TLSSession>,
    initial: Option<Box<InitialSpace<ConnectionConfigType>>>,
    handshake: Option<Box<HandshakeSpace<ConnectionConfigType>>>,
    application: Option<Box<ApplicationSpace<ConnectionConfigType>>>,
    zero_rtt_crypto: Option<Box<<ConnectionConfigType::TLSSession as CryptoSuite>::ZeroRTTCrypto>>,
    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#A.3
    //# The number of times a PTO has been sent without receiving an ack.
    pto_count: u32,
    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#A.3
    //# Multi-modal timer used for loss detection.
    loss_detection_timer: VirtualTimer,
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
            pto_count: 0,
            loss_detection_timer: VirtualTimer::default(),
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
    pub fn on_timeout(&mut self, timestamp: Timestamp) {
        if let Some(space) = self.initial_mut() {
            space.on_timeout(timestamp);
        }
        if let Some(space) = self.handshake_mut() {
            space.on_timeout(timestamp);
        }
        if let Some(space) = self.application_mut() {
            space.on_timeout(timestamp);
        }
    }

    /// Gets the earliest loss time
    fn loss_time(&self) -> Option<Timestamp> {
        core::iter::empty()
            .chain(self.initial.iter().flat_map(|space| space.loss_time()))
            .chain(self.handshake.iter().flat_map(|space| space.loss_time()))
            .chain(self.application.iter().flat_map(|space| space.loss_time()))
            .min()
    }

    /// Gets the earliest probe timeout
    fn probe_timeout(&self, path: &Path, now: Timestamp) -> Option<Timestamp> {
        core::iter::empty()
            .chain(
                self.initial
                    .iter()
                    .flat_map(|space| space.probe_timeout(path, self.pto_count, now)),
            )
            .chain(
                self.handshake
                    .iter()
                    .flat_map(|space| space.probe_timeout(path, self.pto_count, now)),
            )
            .chain(
                self.application
                    .iter()
                    .flat_map(|space| space.probe_timeout(path, self.pto_count, now)),
            )
            .min()
    }

    /// Gets the total number of bytes in flight
    /// TODO: should this get bytes_in_flight from path.congestion_controller.bytes_in_flight?
    fn bytes_in_flight(&self) -> u64 {
        core::iter::empty()
            .chain(self.initial.iter().map(|space| space.bytes_in_flight()))
            .chain(self.handshake.iter().map(|space| space.bytes_in_flight()))
            .chain(self.application.iter().map(|space| space.bytes_in_flight()))
            .sum::<u64>()
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#A.8
    //# QUIC loss detection uses a single timer for all timeout loss detection.
    pub fn set_loss_detection_timer(&mut self, path: &Path, now: Timestamp) {
        if let Some(earliest_loss_time) = self.loss_time() {
            // Time threshold loss detection.
            self.loss_detection_timer.set(earliest_loss_time);
            return;
        }

        if path.at_amplification_limit() {
            // The server's timer is not set if nothing can be sent.
            self.loss_detection_timer.cancel();
            return;
        }

        if self.bytes_in_flight() == 0 && path.is_validated() {
            // There is nothing to detect lost, so no timer is set.
            // However, the client needs to arm the timer if the
            // server might be blocked by the anti-amplification limit.
            self.loss_detection_timer.cancel();
            return;
        }

        // Determine which PN space to arm PTO for.
        if let Some(pto_time) = self.probe_timeout(path, now) {
            self.loss_detection_timer.set(pto_time);
        }
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#A.9
    //# When the loss detection timer expires, the timer's mode determines the action to be performed.
    pub fn on_loss_detection_timeout(&mut self, path: &Path, now: Timestamp) {
        if let Some(_earliest_loss_time) = self.loss_time() {
            // Time threshold loss Detection
            // TODO: get the packet space with the earliest loss time and call
            //       on_loss_time_timeout on that packet space
            self.set_loss_detection_timer(path, now);
            return;
        }

        // TODO:
        if self.bytes_in_flight() > 0 {
            // PTO. Send new data if available, else retransmit old data.
            // If neither is available, send a single PING frame.
            // _, pn_space = self.probe_timeout();
            // send_one_or_two_ack_eliciting_packets(pn_space)
        } else {
            // TODO: implement client
        }

        self.pto_count += 1;
        self.set_loss_detection_timer(path, now);
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
    ) -> Result<(), TransportError>;

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

pub trait PacketSpaceHandler<'a, Packet> {
    type Space: PacketSpace;

    fn space_for_packet(
        &mut self,
        packet: Packet,
    ) -> Option<(&mut Self::Space, PacketNumber, DecoderBufferMut<'a>)>;
}

impl<'a, Config: connection::Config> PacketSpaceHandler<'a, CleartextInitial<'a>>
    for PacketSpaceManager<Config>
{
    type Space = InitialSpace<Config>;

    fn space_for_packet(
        &mut self,
        packet: CleartextInitial<'a>,
    ) -> Option<(&mut Self::Space, PacketNumber, DecoderBufferMut<'a>)> {
        Some((self.initial_mut()?, packet.packet_number, packet.payload))
    }
}

impl<'a, Config: connection::Config> PacketSpaceHandler<'a, CleartextHandshake<'a>>
    for PacketSpaceManager<Config>
{
    type Space = HandshakeSpace<Config>;

    fn space_for_packet(
        &mut self,
        packet: CleartextHandshake<'a>,
    ) -> Option<(&mut Self::Space, PacketNumber, DecoderBufferMut<'a>)> {
        Some((self.handshake_mut()?, packet.packet_number, packet.payload))
    }
}

impl<'a, Config: connection::Config> PacketSpaceHandler<'a, CleartextShort<'a>>
    for PacketSpaceManager<Config>
{
    type Space = ApplicationSpace<Config>;

    fn space_for_packet(
        &mut self,
        packet: CleartextShort<'a>,
    ) -> Option<(&mut Self::Space, PacketNumber, DecoderBufferMut<'a>)> {
        Some((
            self.application_mut()?,
            packet.packet_number,
            packet.payload,
        ))
    }
}

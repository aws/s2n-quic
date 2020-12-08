use crate::{
    connection::{self, ConnectionIdMapperRegistration, ConnectionTransmissionContext},
    processed_packet::ProcessedPacket,
    recovery,
    space::{rx_packet_numbers::AckManager, HandshakeStatus, PacketSpace, TxPacketNumbers},
    stream::AbstractStreamManager,
    sync::flag,
    transmission,
};
use bytes::Bytes;
use core::marker::PhantomData;
use s2n_codec::EncoderBuffer;
use s2n_quic_core::{
    crypto::CryptoSuite,
    endpoint,
    frame::{
        ack::AckRanges, crypto::CryptoRef, stream::StreamRef, Ack, ConnectionClose, DataBlocked,
        HandshakeDone, MaxData, MaxStreamData, MaxStreams, NewConnectionID, NewToken,
        PathChallenge, PathResponse, ResetStream, RetireConnectionID, StopSending,
        StreamDataBlocked, StreamsBlocked,
    },
    inet::DatagramInfo,
    packet::{
        encoding::{PacketEncoder, PacketEncodingError},
        number::{
            PacketNumber, PacketNumberRange, PacketNumberSpace, SlidingWindow, SlidingWindowError,
        },
        short::{KeyPhase, Short, SpinBit},
    },
    path::Path,
    time::Timestamp,
    transport::error::TransportError,
};

pub struct ApplicationSpace<Config: connection::Config> {
    /// Transmission Packet numbers
    pub tx_packet_numbers: TxPacketNumbers,
    /// Ack manager
    pub ack_manager: AckManager,
    /// All streams that are managed through this connection
    pub stream_manager: AbstractStreamManager<Config::Stream>,
    /// The current [`KeyPhase`]
    pub key_phase: KeyPhase,
    /// The current state of the Spin bit
    /// TODO: Spin me
    pub spin_bit: SpinBit,
    /// The crypto suite for application data
    /// TODO: What about ZeroRtt?
    pub crypto: <Config::TLSSession as CryptoSuite>::OneRTTCrypto,
    pub alpn: Option<Bytes>,
    pub sni: Option<Bytes>,
    ping: flag::Ping,
    processed_packet_numbers: SlidingWindow,
    recovery_manager: recovery::Manager,
}

impl<Config: connection::Config> ApplicationSpace<Config> {
    pub fn new(
        crypto: <Config::TLSSession as CryptoSuite>::OneRTTCrypto,
        now: Timestamp,
        stream_manager: AbstractStreamManager<Config::Stream>,
        ack_manager: AckManager,
        sni: Option<Bytes>,
        alpn: Option<Bytes>,
    ) -> Self {
        let max_ack_delay = ack_manager.ack_settings.max_ack_delay;
        Self {
            tx_packet_numbers: TxPacketNumbers::new(PacketNumberSpace::ApplicationData, now),
            ack_manager,
            key_phase: KeyPhase::Zero,
            spin_bit: SpinBit::Zero,
            stream_manager,
            crypto,
            sni,
            alpn,
            ping: flag::Ping::default(),
            processed_packet_numbers: SlidingWindow::default(),
            recovery_manager: recovery::Manager::new(
                PacketNumberSpace::ApplicationData,
                max_ack_delay,
            ),
        }
    }

    /// Returns true if the packet number has already been processed
    pub fn is_duplicate(&self, _packet_number: PacketNumber) -> bool {
        match self.processed_packet_numbers.check(_packet_number) {
            Ok(()) => false,
            Err(SlidingWindowError::Duplicate) => {
                // TODO: emit duplicate metric
                true
            }
            Err(SlidingWindowError::TooOld) => {
                // TODO: emit too old metric
                true
            }
        }
    }

    pub fn on_transmit<'a>(
        &mut self,
        context: &mut ConnectionTransmissionContext<Config>,
        transmission_constraint: transmission::Constraint,
        handshake_status: &mut HandshakeStatus,
        buffer: EncoderBuffer<'a>,
    ) -> Result<EncoderBuffer<'a>, PacketEncodingError<'a>> {
        let mut packet_number = self.tx_packet_numbers.next();

        if self.recovery_manager.requires_probe() {
            //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.2.4
            //# If the sender wants to elicit a faster acknowledgement on PTO, it can
            //# skip a packet number to eliminate the acknowledgment delay.

            // TODO Does this interact negatively with persistent congestion detection, which
            //      relies on consecutive packet numbers?
            packet_number = packet_number.next().unwrap();
        }

        let packet_number_encoder = self.packet_number_encoder();

        let mut outcome = transmission::Outcome::default();

        let payload = transmission::Transmission {
            ack_manager: &mut self.ack_manager,
            config: <PhantomData<Config>>::default(),
            outcome: &mut outcome,
            packet_number,
            payload: transmission::application::Payload {
                handshake_status,
                ping: &mut self.ping,
                stream_manager: &mut self.stream_manager,
                connection_id_mapper_registration: context.connection_id_mapper_registration,
            },
            recovery_manager: &mut self.recovery_manager,
            timestamp: context.timestamp,
            transmission_constraint,
            tx_packet_numbers: &mut self.tx_packet_numbers,
        };

        let packet = Short {
            destination_connection_id: context.path.peer_connection_id.as_ref(),
            spin_bit: self.spin_bit,
            key_phase: self.key_phase,
            packet_number,
            payload,
        };

        let (_protected_packet, buffer) =
            packet.encode_packet(&self.crypto, packet_number_encoder, buffer)?;

        let (recovery_manager, recovery_context) =
            self.recovery(handshake_status, context.connection_id_mapper_registration);
        recovery_manager.on_packet_sent(
            packet_number,
            outcome,
            context.timestamp,
            context.path,
            &recovery_context,
        );

        Ok(buffer)
    }

    /// Signals the connection was previously blocked by anti-amplification limits
    /// but is now no longer limited.
    pub fn on_amplification_unblocked(
        &mut self,
        path: &Path<Config::CongestionController>,
        timestamp: Timestamp,
        is_handshake_confirmed: bool,
    ) {
        debug_assert!(
            Config::ENDPOINT_TYPE.is_server(),
            "Clients are never in an anti-amplification state"
        );

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#A.6
        //# When a server is blocked by anti-amplification limits, receiving a
        //# datagram unblocks it, even if none of the packets in the datagram are
        //# successfully processed.  In such a case, the PTO timer will need to
        //# be re-armed.
        self.recovery_manager
            .update_pto_timer(path, timestamp, is_handshake_confirmed);
    }

    /// Signals the handshake is done
    pub fn on_handshake_done(
        &mut self,
        path: &Path<Config::CongestionController>,
        connection_id_mapper_registration: &mut ConnectionIdMapperRegistration,
        timestamp: Timestamp,
    ) {
        // Retire all local connection IDs used during the handshake to reduce linkability
        connection_id_mapper_registration.retire_all(timestamp);

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.2.1
        //# A sender SHOULD restart its PTO timer every time an ack-eliciting
        //# packet is sent or acknowledged, when the handshake is confirmed
        //# (Section 4.1.2 of [QUIC-TLS]), or when Initial or Handshake keys are
        //# discarded (Section 4.9 of [QUIC-TLS]).
        self.recovery_manager
            .update_pto_timer(path, timestamp, true)
    }

    /// Returns all of the component timers
    pub fn timers(&self) -> impl Iterator<Item = &Timestamp> {
        core::iter::empty()
            .chain(self.ack_manager.timers())
            .chain(self.recovery_manager.timers())
    }

    /// Called when the connection timer expired
    pub fn on_timeout(
        &mut self,
        path: &mut Path<Config::CongestionController>,
        handshake_status: &mut HandshakeStatus,
        connection_id_mapper_registration: &mut ConnectionIdMapperRegistration,
        timestamp: Timestamp,
    ) {
        self.ack_manager.on_timeout(timestamp);

        let (recovery_manager, mut context) =
            self.recovery(handshake_status, connection_id_mapper_registration);
        recovery_manager.on_timeout(path, timestamp, &mut context)
    }

    /// Returns the Packet Number to be used when decoding incoming packets
    pub fn packet_number_decoder(&self) -> PacketNumber {
        self.ack_manager.largest_received_packet_number_acked()
    }

    /// Returns `true` if the recovery manager for this packet space requires a probe
    /// packet to be sent.
    pub fn requires_probe(&self) -> bool {
        self.recovery_manager.requires_probe()
    }

    pub fn ping(&mut self) {
        self.ping.send()
    }

    /// Returns the Packet Number to be used when encoding outgoing packets
    fn packet_number_encoder(&self) -> PacketNumber {
        self.tx_packet_numbers.largest_sent_packet_number_acked()
    }

    fn recovery<'a>(
        &'a mut self,
        handshake_status: &'a mut HandshakeStatus,
        connection_id_mapper_registration: &'a mut ConnectionIdMapperRegistration,
    ) -> (&'a mut recovery::Manager, RecoveryContext<'a, Config>) {
        (
            &mut self.recovery_manager,
            RecoveryContext {
                ack_manager: &mut self.ack_manager,
                handshake_status,
                ping: &mut self.ping,
                stream_manager: &mut self.stream_manager,
                connection_id_mapper_registration,
                tx_packet_numbers: &mut self.tx_packet_numbers,
            },
        )
    }
}

impl<Config: connection::Config> transmission::interest::Provider for ApplicationSpace<Config> {
    fn transmission_interest(&self) -> transmission::Interest {
        transmission::Interest::default()
            + self.ack_manager.transmission_interest()
            + self.ping.transmission_interest()
            + self.recovery_manager.transmission_interest()
            + self.stream_manager.transmission_interest()
    }
}

impl<Config: connection::Config> connection::finalization::Provider for ApplicationSpace<Config> {
    fn finalization_status(&self) -> connection::finalization::Status {
        self.stream_manager.finalization_status()
    }
}

struct RecoveryContext<'a, Config: connection::Config> {
    ack_manager: &'a mut AckManager,
    handshake_status: &'a mut HandshakeStatus,
    ping: &'a mut flag::Ping,
    stream_manager: &'a mut AbstractStreamManager<Config::Stream>,
    connection_id_mapper_registration: &'a mut ConnectionIdMapperRegistration,
    tx_packet_numbers: &'a mut TxPacketNumbers,
}

impl<'a, Config: connection::Config> recovery::Context for RecoveryContext<'a, Config> {
    const ENDPOINT_TYPE: endpoint::Type = Config::ENDPOINT_TYPE;

    fn is_handshake_confirmed(&self) -> bool {
        self.handshake_status.is_confirmed()
    }

    fn validate_packet_ack(
        &mut self,
        datagram: &DatagramInfo,
        packet_number_range: &PacketNumberRange,
    ) -> Result<(), TransportError> {
        self.tx_packet_numbers
            .on_packet_ack(datagram, packet_number_range)
    }

    fn on_new_packet_ack(
        &mut self,
        _datagram: &DatagramInfo,
        packet_number_range: &PacketNumberRange,
    ) {
        self.handshake_status.on_packet_ack(packet_number_range);
        self.ping.on_packet_ack(packet_number_range);
        self.stream_manager.on_packet_ack(packet_number_range);
        self.connection_id_mapper_registration
            .on_packet_ack(packet_number_range);
    }

    fn on_packet_ack(&mut self, datagram: &DatagramInfo, packet_number_range: &PacketNumberRange) {
        self.ack_manager
            .on_packet_ack(datagram, packet_number_range);
    }

    fn on_packet_loss(&mut self, packet_number_range: &PacketNumberRange) {
        self.ack_manager.on_packet_loss(packet_number_range);
        self.handshake_status.on_packet_loss(packet_number_range);
        self.ping.on_packet_loss(packet_number_range);
        self.stream_manager.on_packet_loss(packet_number_range);
        self.connection_id_mapper_registration
            .on_packet_loss(packet_number_range);
    }
}

impl<Config: connection::Config> PacketSpace<Config> for ApplicationSpace<Config> {
    const INVALID_FRAME_ERROR: &'static str = "invalid frame in application space";

    fn handle_crypto_frame(
        &mut self,
        frame: CryptoRef,
        _datagram: &DatagramInfo,
        _path: &mut Path<Config::CongestionController>,
    ) -> Result<(), TransportError> {
        Err(TransportError::INTERNAL_ERROR
            .with_reason("crypto frames are not currently supported in application space")
            .with_frame_type(frame.tag().into()))
    }

    fn handle_ack_frame<A: AckRanges>(
        &mut self,
        frame: Ack<A>,
        datagram: &DatagramInfo,
        path: &mut Path<Config::CongestionController>,
        handshake_status: &mut HandshakeStatus,
        connection_id_mapper_registration: &mut ConnectionIdMapperRegistration,
    ) -> Result<(), TransportError> {
        //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#4.1.2
        //= type=TODO
        //= tracking-issue=297
        //# A client MAY consider the handshake to be confirmed when it receives
        //# an acknowledgement for a 1-RTT packet.

        path.on_peer_validated();
        let (recovery_manager, mut context) =
            self.recovery(handshake_status, connection_id_mapper_registration);
        recovery_manager.on_ack_frame(datagram, frame, path, &mut context)
    }

    fn handle_connection_close_frame(
        &mut self,
        _frame: ConnectionClose,
        _datagram: &DatagramInfo,
        _path: &mut Path<Config::CongestionController>,
    ) -> Result<(), TransportError> {
        Ok(())
    }

    fn handle_stream_frame(
        &mut self,
        frame: StreamRef,
        _datagram: &DatagramInfo,
        _path: &mut Path<Config::CongestionController>,
    ) -> Result<(), TransportError> {
        self.stream_manager.on_data(&frame)
    }

    fn handle_data_blocked_frame(
        &mut self,
        frame: DataBlocked,
        _datagram: &DatagramInfo,
        _path: &mut Path<Config::CongestionController>,
    ) -> Result<(), TransportError> {
        self.stream_manager.on_data_blocked(frame)
    }

    fn handle_max_data_frame(
        &mut self,
        frame: MaxData,
        _datagram: &DatagramInfo,
        _path: &mut Path<Config::CongestionController>,
    ) -> Result<(), TransportError> {
        self.stream_manager.on_max_data(frame)
    }

    fn handle_max_stream_data_frame(
        &mut self,
        frame: MaxStreamData,
        _datagram: &DatagramInfo,
        _path: &mut Path<Config::CongestionController>,
    ) -> Result<(), TransportError> {
        self.stream_manager.on_max_stream_data(&frame)
    }

    fn handle_max_streams_frame(
        &mut self,
        frame: MaxStreams,
        _datagram: &DatagramInfo,
        _path: &mut Path<Config::CongestionController>,
    ) -> Result<(), TransportError> {
        self.stream_manager.on_max_streams(&frame)
    }

    fn handle_reset_stream_frame(
        &mut self,
        frame: ResetStream,
        _datagram: &DatagramInfo,
        _path: &mut Path<Config::CongestionController>,
    ) -> Result<(), TransportError> {
        self.stream_manager.on_reset_stream(&frame)
    }

    fn handle_stop_sending_frame(
        &mut self,
        frame: StopSending,
        _datagram: &DatagramInfo,
        _path: &mut Path<Config::CongestionController>,
    ) -> Result<(), TransportError> {
        self.stream_manager.on_stop_sending(&frame)
    }

    fn handle_stream_data_blocked_frame(
        &mut self,
        frame: StreamDataBlocked,
        _datagram: &DatagramInfo,
        _path: &mut Path<Config::CongestionController>,
    ) -> Result<(), TransportError> {
        self.stream_manager.on_stream_data_blocked(&frame)
    }

    fn handle_streams_blocked_frame(
        &mut self,
        frame: StreamsBlocked,
        _datagram: &DatagramInfo,
        _path: &mut Path<Config::CongestionController>,
    ) -> Result<(), TransportError> {
        self.stream_manager.on_streams_blocked(&frame)
    }

    fn handle_new_token_frame(
        &mut self,
        frame: NewToken,
        _datagram: &DatagramInfo,
        _path: &mut Path<Config::CongestionController>,
    ) -> Result<(), TransportError> {
        // TODO
        eprintln!("UNIMPLEMENTED APPLICATION FRAME {:?}", frame);
        Ok(())
    }

    fn handle_new_connection_id_frame(
        &mut self,
        frame: NewConnectionID,
        _datagram: &DatagramInfo,
        _path: &mut Path<Config::CongestionController>,
    ) -> Result<(), TransportError> {
        // TODO
        eprintln!("UNIMPLEMENTED APPLICATION FRAME {:?}", frame);
        Ok(())
    }

    fn handle_retire_connection_id_frame(
        &mut self,
        frame: RetireConnectionID,
        _datagram: &DatagramInfo,
        _path: &mut Path<Config::CongestionController>,
        connection_id_mapper_registration: &mut ConnectionIdMapperRegistration,
    ) -> Result<(), TransportError> {
        let sequence_number = frame.sequence_number.as_u64() as u32;

        if let Some(id) = connection_id_mapper_registration.get_connection_id(sequence_number) {
            connection_id_mapper_registration.unregister_connection_id(&id);
        }

        Ok(())
    }

    fn handle_path_challenge_frame(
        &mut self,
        frame: PathChallenge,
        _datagram: &DatagramInfo,
        _path: &mut Path<Config::CongestionController>,
    ) -> Result<(), TransportError> {
        // TODO
        eprintln!("UNIMPLEMENTED APPLICATION FRAME {:?}", frame);
        Ok(())
    }

    fn handle_path_response_frame(
        &mut self,
        frame: PathResponse,
        _datagram: &DatagramInfo,
        _path: &mut Path<Config::CongestionController>,
    ) -> Result<(), TransportError> {
        // TODO map this frame to a Path
        eprintln!("UNIMPLEMENTED APPLICATION FRAME {:?}", frame);
        Ok(())
    }

    fn handle_handshake_done_frame(
        &mut self,
        frame: HandshakeDone,
        datagram: &DatagramInfo,
        path: &mut Path<Config::CongestionController>,
        connection_id_mapper_registration: &mut ConnectionIdMapperRegistration,
        handshake_status: &mut HandshakeStatus,
    ) -> Result<(), TransportError> {
        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#19.20
        //# A server MUST
        //# treat receipt of a HANDSHAKE_DONE frame as a connection error of type
        //# PROTOCOL_VIOLATION.

        if Config::ENDPOINT_TYPE.is_server() {
            return Err(TransportError::PROTOCOL_VIOLATION
                .with_reason("Clients MUST NOT send HANDSHAKE_DONE frames")
                .with_frame_type(frame.tag().into()));
        }

        handshake_status.on_handshake_done_received();
        self.on_handshake_done(path, connection_id_mapper_registration, datagram.timestamp);

        Ok(())
    }

    fn on_processed_packet(
        &mut self,
        processed_packet: ProcessedPacket,
    ) -> Result<(), TransportError> {
        self.ack_manager.on_processed_packet(&processed_packet);
        self.processed_packet_numbers
            .insert(processed_packet.packet_number)
            .expect("packet number was already checked");
        Ok(())
    }
}

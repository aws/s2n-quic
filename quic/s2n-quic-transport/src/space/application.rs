use crate::{
    connection::{self, ConnectionTransmissionContext, ProcessingError},
    path,
    processed_packet::ProcessedPacket,
    recovery,
    space::{
        rx_packet_numbers::AckManager, HandshakeStatus, PacketSpace, PacketSpaceCrypto,
        TxPacketNumbers,
    },
    stream::AbstractStreamManager,
    sync::flag,
    timer::VirtualTimer,
    transmission,
};
use bytes::Bytes;
use core::{convert::TryInto, marker::PhantomData};
use s2n_codec::EncoderBuffer;
use s2n_quic_core::{
    crypto::{CryptoError, CryptoSuite, Key, OneRTTCrypto},
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
        short::{CleartextShort, ProtectedShort, Short, SpinBit},
        KeyPhase,
    },
    path::Path,
    recovery::RTTEstimator,
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
    /// The current state of the Spin bit
    /// TODO: Spin me
    pub spin_bit: SpinBit,
    /// The crypto suite for application data
    /// TODO: What about ZeroRtt?
    //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#6.3
    //# For this reason, endpoints MUST be able to retain two sets of packet
    //# protection keys for receiving packets: the current and the next.
    key_set: ApplicationKeySet<<Config::TLSSession as CryptoSuite>::OneRTTCrypto>,

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#7
    //# Endpoints MUST explicitly negotiate an application protocol.

    //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#8.1
    //# Unless
    //# another mechanism is used for agreeing on an application protocol,
    //# endpoints MUST use ALPN for this purpose.
    pub alpn: Bytes,
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
        alpn: Bytes,
    ) -> Self {
        //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#6.3
        //# Endpoints responding to an apparent key update MUST NOT generate a
        //# timing side-channel signal that might indicate that the Key Phase bit
        //# was invalid (see Section 9.4).
        // By pre-generating the next key, we can respond to a KeyUpdate without exposing a timing
        // side channel.
        let integrity_limit = crypto.aead_integrity_limit();
        let next_key = PacketSpaceCrypto::new(crypto.derive_next_key());
        let active_key = PacketSpaceCrypto::new(crypto);
        let key_set = ApplicationKeySet::new(active_key, next_key, integrity_limit);
        let max_ack_delay = ack_manager.ack_settings.max_ack_delay;

        Self {
            tx_packet_numbers: TxPacketNumbers::new(PacketNumberSpace::ApplicationData, now),
            ack_manager,
            spin_bit: SpinBit::Zero,
            stream_manager,
            key_set,
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

    fn key_update_in_progress(&self) -> bool {
        self.key_set.key_derivation_timer.is_armed()
    }

    /// Returns the active key which is suitable for encrypting packets or unprotecting packet
    /// headers.
    pub fn crypto(&self) -> &PacketSpaceCrypto<<Config::TLSSession as CryptoSuite>::OneRTTCrypto> {
        //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#5.4
        //# The same header protection key is used for the duration of the
        //# connection, with the value not changing after a key update (see
        //# Section 6).  This allows header protection to be used to protect the
        //# key phase.
        self.key_set.active_key()
    }

    /// Returns the current phase
    pub fn crypto_phase(&self) -> KeyPhase {
        self.key_set.key_phase
    }

    /// Rotate the key phase in the application space
    pub fn crypto_rotate_phase(&mut self) {
        self.key_set.rotate_phase()
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

        let destination_connection_id = context.path().peer_connection_id;

        let payload = transmission::Transmission {
            ack_manager: &mut self.ack_manager,
            config: <PhantomData<Config>>::default(),
            outcome: &mut outcome,
            packet_number,
            payload: transmission::application::Payload {
                handshake_status,
                ping: &mut self.ping,
                stream_manager: &mut self.stream_manager,
                local_id_registry: context.local_id_registry,
                path_manager: context.path_manager,
            },
            recovery_manager: &mut self.recovery_manager,
            timestamp: context.timestamp,
            transmission_constraint,
            tx_packet_numbers: &mut self.tx_packet_numbers,
        };

        let packet = Short {
            destination_connection_id: destination_connection_id.as_ref(),
            spin_bit: self.spin_bit,
            key_phase: self.key_set.key_phase(),
            packet_number,
            payload,
        };

        let min_packet_len = context.min_packet_len;
        let (_protected_packet, buffer) = self.key_set.crypto[self.key_set.key_phase as usize]
            .encode_packet(buffer, |buffer, key| {
                packet.encode_packet(key, packet_number_encoder, min_packet_len, buffer)
            })?;

        let (recovery_manager, mut recovery_context) = self.recovery(
            handshake_status,
            context.local_id_registry,
            context.path_id,
            context.path_manager,
        );
        recovery_manager.on_packet_sent(
            packet_number,
            outcome,
            context.timestamp,
            &mut recovery_context,
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
        local_id_registry: &mut connection::LocalIdRegistry,
        timestamp: Timestamp,
    ) {
        // Retire the local connection ID used during the handshake to reduce linkability
        local_id_registry.retire_handshake_connection_id(timestamp);

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
            .chain(self.key_set.timers())
    }

    /// Called when the connection timer expired
    pub fn on_timeout(
        &mut self,
        path_manager: &mut path::Manager<Config::CongestionController>,
        handshake_status: &mut HandshakeStatus,
        local_id_registry: &mut connection::LocalIdRegistry,
        timestamp: Timestamp,
    ) {
        self.ack_manager.on_timeout(timestamp);
        self.key_set.on_timeout(timestamp);

        let (recovery_manager, mut context) = self.recovery(
            handshake_status,
            local_id_registry,
            path_manager.active_path_id(),
            path_manager,
        );
        recovery_manager.on_timeout(timestamp, &mut context);
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
        local_id_registry: &'a mut connection::LocalIdRegistry,
        path_id: path::Id,
        path_manager: &'a mut path::Manager<Config::CongestionController>,
    ) -> (&'a mut recovery::Manager, RecoveryContext<'a, Config>) {
        (
            &mut self.recovery_manager,
            RecoveryContext {
                ack_manager: &mut self.ack_manager,
                handshake_status,
                ping: &mut self.ping,
                stream_manager: &mut self.stream_manager,
                local_id_registry,
                path_id,
                path_manager,
                tx_packet_numbers: &mut self.tx_packet_numbers,
            },
        )
    }

    /// Validate packets in the Application packet space
    pub fn validate_and_decrypt_packet<'a>(
        &mut self,
        protected: ProtectedShort<'a>,
        datagram: &DatagramInfo,
        rtt_estimator: &RTTEstimator,
    ) -> Result<CleartextShort<'a>, ProcessingError> {
        let crypto = self.crypto();
        let packet_number_decoder = self.packet_number_decoder();
        let packet =
            crypto.unprotect_packet(|key| protected.unprotect(key, packet_number_decoder))?;

        if self.is_duplicate(packet.packet_number) {
            return Err(ProcessingError::DuplicatePacket);
        }

        let packet_phase = packet.key_phase();
        let mut phase_to_use = self.crypto_phase() as u8;
        let phase_switch = phase_to_use != (packet_phase as u8);
        phase_to_use ^= phase_switch as u8;

        if self.key_update_in_progress() && phase_switch {
            //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#6.5
            //# An endpoint MAY allow a period of approximately the Probe Timeout
            //# (PTO; see [QUIC-RECOVERY]) after receiving a packet that uses the new
            //# key generation before it creates the next set of packet protection
            //# keys.
            // During this PTO we can still process delayed packets, reducing retransmits
            // required from the peer. We know the packets are delayed because they have a
            // lower packet number than expected and the old key phase.
            if packet.packet_number < self.ack_manager.largest_received_packet_number_acked() {
                phase_to_use = packet.key_phase() as u8;
            }
        }

        match self
            .key_set
            .app_decrypt_packet(phase_to_use.into(), |key| packet.decrypt(key))
        {
            Ok(packet) => {
                if packet_phase != self.crypto_phase() {
                    //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#6.2
                    //# Sending keys MUST be updated before sending an
                    //# acknowledgement for the packet that was received with updated keys.

                    //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#6.2
                    //# The endpoint MUST update its
                    //# send keys to the corresponding key phase in response, as described in
                    //# Section 6.1.
                    self.crypto_rotate_phase();

                    //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#6.3
                    //# Endpoints responding to an apparent key update MUST NOT generate a
                    //# timing side-channel signal that might indicate that the Key Phase bit
                    //# was invalid (see Section 9.4).

                    //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#6.5
                    //# An endpoint MAY allow a period of approximately the Probe Timeout
                    //# (PTO; see [QUIC-RECOVERY]) after receiving a packet that uses the new
                    //# key generation before it creates the next set of packet protection
                    //# keys.
                    self.key_set
                        .key_derivation_timer
                        .set(datagram.timestamp + rtt_estimator.pto_period(1));
                }

                //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#6.4
                //= type=TODO
                //= tracking-issue=479
                //= feature=Key update
                //# An endpoint that successfully removes protection with old
                //# keys when newer keys were used for packets with lower packet numbers
                //# MUST treat this as a connection error of type KEY_UPDATE_ERROR.
                Ok(packet)
            }
            Err(e) => {
                //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#6.4
                //= type=TODO
                //= tracking-issue=479
                //= feature=Key update
                //# Packets with higher packet numbers MUST be protected with either the
                //# same or newer packet protection keys than packets with lower packet
                //# numbers.
                Err(e)
            }
        }
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
    local_id_registry: &'a mut connection::LocalIdRegistry,
    path_id: path::Id,
    path_manager: &'a mut path::Manager<Config::CongestionController>,
    tx_packet_numbers: &'a mut TxPacketNumbers,
}

impl<'a, Config: connection::Config> recovery::Context<Config::CongestionController>
    for RecoveryContext<'a, Config>
{
    const ENDPOINT_TYPE: endpoint::Type = Config::ENDPOINT_TYPE;

    fn is_handshake_confirmed(&self) -> bool {
        self.handshake_status.is_confirmed()
    }

    fn path(&self) -> &Path<Config::CongestionController> {
        &self.path_manager[self.path_id]
    }

    fn path_mut(&mut self) -> &mut Path<Config::CongestionController> {
        &mut self.path_manager[self.path_id]
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
        self.local_id_registry.on_packet_ack(packet_number_range);
        self.path_manager.on_packet_ack(packet_number_range);
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
        self.local_id_registry.on_packet_loss(packet_number_range);
        self.path_manager.on_packet_loss(packet_number_range);
    }
}

impl<Config: connection::Config> PacketSpace<Config> for ApplicationSpace<Config> {
    const INVALID_FRAME_ERROR: &'static str = "invalid frame in application space";

    fn handle_crypto_frame(
        &mut self,
        _frame: CryptoRef,
        _datagram: &DatagramInfo,
        _path: &mut Path<Config::CongestionController>,
    ) -> Result<(), TransportError> {
        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#7.5
        //# Once the handshake completes, if an endpoint is unable to buffer all
        //# data in a CRYPTO frame, it MAY discard that CRYPTO frame and all
        //# CRYPTO frames received in the future, or it MAY close the connection
        //# with a CRYPTO_BUFFER_EXCEEDED error code.

        // we currently just discard CRYPTO frames post-handshake
        Ok(())
    }

    fn handle_ack_frame<A: AckRanges>(
        &mut self,
        frame: Ack<A>,
        datagram: &DatagramInfo,
        path_id: path::Id,
        path_manager: &mut path::Manager<Config::CongestionController>,
        handshake_status: &mut HandshakeStatus,
        local_id_registry: &mut connection::LocalIdRegistry,
    ) -> Result<(), TransportError> {
        //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#4.1.2
        //= type=TODO
        //= tracking-issue=297
        //# A client MAY consider the handshake to be confirmed when it receives
        //# an acknowledgement for a 1-RTT packet.

        let path = &mut path_manager[path_id];
        path.on_peer_validated();
        let (recovery_manager, mut context) =
            self.recovery(handshake_status, local_id_registry, path_id, path_manager);
        recovery_manager.on_ack_frame(datagram, frame, &mut context)
    }

    fn handle_connection_close_frame(
        &mut self,
        _frame: ConnectionClose,
        _datagram: &DatagramInfo,
        _path: &mut Path<Config::CongestionController>,
    ) -> Result<(), TransportError> {
        Ok(())
    }

    fn handle_stream_frame(&mut self, frame: StreamRef) -> Result<(), TransportError> {
        self.stream_manager.on_data(&frame)
    }

    fn handle_data_blocked_frame(&mut self, frame: DataBlocked) -> Result<(), TransportError> {
        self.stream_manager.on_data_blocked(frame)
    }

    fn handle_max_data_frame(&mut self, frame: MaxData) -> Result<(), TransportError> {
        self.stream_manager.on_max_data(frame)
    }

    fn handle_max_stream_data_frame(&mut self, frame: MaxStreamData) -> Result<(), TransportError> {
        self.stream_manager.on_max_stream_data(&frame)
    }

    fn handle_max_streams_frame(&mut self, frame: MaxStreams) -> Result<(), TransportError> {
        self.stream_manager.on_max_streams(&frame)
    }

    fn handle_reset_stream_frame(&mut self, frame: ResetStream) -> Result<(), TransportError> {
        self.stream_manager.on_reset_stream(&frame)
    }

    fn handle_stop_sending_frame(&mut self, frame: StopSending) -> Result<(), TransportError> {
        self.stream_manager.on_stop_sending(&frame)
    }

    fn handle_stream_data_blocked_frame(
        &mut self,
        frame: StreamDataBlocked,
    ) -> Result<(), TransportError> {
        self.stream_manager.on_stream_data_blocked(&frame)
    }

    fn handle_streams_blocked_frame(
        &mut self,
        frame: StreamsBlocked,
    ) -> Result<(), TransportError> {
        self.stream_manager.on_streams_blocked(&frame)
    }

    fn handle_new_token_frame(&mut self, frame: NewToken) -> Result<(), TransportError> {
        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#19.7
        //# Servers MUST treat receipt
        //# of a NEW_TOKEN frame as a connection error of type
        //# PROTOCOL_VIOLATION.
        if Config::ENDPOINT_TYPE.is_server() {
            return Err(TransportError::PROTOCOL_VIOLATION
                .with_reason(Self::INVALID_FRAME_ERROR)
                .with_frame_type(frame.tag().into()));
        }
        // TODO
        eprintln!("UNIMPLEMENTED APPLICATION FRAME {:?}", frame);
        Ok(())
    }

    fn handle_new_connection_id_frame(
        &mut self,
        frame: NewConnectionID,
        _datagram: &DatagramInfo,
        path_manager: &mut path::Manager<Config::CongestionController>,
    ) -> Result<(), TransportError> {
        if path_manager.active_path().peer_connection_id.is_empty() {
            //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#19.15
            //# An endpoint that is sending packets with a zero-length Destination
            //# Connection ID MUST treat receipt of a NEW_CONNECTION_ID frame as a
            //# connection error of type PROTOCOL_VIOLATION.
            return Err(TransportError::PROTOCOL_VIOLATION);
        }

        let peer_id = connection::PeerId::try_from_bytes(frame.connection_id)
            .expect("Length is validated when decoding the frame");
        let sequence_number = frame
            .sequence_number
            .as_u64()
            .try_into()
            .map_err(|_err| TransportError::PROTOCOL_VIOLATION)?;
        let retire_prior_to = frame
            .retire_prior_to
            .as_u64()
            .try_into()
            .map_err(|_err| TransportError::PROTOCOL_VIOLATION)?;
        let stateless_reset_token = (*frame.stateless_reset_token).into();

        path_manager.on_new_connection_id(
            &peer_id,
            sequence_number,
            retire_prior_to,
            &stateless_reset_token,
        )
    }

    fn handle_retire_connection_id_frame(
        &mut self,
        frame: RetireConnectionID,
        datagram: &DatagramInfo,
        path: &mut Path<Config::CongestionController>,
        local_id_registry: &mut connection::LocalIdRegistry,
    ) -> Result<(), TransportError> {
        let sequence_number = frame
            .sequence_number
            .as_u64()
            .try_into()
            .map_err(|_err| TransportError::PROTOCOL_VIOLATION)?;

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#19.16
        //# The sequence number specified in a RETIRE_CONNECTION_ID frame MUST
        //# NOT refer to the Destination Connection ID field of the packet in
        //# which the frame is contained.

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#19.16
        //# The peer MAY treat this as a
        //# connection error of type PROTOCOL_VIOLATION.

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#19.16
        //# Receipt of a RETIRE_CONNECTION_ID frame containing a sequence number
        //# greater than any previously sent to the peer MUST be treated as a
        //# connection error of type PROTOCOL_VIOLATION.
        local_id_registry
            .on_retire_connection_id(
                sequence_number,
                &datagram.destination_connection_id,
                path.rtt_estimator.smoothed_rtt(),
                datagram.timestamp,
            )
            .map_err(|err| TransportError::PROTOCOL_VIOLATION.with_reason(err.message()))
    }

    fn handle_path_challenge_frame(&mut self, frame: PathChallenge) -> Result<(), TransportError> {
        // TODO
        eprintln!("UNIMPLEMENTED APPLICATION FRAME {:?}", frame);
        Ok(())
    }

    fn handle_path_response_frame(&mut self, frame: PathResponse) -> Result<(), TransportError> {
        // TODO map this frame to a Path
        eprintln!("UNIMPLEMENTED APPLICATION FRAME {:?}", frame);
        Ok(())
    }

    fn handle_handshake_done_frame(
        &mut self,
        frame: HandshakeDone,
        datagram: &DatagramInfo,
        path: &mut Path<Config::CongestionController>,
        local_id_registry: &mut connection::LocalIdRegistry,
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
        self.on_handshake_done(path, local_id_registry, datagram.timestamp);

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

pub(crate) struct ApplicationKeySet<Key> {
    /// The current [`KeyPhase`]
    key_phase: KeyPhase,

    key_derivation_timer: VirtualTimer,

    packet_decryption_failures: u64,
    aead_integrity_limit: u64,

    /// Set of keys for the current and next phase
    pub crypto: [PacketSpaceCrypto<Key>; 2],
}

impl<K: Key> ApplicationKeySet<K>
where
    K: OneRTTCrypto,
{
    fn new(
        phase_zero: PacketSpaceCrypto<K>,
        phase_one: PacketSpaceCrypto<K>,
        aead_integrity_limit: u64,
    ) -> Self {
        //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#6
        //# The Key Phase bit is initially set to 0 for the
        //# first set of 1-RTT packets and toggled to signal each subsequent key
        //# update.
        Self {
            key_phase: KeyPhase::Zero,
            key_derivation_timer: Default::default(),
            packet_decryption_failures: 0,
            aead_integrity_limit,
            crypto: [phase_zero, phase_one],
        }
    }

    /// Rotating the phase will switch the active key
    pub fn rotate_phase(&mut self) {
        self.key_phase = (((self.key_phase as u8) + 1) % 2).into();
    }

    /// Derive a new key based on the active key, and store it in the non-active slot
    pub fn derive_and_store_next_key(&mut self) {
        let next_key = self.active_key().derive_next_key();
        let slot_to_store = ((self.key_phase as u8) + 1) % 2;
        self.crypto[slot_to_store as usize] = PacketSpaceCrypto::new(next_key);
    }

    fn aead_integrity_limit(&self) -> u64 {
        self.aead_integrity_limit
    }

    pub fn timers(&self) -> impl Iterator<Item = &Timestamp> {
        core::iter::empty().chain(self.key_derivation_timer.iter())
    }

    pub fn on_timeout(&mut self, timestamp: Timestamp) {
        // key_derivation_timer
        if self
            .key_derivation_timer
            .poll_expiration(timestamp)
            .is_ready()
        {
            //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#6.5
            //# An endpoint SHOULD retain old read keys for no more than three times
            //# the PTO after having received a packet protected using the new keys.
            self.derive_and_store_next_key();
        }
    }

    fn key_phase(&self) -> KeyPhase {
        self.key_phase
    }

    fn active_key(&self) -> &PacketSpaceCrypto<K> {
        self.key_for_phase(self.key_phase)
    }

    fn key_for_phase(&self, key_phase: KeyPhase) -> &PacketSpaceCrypto<K> {
        &self.crypto[(key_phase as u8) as usize]
    }

    pub fn on_decryption_error(&mut self) {
        self.packet_decryption_failures += 1
    }

    pub fn decryption_error_count(&self) -> u64 {
        self.packet_decryption_failures
    }

    pub fn app_decrypt_packet<F, R>(
        &mut self,
        phase: KeyPhase,
        f: F,
    ) -> Result<R, connection::ProcessingError>
    where
        K: OneRTTCrypto,
        F: FnOnce(&K) -> Result<R, CryptoError>,
    {
        match f(&self.crypto[phase as usize].key) {
            Ok(r) => Ok(r),
            Err(e) => {
                //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#6.6
                //# In addition to counting packets sent, endpoints MUST count the number
                //# of received packets that fail authentication during the lifetime of a
                //# connection.
                self.on_decryption_error();

                //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#6.6
                //# If the total number of received packets that fail
                //# authentication within the connection, across all keys, exceeds the
                //# integrity limit for the selected AEAD, the endpoint MUST immediately
                //# close the connection with a connection error of type
                //# AEAD_LIMIT_REACHED and not process any more packets.
                if self.decryption_error_count() > self.aead_integrity_limit() {
                    return Err(ProcessingError::TransportError(
                        TransportError::AEAD_LIMIT_REACHED,
                    ));
                }

                Err(ProcessingError::CryptoError(e))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use s2n_quic_core::{
        crypto::{
            header_crypto::{HeaderCrypto, HeaderProtectionMask},
            CryptoError, Key, OneRTTCrypto,
        },
        time::Clock,
    };

    use core::time::Duration;
    use s2n_quic_platform::time;
    use std::sync::Arc;

    #[derive(Default)]
    struct NullKey {
        pub value: u64,
    }

    impl Key for NullKey {
        fn decrypt(
            &self,
            _packet_number: u64,
            _header: &[u8],
            _payload: &mut [u8],
        ) -> Result<(), CryptoError> {
            Ok(())
        }

        fn encrypt(
            &self,
            _packet_number: u64,
            _header: &[u8],
            _payload: &mut [u8],
        ) -> Result<(), CryptoError> {
            Ok(())
        }

        fn tag_len(&self) -> usize {
            0
        }

        fn aead_confidentiality_limit(&self) -> u64 {
            0
        }

        fn aead_integrity_limit(&self) -> u64 {
            0
        }
    }

    impl HeaderCrypto for NullKey {
        fn opening_header_protection_mask(&self, _sample: &[u8]) -> HeaderProtectionMask {
            [0; 5]
        }

        fn opening_sample_len(&self) -> usize {
            0
        }

        fn sealing_header_protection_mask(&self, _sample: &[u8]) -> HeaderProtectionMask {
            [0; 5]
        }

        fn sealing_sample_len(&self) -> usize {
            0
        }
    }

    impl OneRTTCrypto for NullKey {
        fn derive_next_key(&self) -> Self {
            Self {
                value: self.value + 1,
            }
        }
    }

    #[test]
    fn test_key_set() {
        //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#6.3
        //= type=test
        //# For this reason, endpoints MUST be able to retain two sets of packet
        //# protection keys for receiving packets: the current and the next.

        let current_key = PacketSpaceCrypto::new(NullKey::default());
        let next_key = PacketSpaceCrypto::new(current_key.key.derive_next_key());
        let keyset = ApplicationKeySet::new(current_key, next_key, 10);

        assert_eq!(keyset.active_key().key.value, 0);
        assert_eq!(keyset.key_for_phase(KeyPhase::One).key.value, 1);
    }

    #[test]
    fn test_phase_rotation() {
        let current_key = PacketSpaceCrypto::new(NullKey::default());
        let next_key = PacketSpaceCrypto::new(current_key.key.derive_next_key());
        let mut keyset = ApplicationKeySet::new(current_key, next_key, 10);

        keyset.rotate_phase();
        assert_eq!(keyset.active_key().key.value, 1);
        keyset.rotate_phase();
        assert_eq!(keyset.active_key().key.value, 0);
    }

    #[test]
    fn test_key_derivation() {
        let current_key = PacketSpaceCrypto::new(NullKey::default());
        let next_key = PacketSpaceCrypto::new(current_key.key.derive_next_key());
        let mut keyset = ApplicationKeySet::new(current_key, next_key, 10);

        keyset.rotate_phase();
        keyset.derive_and_store_next_key();
        keyset.rotate_phase();
        let next_key = keyset.active_key();
        assert_eq!(next_key.key.value, 2);
    }

    #[test]
    fn test_key_derivation_timer() {
        let clock = Arc::new(time::testing::MockClock::new());
        time::testing::set_local_clock(clock.clone());
        let now = clock.get_time();
        let current_key = PacketSpaceCrypto::new(NullKey::default());
        let next_key = PacketSpaceCrypto::new(current_key.key.derive_next_key());
        let mut keyset = ApplicationKeySet::new(current_key, next_key, 10);
        keyset.rotate_phase();

        keyset
            .key_derivation_timer
            .set(now + Duration::from_millis(10));
        clock.adjust_by(Duration::from_millis(8));

        keyset.on_timeout(clock.get_time());
        assert_eq!(keyset.crypto[0].key.value, 0);

        clock.adjust_by(Duration::from_millis(8));
        keyset.on_timeout(clock.get_time());
        assert_eq!(keyset.crypto[0].key.value, 2);
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#6.6
    //= type=test
    //# In addition to counting packets sent, endpoints MUST count the number
    //# of received packets that fail authentication during the lifetime of a
    //# connection.
    #[test]
    fn test_decryption_failure_counter() {
        let current_key = PacketSpaceCrypto::new(NullKey::default());
        let next_key = PacketSpaceCrypto::new(current_key.key.derive_next_key());
        let mut keyset = ApplicationKeySet::new(current_key, next_key, 1);

        assert_eq!(keyset.decryption_error_count(), 0);
        assert!(matches!(
            keyset.app_decrypt_packet(keyset.key_phase(), |_key| -> Result<(), CryptoError> {
                Err(CryptoError::DECRYPT_ERROR)
            }),
            Err(ProcessingError::CryptoError(_))
        ));
        assert_eq!(keyset.decryption_error_count(), 1);
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#6.6
    //= type=test
    //# If the total number of received packets that fail
    //# authentication within the connection, across all keys, exceeds the
    //# integrity limit for the selected AEAD, the endpoint MUST immediately
    //# close the connection with a connection error of type
    //# AEAD_LIMIT_REACHED and not process any more packets.
    #[test]
    fn test_decryption_failure_enforced_aead_limit() {
        let current_key = PacketSpaceCrypto::new(NullKey::default());
        let next_key = PacketSpaceCrypto::new(current_key.key.derive_next_key());
        let mut keyset = ApplicationKeySet::new(current_key, next_key, 0);

        assert_eq!(keyset.decryption_error_count(), 0);
        assert!(matches!(
            keyset.app_decrypt_packet(keyset.key_phase(), |_key| -> Result<(), CryptoError> {
                Err(CryptoError::DECRYPT_ERROR)
            }),
            Err(ProcessingError::TransportError(
                TransportError::AEAD_LIMIT_REACHED
            ))
        ));
    }
}

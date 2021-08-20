// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    connection::{self, ConnectionTransmissionContext, ProcessingError},
    endpoint, path,
    path::Path,
    processed_packet::ProcessedPacket,
    recovery,
    space::{rx_packet_numbers::AckManager, HandshakeStatus, PacketSpace, TxPacketNumbers},
    stream::AbstractStreamManager,
    sync::flag,
    transmission,
};
use bytes::Bytes;
use core::{convert::TryInto, fmt, marker::PhantomData};
use s2n_codec::EncoderBuffer;
use s2n_quic_core::{
    crypto::{application::KeySet, tls, CryptoSuite},
    event::{self, IntoEvent},
    frame::{
        ack::AckRanges, crypto::CryptoRef, stream::StreamRef, Ack, ConnectionClose, DataBlocked,
        HandshakeDone, MaxData, MaxStreamData, MaxStreams, NewConnectionId, NewToken,
        PathChallenge, PathResponse, ResetStream, RetireConnectionId, StopSending,
        StreamDataBlocked, StreamsBlocked,
    },
    inet::DatagramInfo,
    packet::{
        encoding::{PacketEncoder, PacketEncodingError},
        number::{PacketNumber, PacketNumberRange, PacketNumberSpace, SlidingWindow},
        short::{CleartextShort, ProtectedShort, Short, SpinBit},
    },
    recovery::RttEstimator,
    time::{timer, Timestamp},
    transport,
};

pub struct ApplicationSpace<Config: endpoint::Config> {
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

    //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#6.1
    //# An endpoint MUST NOT initiate a key update prior to having confirmed
    //# the handshake (Section 4.1.2).
    key_set: KeySet<<<Config::TLSEndpoint as tls::Endpoint>::Session as CryptoSuite>::OneRttKey>,
    header_key: <<Config::TLSEndpoint as tls::Endpoint>::Session as CryptoSuite>::OneRttHeaderKey,

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
    recovery_manager: recovery::Manager<Config>,
}

impl<Config: endpoint::Config> fmt::Debug for ApplicationSpace<Config> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ApplicationSpace")
            .field("ack_manager", &self.ack_manager)
            .field("alpn", &self.alpn)
            .field("ping", &self.ping)
            .field("processed_packet_numbers", &self.processed_packet_numbers)
            .field("recovery_manager", &self.recovery_manager)
            .field("sni", &self.sni)
            .field("stream_manager", &self.stream_manager)
            .field("tx_packet_numbers", &self.tx_packet_numbers)
            .finish()
    }
}

impl<Config: endpoint::Config> ApplicationSpace<Config> {
    pub fn new(
        key: <<Config::TLSEndpoint as tls::Endpoint>::Session as CryptoSuite>::OneRttKey,
        header_key: <<Config::TLSEndpoint as tls::Endpoint>::Session as CryptoSuite>::OneRttHeaderKey,
        now: Timestamp,
        stream_manager: AbstractStreamManager<Config::Stream>,
        ack_manager: AckManager,
        sni: Option<Bytes>,
        alpn: Bytes,
    ) -> Self {
        let key_set = KeySet::new(key);
        let max_ack_delay = ack_manager.ack_settings.max_ack_delay;

        Self {
            tx_packet_numbers: TxPacketNumbers::new(PacketNumberSpace::ApplicationData, now),
            ack_manager,
            spin_bit: SpinBit::Zero,
            stream_manager,
            key_set,
            header_key,
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
    pub fn is_duplicate<Pub: event::Publisher>(
        &self,
        packet_number: PacketNumber,
        path_id: path::Id,
        publisher: &mut Pub,
    ) -> bool {
        let packet_check = self.processed_packet_numbers.check(packet_number);
        if let Err(error) = packet_check {
            publisher.on_duplicate_packet(event::builder::DuplicatePacket {
                packet_header: event::builder::PacketHeader {
                    packet_type: packet_number.into_event(),
                    version: publisher.quic_version(),
                },
                path_id: path_id.into_event(),
                error: error.into_event(),
            });
        }
        match packet_check {
            Ok(()) => false,
            Err(_) => true,
        }
    }

    pub fn on_transmit<'a>(
        &mut self,
        context: &mut ConnectionTransmissionContext<Config>,
        transmission_constraint: transmission::Constraint,
        handshake_status: &mut HandshakeStatus,
        buffer: EncoderBuffer<'a>,
    ) -> Result<(transmission::Outcome, EncoderBuffer<'a>), PacketEncodingError<'a>> {
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

        let mut outcome = transmission::Outcome::new(packet_number);

        let destination_connection_id = context.path().peer_connection_id;
        let timestamp = context.timestamp;
        let transmission_mode = context.transmission_mode;
        let min_packet_len = context.min_packet_len;

        let payload = transmission::Transmission {
            config: <PhantomData<Config>>::default(),
            outcome: &mut outcome,
            packet_number,
            payload: transmission::application::Payload::<Config>::new(
                context.path_id,
                &mut context.path_manager,
                &mut context.local_id_registry,
                context.transmission_mode,
                &mut self.ack_manager,
                handshake_status,
                &mut self.ping,
                &mut self.stream_manager,
                &mut self.recovery_manager,
            ),
            timestamp,
            transmission_constraint,
            transmission_mode,
            tx_packet_numbers: &mut self.tx_packet_numbers,
            path_id: context.path_id,
            publisher: context.publisher,
        };

        let spin_bit = self.spin_bit;
        let header_key = &self.header_key;
        let (_protected_packet, buffer) =
            self.key_set
                .encrypt_packet(buffer, |buffer, key, key_phase| {
                    let packet = Short {
                        spin_bit,
                        key_phase,
                        destination_connection_id,
                        packet_number,
                        payload,
                    };
                    packet.encode_packet(
                        key,
                        header_key,
                        packet_number_encoder,
                        min_packet_len,
                        buffer,
                    )
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
            context.ecn,
            &mut recovery_context,
        );

        Ok((outcome, buffer))
    }

    pub(super) fn on_transmit_close<'a>(
        &mut self,
        context: &mut ConnectionTransmissionContext<Config>,
        connection_close: &ConnectionClose,
        buffer: EncoderBuffer<'a>,
    ) -> Result<(transmission::Outcome, EncoderBuffer<'a>), PacketEncodingError<'a>> {
        let packet_number = self.tx_packet_numbers.next();

        let packet_number_encoder = self.packet_number_encoder();

        let mut outcome = transmission::Outcome::new(packet_number);
        let destination_connection_id = context.path().peer_connection_id;

        let payload = transmission::Transmission {
            config: <PhantomData<Config>>::default(),
            outcome: &mut outcome,
            packet_number,
            payload: transmission::connection_close::Payload {
                connection_close,
                packet_number_space: PacketNumberSpace::ApplicationData,
            },
            timestamp: context.timestamp,
            transmission_constraint: transmission::Constraint::None,
            transmission_mode: transmission::Mode::Normal,
            tx_packet_numbers: &mut self.tx_packet_numbers,
            path_id: context.path_id,
            publisher: context.publisher,
        };

        let spin_bit = self.spin_bit;
        let min_packet_len = context.min_packet_len;
        let header_key = &self.header_key;
        let (_protected_packet, buffer) =
            self.key_set
                .encrypt_packet(buffer, |buffer, key, key_phase| {
                    let packet = Short {
                        spin_bit,
                        key_phase,
                        destination_connection_id,
                        packet_number,
                        payload,
                    };
                    packet.encode_packet(
                        key,
                        header_key,
                        packet_number_encoder,
                        min_packet_len,
                        buffer,
                    )
                })?;

        Ok((outcome, buffer))
    }

    /// Signals the connection was previously blocked by anti-amplification limits
    /// but is now no longer limited.
    pub fn on_amplification_unblocked(
        &mut self,
        path: &Path<Config>,
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
        path: &Path<Config>,
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

    /// Called when the connection timer expired
    pub fn on_timeout<Pub: event::Publisher>(
        &mut self,
        path_manager: &mut path::Manager<Config>,
        handshake_status: &mut HandshakeStatus,
        local_id_registry: &mut connection::LocalIdRegistry,
        timestamp: Timestamp,
        publisher: &mut Pub,
    ) {
        self.ack_manager.on_timeout(timestamp);
        self.key_set.on_timeout(timestamp);

        let (recovery_manager, mut context) = self.recovery(
            handshake_status,
            local_id_registry,
            path_manager.active_path_id(),
            path_manager,
        );

        recovery_manager.on_timeout(timestamp, &mut context, publisher);

        self.stream_manager.on_timeout(timestamp);
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
        path_manager: &'a mut path::Manager<Config>,
    ) -> (
        &'a mut recovery::Manager<Config>,
        RecoveryContext<'a, Config>,
    ) {
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
    pub fn validate_and_decrypt_packet<'a, Pub: event::Publisher>(
        &mut self,
        protected: ProtectedShort<'a>,
        datagram: &DatagramInfo,
        rtt_estimator: &RttEstimator,
        path_id: path::Id,
        publisher: &mut Pub,
    ) -> Result<CleartextShort<'a>, ProcessingError> {
        let largest_acked = self.ack_manager.largest_received_packet_number_acked();
        let packet = protected.unprotect(&self.header_key, largest_acked)?;
        let packet_number = packet.packet_number;

        let decrypted = self.key_set.decrypt_packet(
            packet,
            largest_acked,
            //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#6.3
            //# Endpoints MAY instead defer the creation of the next set of
            //# receive packet protection keys until some time after a key update
            //# completes, up to three times the PTO; see Section 6.5.

            //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#6.5
            //# An endpoint MAY allow a period of approximately the Probe Timeout
            //# (PTO; see [QUIC-RECOVERY]) after receiving a packet that uses the new
            //# key generation before it creates the next set of packet protection
            //# keys.
            datagram.timestamp + rtt_estimator.pto_period(1, PacketNumberSpace::ApplicationData),
        );
        if let Ok((_, Some(generation))) = decrypted {
            publisher.on_key_update(event::builder::KeyUpdate {
                key_type: event::builder::KeyType::OneRtt { generation },
            });
        }

        //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#9.5
        //# For authentication to be
        //# free from side-channels, the entire process of header protection
        //# removal, packet number recovery, and packet protection removal MUST
        //# be applied together without timing and other side-channels.

        // We perform decryption prior to checking for duplicate to avoid short-circuiting
        // and maintain constant-time operation.
        if self.is_duplicate(packet_number, path_id, publisher) {
            return Err(ProcessingError::DuplicatePacket);
        }

        decrypted.map(|x| x.0)
    }
}

impl<Config: endpoint::Config> timer::Provider for ApplicationSpace<Config> {
    #[inline]
    fn timers<Q: timer::Query>(&self, query: &mut Q) -> timer::Result {
        self.ack_manager.timers(query)?;
        self.recovery_manager.timers(query)?;
        self.key_set.timers(query)?;
        self.stream_manager.timers(query)?;

        Ok(())
    }
}

impl<Config: endpoint::Config> transmission::interest::Provider for ApplicationSpace<Config> {
    #[inline]
    fn transmission_interest<Q: transmission::interest::Query>(
        &self,
        query: &mut Q,
    ) -> transmission::interest::Result {
        self.ack_manager.transmission_interest(query)?;
        self.ping.transmission_interest(query)?;
        self.recovery_manager.transmission_interest(query)?;
        self.stream_manager.transmission_interest(query)?;
        Ok(())
    }
}

impl<Config: endpoint::Config> connection::finalization::Provider for ApplicationSpace<Config> {
    fn finalization_status(&self) -> connection::finalization::Status {
        self.stream_manager.finalization_status()
    }
}

struct RecoveryContext<'a, Config: endpoint::Config> {
    ack_manager: &'a mut AckManager,
    handshake_status: &'a mut HandshakeStatus,
    ping: &'a mut flag::Ping,
    stream_manager: &'a mut AbstractStreamManager<Config::Stream>,
    local_id_registry: &'a mut connection::LocalIdRegistry,
    path_id: path::Id,
    path_manager: &'a mut path::Manager<Config>,
    tx_packet_numbers: &'a mut TxPacketNumbers,
}

impl<'a, Config: endpoint::Config> recovery::Context<Config> for RecoveryContext<'a, Config> {
    const ENDPOINT_TYPE: endpoint::Type = Config::ENDPOINT_TYPE;

    fn is_handshake_confirmed(&self) -> bool {
        self.handshake_status.is_confirmed()
    }

    fn path(&self) -> &Path<Config> {
        &self.path_manager[self.path_id]
    }

    fn path_mut(&mut self) -> &mut Path<Config> {
        &mut self.path_manager[self.path_id]
    }

    fn path_by_id(&self, path_id: path::Id) -> &path::Path<Config> {
        &self.path_manager[path_id]
    }

    fn path_mut_by_id(&mut self, path_id: path::Id) -> &mut path::Path<Config> {
        &mut self.path_manager[path_id]
    }

    fn path_id(&self) -> path::Id {
        self.path_id
    }

    fn validate_packet_ack(
        &mut self,
        datagram: &DatagramInfo,
        packet_number_range: &PacketNumberRange,
    ) -> Result<(), transport::Error> {
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

    fn on_rtt_update(&mut self) {
        // Update the stream manager if this RTT update was for the active path
        if self.path_manager.active_path_id() == self.path_id {
            self.stream_manager
                .on_rtt_update(&self.path_manager.active_path().rtt_estimator)
        }
    }
}

impl<Config: endpoint::Config> PacketSpace<Config> for ApplicationSpace<Config> {
    const INVALID_FRAME_ERROR: &'static str = "invalid frame in application space";

    fn handle_crypto_frame(
        &mut self,
        _frame: CryptoRef,
        _datagram: &DatagramInfo,
        _path: &mut Path<Config>,
    ) -> Result<(), transport::Error> {
        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#7.5
        //# Once the handshake completes, if an endpoint is unable to buffer all
        //# data in a CRYPTO frame, it MAY discard that CRYPTO frame and all
        //# CRYPTO frames received in the future, or it MAY close the connection
        //# with a CRYPTO_BUFFER_EXCEEDED error code.

        // we currently just discard CRYPTO frames post-handshake
        Ok(())
    }

    fn handle_ack_frame<A: AckRanges, Pub: event::Publisher>(
        &mut self,
        frame: Ack<A>,
        datagram: &DatagramInfo,
        path_id: path::Id,
        path_manager: &mut path::Manager<Config>,
        handshake_status: &mut HandshakeStatus,
        local_id_registry: &mut connection::LocalIdRegistry,
        publisher: &mut Pub,
    ) -> Result<(), transport::Error> {
        //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#4.1.2
        //= type=TODO
        //= tracking-issue=297
        //# A client MAY consider the handshake to be confirmed when it receives
        //# an acknowledgement for a 1-RTT packet.

        let path = &mut path_manager[path_id];
        path.on_peer_validated();
        let (recovery_manager, mut context) =
            self.recovery(handshake_status, local_id_registry, path_id, path_manager);
        recovery_manager.on_ack_frame(datagram, frame, &mut context, publisher)
    }

    fn handle_connection_close_frame(
        &mut self,
        _frame: ConnectionClose,
        _datagram: &DatagramInfo,
        _path: &mut Path<Config>,
    ) -> Result<(), transport::Error> {
        Ok(())
    }

    fn handle_stream_frame(&mut self, frame: StreamRef) -> Result<(), transport::Error> {
        self.stream_manager.on_data(&frame)
    }

    fn handle_data_blocked_frame(&mut self, frame: DataBlocked) -> Result<(), transport::Error> {
        self.stream_manager.on_data_blocked(frame)
    }

    fn handle_max_data_frame(&mut self, frame: MaxData) -> Result<(), transport::Error> {
        self.stream_manager.on_max_data(frame)
    }

    fn handle_max_stream_data_frame(
        &mut self,
        frame: MaxStreamData,
    ) -> Result<(), transport::Error> {
        self.stream_manager.on_max_stream_data(&frame)
    }

    fn handle_max_streams_frame(&mut self, frame: MaxStreams) -> Result<(), transport::Error> {
        self.stream_manager.on_max_streams(&frame)
    }

    fn handle_reset_stream_frame(&mut self, frame: ResetStream) -> Result<(), transport::Error> {
        self.stream_manager.on_reset_stream(&frame)
    }

    fn handle_stop_sending_frame(&mut self, frame: StopSending) -> Result<(), transport::Error> {
        self.stream_manager.on_stop_sending(&frame)
    }

    fn handle_stream_data_blocked_frame(
        &mut self,
        frame: StreamDataBlocked,
    ) -> Result<(), transport::Error> {
        self.stream_manager.on_stream_data_blocked(&frame)
    }

    fn handle_streams_blocked_frame(
        &mut self,
        frame: StreamsBlocked,
    ) -> Result<(), transport::Error> {
        self.stream_manager.on_streams_blocked(&frame)
    }

    fn handle_new_token_frame(&mut self, frame: NewToken) -> Result<(), transport::Error> {
        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#19.7
        //# Servers MUST treat receipt
        //# of a NEW_TOKEN frame as a connection error of type
        //# PROTOCOL_VIOLATION.
        if Config::ENDPOINT_TYPE.is_server() {
            return Err(transport::Error::PROTOCOL_VIOLATION
                .with_reason(Self::INVALID_FRAME_ERROR)
                .with_frame_type(frame.tag().into()));
        }
        // TODO
        eprintln!("UNIMPLEMENTED APPLICATION FRAME {:?}", frame);
        Ok(())
    }

    fn handle_new_connection_id_frame<Pub: event::Publisher>(
        &mut self,
        frame: NewConnectionId,
        _datagram: &DatagramInfo,
        path_manager: &mut path::Manager<Config>,
        publisher: &mut Pub,
    ) -> Result<(), transport::Error> {
        if path_manager.active_path().peer_connection_id.is_empty() {
            //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#19.15
            //# An endpoint that is sending packets with a zero-length Destination
            //# Connection ID MUST treat receipt of a NEW_CONNECTION_ID frame as a
            //# connection error of type PROTOCOL_VIOLATION.
            return Err(transport::Error::PROTOCOL_VIOLATION);
        }

        let peer_id = connection::PeerId::try_from_bytes(frame.connection_id)
            .expect("Length is validated when decoding the frame");
        let sequence_number = frame
            .sequence_number
            .as_u64()
            .try_into()
            .map_err(|_err| transport::Error::PROTOCOL_VIOLATION)?;
        let retire_prior_to = frame
            .retire_prior_to
            .as_u64()
            .try_into()
            .map_err(|_err| transport::Error::PROTOCOL_VIOLATION)?;
        let stateless_reset_token = (*frame.stateless_reset_token).into();

        path_manager.on_new_connection_id(
            &peer_id,
            sequence_number,
            retire_prior_to,
            &stateless_reset_token,
            publisher,
        )
    }

    fn handle_retire_connection_id_frame(
        &mut self,
        frame: RetireConnectionId,
        datagram: &DatagramInfo,
        path: &mut Path<Config>,
        local_id_registry: &mut connection::LocalIdRegistry,
    ) -> Result<(), transport::Error> {
        let sequence_number = frame
            .sequence_number
            .as_u64()
            .try_into()
            .map_err(|_err| transport::Error::PROTOCOL_VIOLATION)?;

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
            .map_err(|err| transport::Error::PROTOCOL_VIOLATION.with_reason(err.message()))
    }

    fn handle_path_challenge_frame(
        &mut self,
        frame: PathChallenge,
        path_id: path::Id,
        path_manager: &mut path::Manager<Config>,
    ) -> Result<(), transport::Error> {
        path_manager.on_path_challenge(path_id, &frame);
        Ok(())
    }

    fn handle_path_response_frame(
        &mut self,
        frame: PathResponse,
        path_manager: &mut path::Manager<Config>,
    ) -> Result<(), transport::Error> {
        path_manager.on_path_response(&frame);
        Ok(())
    }

    fn handle_handshake_done_frame(
        &mut self,
        frame: HandshakeDone,
        datagram: &DatagramInfo,
        path: &mut Path<Config>,
        local_id_registry: &mut connection::LocalIdRegistry,
        handshake_status: &mut HandshakeStatus,
    ) -> Result<(), transport::Error> {
        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#19.20
        //# A server MUST
        //# treat receipt of a HANDSHAKE_DONE frame as a connection error of type
        //# PROTOCOL_VIOLATION.

        if Config::ENDPOINT_TYPE.is_server() {
            return Err(transport::Error::PROTOCOL_VIOLATION
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
    ) -> Result<(), transport::Error> {
        self.ack_manager.on_processed_packet(&processed_packet);
        self.processed_packet_numbers
            .insert(processed_packet.packet_number)
            .expect("packet number was already checked");
        Ok(())
    }
}

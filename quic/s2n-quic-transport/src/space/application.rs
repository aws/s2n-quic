// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    ack::{pending_ack_ranges::PendingAckRanges, AckManager},
    connection::{self, ConnectionTransmissionContext, ProcessingError},
    endpoint, path,
    path::{path_event, Path},
    processed_packet::ProcessedPacket,
    recovery,
    space::{datagram, keep_alive::KeepAlive, HandshakeStatus, PacketSpace, TxPacketNumbers},
    stream::AbstractStreamManager,
    sync::flag,
    transmission,
};
use core::{convert::TryInto, fmt, marker::PhantomData};
use once_cell::sync::OnceCell;
use s2n_codec::EncoderBuffer;
use s2n_quic_core::{
    crypto::{application::KeySet, limited, tls, CryptoSuite},
    datagram::Endpoint,
    event::{self, ConnectionPublisher as _, IntoEvent},
    frame::{
        self, ack::AckRanges, crypto::CryptoRef, datagram::DatagramRef, stream::StreamRef, Ack,
        ConnectionClose, DataBlocked, HandshakeDone, MaxData, MaxStreamData, MaxStreams,
        NewConnectionId, NewToken, PathChallenge, PathResponse, ResetStream, RetireConnectionId,
        StopSending, StreamDataBlocked, StreamsBlocked,
    },
    inet::DatagramInfo,
    packet::{
        encoding::{PacketEncoder, PacketEncodingError},
        number::{PacketNumber, PacketNumberRange, PacketNumberSpace, SlidingWindow},
        short::{CleartextShort, ProtectedShort, Short, SpinBit},
    },
    path::MaxMtu,
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
    /// Aggregate ACK info stored for delayed processing
    pub pending_ack_ranges: PendingAckRanges,
    /// The crypto suite for application data
    /// TODO: What about ZeroRtt?
    //= https://www.rfc-editor.org/rfc/rfc9001#section-6.3
    //# For this reason, endpoints MUST be able to retain two sets of packet
    //# protection keys for receiving packets: the current and the next.

    //= https://www.rfc-editor.org/rfc/rfc9001#section-6.1
    //# An endpoint MUST NOT initiate a key update prior to having confirmed
    //# the handshake (Section 4.1.2).
    key_set: KeySet<<<Config::TLSEndpoint as tls::Endpoint>::Session as CryptoSuite>::OneRttKey>,
    header_key: <<Config::TLSEndpoint as tls::Endpoint>::Session as CryptoSuite>::OneRttHeaderKey,

    ping: flag::Ping,
    keep_alive: KeepAlive,
    processed_packet_numbers: SlidingWindow,
    recovery_manager: recovery::Manager<Config>,
    datagram_manager: datagram::Manager<Config>,
}

impl<Config: endpoint::Config> fmt::Debug for ApplicationSpace<Config> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ApplicationSpace")
            .field("ack_manager", &self.ack_manager)
            .field("ping", &self.ping)
            .field("processed_packet_numbers", &self.processed_packet_numbers)
            .field("recovery_manager", &self.recovery_manager)
            .field("stream_manager", &self.stream_manager)
            .field("tx_packet_numbers", &self.tx_packet_numbers)
            .finish()
    }
}

impl<Config: endpoint::Config> ApplicationSpace<Config> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        key: <<Config::TLSEndpoint as tls::Endpoint>::Session as CryptoSuite>::OneRttKey,
        header_key: <<Config::TLSEndpoint as tls::Endpoint>::Session as CryptoSuite>::OneRttHeaderKey,
        now: Timestamp,
        stream_manager: AbstractStreamManager<Config::Stream>,
        ack_manager: AckManager,
        keep_alive: KeepAlive,
        max_mtu: MaxMtu,
        datagram_sender: <<Config as endpoint::Config>::DatagramEndpoint as Endpoint>::Sender,
        datagram_receiver: <<Config as endpoint::Config>::DatagramEndpoint as Endpoint>::Receiver,
    ) -> Self {
        let key_set = KeySet::new(key, Self::key_limits(max_mtu));

        Self {
            tx_packet_numbers: TxPacketNumbers::new(PacketNumberSpace::ApplicationData, now),
            ack_manager,
            spin_bit: SpinBit::Zero,
            pending_ack_ranges: PendingAckRanges::default(),
            stream_manager,
            key_set,
            header_key,
            ping: flag::Ping::default(),
            keep_alive,
            processed_packet_numbers: SlidingWindow::default(),
            recovery_manager: recovery::Manager::new(PacketNumberSpace::ApplicationData),
            datagram_manager: datagram::Manager::new(datagram_sender, datagram_receiver),
        }
    }

    /// Returns true if the packet number has already been processed
    pub fn is_duplicate<Pub: event::ConnectionPublisher>(
        &self,
        packet_number: PacketNumber,
        path_id: path::Id,
        path: &path::Path<Config>,
        publisher: &mut Pub,
    ) -> bool {
        let packet_check = self.processed_packet_numbers.check(packet_number);
        if let Err(error) = packet_check {
            publisher.on_duplicate_packet(event::builder::DuplicatePacket {
                packet_header: event::builder::PacketHeader::new(
                    packet_number,
                    publisher.quic_version(),
                ),
                path: path_event!(path, path_id),
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
            //= https://www.rfc-editor.org/rfc/rfc9002#section-6.2.4
            //# If the sender wants to elicit a faster acknowledgement on PTO, it can
            //# skip a packet number to eliminate the acknowledgment delay.

            // TODO Does this interact negatively with persistent congestion detection, which
            //      relies on consecutive packet numbers?
            packet_number = packet_number.next().unwrap();
        }

        let packet_number_encoder = self.packet_number_encoder();

        let mut outcome = transmission::Outcome::default();

        let destination_connection_id = context.path().peer_connection_id;
        let timestamp = context.timestamp;
        let transmission_mode = context.transmission_mode;
        let min_packet_len = context.min_packet_len;
        let bytes_progressed = self.stream_manager.outgoing_bytes_progressed();

        let payload = transmission::Transmission {
            config: <PhantomData<Config>>::default(),
            outcome: &mut outcome,
            packet_number,
            payload: transmission::application::Payload::<Config>::new(
                context.path_id,
                context.path_manager,
                context.local_id_registry,
                context.transmission_mode,
                &mut self.ack_manager,
                handshake_status,
                &mut self.ping,
                &mut self.stream_manager,
                &mut self.recovery_manager,
                &mut self.datagram_manager,
            ),
            timestamp,
            transmission_constraint,
            transmission_mode,
            tx_packet_numbers: &mut self.tx_packet_numbers,
            path_id: context.path_id,
            publisher: context.publisher,
            packet_interceptor: context.packet_interceptor,
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

        outcome.bytes_progressed +=
            (self.stream_manager.outgoing_bytes_progressed() - bytes_progressed).as_u64() as usize;

        let (recovery_manager, mut recovery_context, _) = self.recovery(
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
            context.publisher,
        );

        // reset the keep alive timer after sending an ack-eliciting packet
        if outcome.ack_elicitation.is_ack_eliciting() {
            self.keep_alive.reset(timestamp);
        }

        context
            .publisher
            .on_packet_sent(event::builder::PacketSent {
                packet_header: event::builder::PacketHeader::new(
                    packet_number,
                    context.publisher.quic_version(),
                ),
            });

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

        let mut outcome = transmission::Outcome::default();
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
            packet_interceptor: context.packet_interceptor,
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

        context
            .publisher
            .on_packet_sent(event::builder::PacketSent {
                packet_header: event::builder::PacketHeader::new(
                    packet_number,
                    context.publisher.quic_version(),
                ),
            });

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

        //= https://www.rfc-editor.org/rfc/rfc9002#section-A.6
        //# When a server is blocked by anti-amplification limits, receiving a
        //# datagram unblocks it, even if none of the packets in the datagram are
        //# successfully processed.  In such a case, the PTO timer will need to
        //# be re-armed.
        self.recovery_manager
            .update_pto_timer(path, timestamp, is_handshake_confirmed);
    }

    /// Signals the handshake is confirmed
    pub fn on_handshake_confirmed(
        &mut self,
        path: &Path<Config>,
        local_id_registry: &mut connection::LocalIdRegistry,
        timestamp: Timestamp,
    ) {
        // Retire the local connection ID used during the handshake to reduce linkability
        local_id_registry.retire_handshake_connection_id();

        //= https://www.rfc-editor.org/rfc/rfc9002#section-6.2.1
        //# A sender SHOULD restart its PTO timer every time an ack-eliciting
        //# packet is sent or acknowledged, or when Initial or Handshake keys are
        //# discarded (Section 4.9 of [QUIC-TLS]).

        //= https://www.rfc-editor.org/rfc/rfc9002#section-6.2.1
        //# An endpoint MUST NOT set its PTO timer for the Application Data
        //# packet number space until the handshake is confirmed.

        // Since we maintain a separate PTO timer for each packet space, we don't have to update
        // it when the Initial or Handshake keys are discarded. However, we do need to update the
        // PTO timer when the handshake is confirmed, as the Application space PTO timer is not
        // started until the handshake is confirmed.
        self.recovery_manager
            .update_pto_timer(path, timestamp, true)
    }

    /// Called when the connection timer expired
    pub fn on_timeout<Pub: event::ConnectionPublisher>(
        &mut self,
        path_manager: &mut path::Manager<Config>,
        handshake_status: &mut HandshakeStatus,
        local_id_registry: &mut connection::LocalIdRegistry,
        random_generator: &mut Config::RandomGenerator,
        timestamp: Timestamp,
        publisher: &mut Pub,
    ) {
        self.ack_manager.on_timeout(timestamp);
        self.key_set.on_timeout(timestamp);

        let (recovery_manager, mut context, _) = self.recovery(
            handshake_status,
            local_id_registry,
            path_manager.active_path_id(),
            path_manager,
        );

        recovery_manager.on_timeout(timestamp, random_generator, &mut context, publisher);

        self.stream_manager.on_timeout(timestamp);

        if self.keep_alive.on_timeout(timestamp).is_ready() {
            publisher.on_keep_alive_timer_expired(event::builder::KeepAliveTimerExpired {
                timeout: self.keep_alive.period(),
            });

            // send a ping after timing out
            self.ping();
        }
    }

    /// Returns `true` if the recovery manager for this packet space requires a probe
    /// packet to be sent.
    pub fn requires_probe(&self) -> bool {
        self.recovery_manager.requires_probe()
    }

    pub fn ping(&mut self) {
        self.ping.send()
    }

    pub fn keep_alive(&mut self, enabled: bool) {
        self.keep_alive.update(enabled);
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
        &'a mut PendingAckRanges,
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
            &mut self.pending_ack_ranges,
        )
    }

    /// Validate packets in the Application packet space
    pub fn validate_and_decrypt_packet<'a, Pub: event::ConnectionPublisher>(
        &mut self,
        protected: ProtectedShort<'a>,
        datagram: &DatagramInfo,
        path_id: path::Id,
        path: &path::Path<Config>,
        publisher: &mut Pub,
    ) -> Result<CleartextShort<'a>, ProcessingError> {
        let largest_acked = self.ack_manager.largest_received_packet_number_acked();
        let packet = protected
            .unprotect(&self.header_key, largest_acked)
            .map_err(|err| {
                publisher.on_packet_dropped(event::builder::PacketDropped {
                    reason: event::builder::PacketDropReason::UnprotectFailed {
                        space: event::builder::KeySpace::OneRtt,
                        path: path_event!(path, path_id),
                    },
                });
                err
            })?;

        let packet_number = packet.packet_number;
        let packet_header =
            event::builder::PacketHeader::new(packet.packet_number, publisher.quic_version());
        let decrypted = self.key_set.decrypt_packet(
            packet,
            largest_acked,
            //= https://www.rfc-editor.org/rfc/rfc9001#section-6.3
            //# For a short period after a key
            //# update completes, up to the PTO, endpoints MAY defer generation of
            //# the next set of receive packet protection keys.  This allows
            //# endpoints to retain only two sets of receive keys; see Section 6.5.

            //= https://www.rfc-editor.org/rfc/rfc9001#section-6.5
            //# An endpoint MAY allow a period of approximately the Probe Timeout
            //# (PTO; see [QUIC-RECOVERY]) after promoting the next set of receive
            //# keys to be current before it creates the subsequent set of packet
            //# protection keys.
            datagram.timestamp
                + path
                    .rtt_estimator
                    .pto_period(1, PacketNumberSpace::ApplicationData),
        );
        match decrypted {
            Ok((_, Some(generation))) => {
                publisher.on_key_update(event::builder::KeyUpdate {
                    key_type: event::builder::KeyType::OneRtt { generation },
                    cipher_suite: self.key_set.cipher_suite().into_event(),
                });
            }
            Ok(_) => {}
            Err(_) => {
                publisher.on_packet_dropped(event::builder::PacketDropped {
                    reason: event::builder::PacketDropReason::DecryptionFailed {
                        packet_header,
                        path: path_event!(path, path_id),
                    },
                });
            }
        }
        //= https://www.rfc-editor.org/rfc/rfc9001#section-9.5
        //# For authentication to be
        //# free from side channels, the entire process of header protection
        //# removal, packet number recovery, and packet protection removal MUST
        //# be applied together without timing and other side channels.

        // We perform decryption prior to checking for duplicate to avoid short-circuiting
        // and maintain constant-time operation.
        if self.is_duplicate(packet_number, path_id, path, publisher) {
            return Err(ProcessingError::DuplicatePacket);
        }

        if decrypted.is_ok() {
            // reset the keep alive timer after receiving a packet
            self.keep_alive.reset(datagram.timestamp);
        }

        decrypted.map(|x| x.0)
    }

    fn key_limits(max_mtu: MaxMtu) -> limited::Limits {
        let mut limits = limited::Limits::default();

        limits.max_mtu = max_mtu;

        // AEAD optimizations are currently in the testing phase so make them opt-in at runtime
        limits.sealer_optimization_threshold = {
            static THRESHOLD: OnceCell<u64> = OnceCell::new();

            *THRESHOLD.get_or_init(|| {
                std::env::var("S2N_UNSTABLE_CRYPTO_OPT_TX")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(u64::MAX)
            })
        };

        limits.opener_optimization_threshold = {
            static THRESHOLD: OnceCell<u64> = OnceCell::new();

            *THRESHOLD.get_or_init(|| {
                std::env::var("S2N_UNSTABLE_CRYPTO_OPT_RX")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(u64::MAX)
            })
        };

        limits
    }

    // Store ACKs in PendingAckRanges for delayed processing
    //
    // Returns `Err` if the range was not inserted.
    pub fn update_pending_acks<A: frame::ack::AckRanges>(
        &mut self,
        frame: &frame::Ack<A>,
        pending_ack_ranges: &mut PendingAckRanges,
    ) -> Result<(), ()> {
        let range = frame.ack_ranges().into_iter().map(|f| {
            PacketNumberRange::new(
                PacketNumberSpace::ApplicationData.new_packet_number(*f.start()),
                PacketNumberSpace::ApplicationData.new_packet_number(*f.end()),
            )
        });
        pending_ack_ranges.extend(range, frame.ecn_counts, frame.ack_delay())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn on_pending_ack_ranges<Pub: event::ConnectionPublisher>(
        &mut self,
        timestamp: Timestamp,
        path_id: path::Id,
        path_manager: &mut path::Manager<Config>,
        handshake_status: &mut HandshakeStatus,
        local_id_registry: &mut connection::LocalIdRegistry,
        random_generator: &mut Config::RandomGenerator,
        publisher: &mut Pub,
    ) -> Result<(), transport::Error> {
        debug_assert!(
            !self.pending_ack_ranges.is_empty(),
            "pending_ack_ranges should be non-empty since connection indicated ack interest"
        );

        let (recovery_manager, mut context, pending_ack_ranges) =
            self.recovery(handshake_status, local_id_registry, path_id, path_manager);
        recovery_manager.on_pending_ack_ranges(
            timestamp,
            pending_ack_ranges,
            random_generator,
            &mut context,
            publisher,
        )
    }
}

impl<Config: endpoint::Config> timer::Provider for ApplicationSpace<Config> {
    #[inline]
    fn timers<Q: timer::Query>(&self, query: &mut Q) -> timer::Result {
        self.ack_manager.timers(query)?;
        self.recovery_manager.timers(query)?;
        self.key_set.timers(query)?;
        self.stream_manager.timers(query)?;
        self.keep_alive.timers(query)?;

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
        self.datagram_manager.transmission_interest(query)?;
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
        timestamp: Timestamp,
        packet_number_range: &PacketNumberRange,
    ) -> Result<(), transport::Error> {
        self.tx_packet_numbers
            .on_packet_ack(timestamp, packet_number_range)
    }

    fn on_new_packet_ack<Pub: event::ConnectionPublisher>(
        &mut self,
        packet_number_range: &PacketNumberRange,
        publisher: &mut Pub,
    ) {
        self.handshake_status
            .on_packet_ack(packet_number_range, publisher);
        self.ping.on_packet_ack(packet_number_range);
        self.stream_manager.on_packet_ack(packet_number_range);
        self.local_id_registry.on_packet_ack(packet_number_range);
        self.path_manager.on_packet_ack(packet_number_range);
    }

    fn on_packet_ack(&mut self, timestamp: Timestamp, packet_number_range: &PacketNumberRange) {
        self.ack_manager
            .on_packet_ack(timestamp, packet_number_range);
    }

    fn on_packet_loss<Pub: event::ConnectionPublisher>(
        &mut self,
        packet_number_range: &PacketNumberRange,
        publisher: &mut Pub,
    ) {
        self.ack_manager.on_packet_loss(packet_number_range);
        self.handshake_status
            .on_packet_loss(packet_number_range, publisher);
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

    fn handle_crypto_frame<Pub: event::ConnectionPublisher>(
        &mut self,
        _frame: CryptoRef,
        _datagram: &DatagramInfo,
        _path: &mut Path<Config>,
        _publisher: &mut Pub,
    ) -> Result<(), transport::Error> {
        //= https://www.rfc-editor.org/rfc/rfc9000#section-7.5
        //# Once the handshake completes, if an endpoint is unable to buffer all
        //# data in a CRYPTO frame, it MAY discard that CRYPTO frame and all
        //# CRYPTO frames received in the future, or it MAY close the connection
        //# with a CRYPTO_BUFFER_EXCEEDED error code.

        // we currently just discard CRYPTO frames post-handshake
        Ok(())
    }

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
    ) -> Result<(), transport::Error> {
        let path = &mut path_manager[path_id];
        path.on_peer_validated();
        let (recovery_manager, mut context, _) =
            self.recovery(handshake_status, local_id_registry, path_id, path_manager);

        // TODO enable delayed ack processing. It might be possible to process
        // the ACKs immediately if insertion into PendingAckRanges fails
        //
        // self.update_pending_acks(&frame, &mut self.pending_ack_ranges)
        recovery_manager.on_ack_frame(timestamp, frame, random_generator, &mut context, publisher)
    }

    fn handle_connection_close_frame(
        &mut self,
        _frame: ConnectionClose,
        _timestamp: Timestamp,
        _path: &mut Path<Config>,
    ) -> Result<(), transport::Error> {
        Ok(())
    }

    fn handle_stream_frame(
        &mut self,
        frame: StreamRef,
        packet: &mut ProcessedPacket,
    ) -> Result<(), transport::Error> {
        let bytes_progressed = self.stream_manager.incoming_bytes_progressed();

        self.stream_manager.on_data(&frame)?;

        packet.bytes_progressed +=
            (self.stream_manager.incoming_bytes_progressed() - bytes_progressed).as_u64() as usize;

        Ok(())
    }

    fn handle_datagram_frame(&mut self, frame: DatagramRef) -> Result<(), transport::Error> {
        self.datagram_manager.on_datagram_frame(frame);
        Ok(())
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
        //= https://www.rfc-editor.org/rfc/rfc9000#section-19.7
        //# A server MUST treat receipt
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

    fn handle_new_connection_id_frame<Pub: event::ConnectionPublisher>(
        &mut self,
        frame: NewConnectionId,
        _datagram: &DatagramInfo,
        path_manager: &mut path::Manager<Config>,
        publisher: &mut Pub,
    ) -> Result<(), transport::Error> {
        if path_manager.active_path().peer_connection_id.is_empty() {
            //= https://www.rfc-editor.org/rfc/rfc9000#section-19.15
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

        //= https://www.rfc-editor.org/rfc/rfc9000#section-19.16
        //# The sequence number specified in a RETIRE_CONNECTION_ID frame MUST
        //# NOT refer to the Destination Connection ID field of the packet in
        //# which the frame is contained.

        //= https://www.rfc-editor.org/rfc/rfc9000#section-19.16
        //# The peer MAY treat this as a
        //# connection error of type PROTOCOL_VIOLATION.

        //= https://www.rfc-editor.org/rfc/rfc9000#section-19.16
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

    fn handle_path_response_frame<Pub: event::ConnectionPublisher>(
        &mut self,
        frame: PathResponse,
        path_manager: &mut path::Manager<Config>,
        publisher: &mut Pub,
    ) -> Result<(), transport::Error> {
        path_manager.on_path_response(&frame, publisher);
        Ok(())
    }

    fn handle_handshake_done_frame<Pub: event::ConnectionPublisher>(
        &mut self,
        frame: HandshakeDone,
        timestamp: Timestamp,
        path: &mut Path<Config>,
        local_id_registry: &mut connection::LocalIdRegistry,
        handshake_status: &mut HandshakeStatus,
        publisher: &mut Pub,
    ) -> Result<(), transport::Error> {
        //= https://www.rfc-editor.org/rfc/rfc9000#section-19.20
        //# A server MUST
        //# treat receipt of a HANDSHAKE_DONE frame as a connection error of type
        //# PROTOCOL_VIOLATION.

        if Config::ENDPOINT_TYPE.is_server() {
            return Err(transport::Error::PROTOCOL_VIOLATION
                .with_reason("Clients MUST NOT send HANDSHAKE_DONE frames")
                .with_frame_type(frame.tag().into()));
        }

        handshake_status.on_handshake_done_received(publisher);

        //= https://www.rfc-editor.org/rfc/rfc9001#section-4.1.2
        //# At the
        //# client, the handshake is considered confirmed when a HANDSHAKE_DONE
        //# frame is received.
        self.on_handshake_confirmed(path, local_id_registry, timestamp);

        Ok(())
    }

    fn on_processed_packet<Pub: event::ConnectionPublisher>(
        &mut self,
        processed_packet: ProcessedPacket,
        path_id: path::Id,
        path: &Path<Config>,
        publisher: &mut Pub,
    ) -> Result<(), transport::Error> {
        self.ack_manager.on_processed_packet(
            &processed_packet,
            path_event!(path, path_id),
            publisher,
        );
        self.processed_packet_numbers
            .insert(processed_packet.packet_number)
            .expect("packet number was already checked");

        Ok(())
    }
}

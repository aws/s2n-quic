// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    ack::AckManager,
    connection::{self, ConnectionTransmissionContext, ProcessingError},
    endpoint, path,
    path::{path_event, Path},
    processed_packet::ProcessedPacket,
    recovery,
    space::{CryptoStream, HandshakeStatus, PacketSpace, TxPacketNumbers},
    transmission,
};
use core::{fmt, marker::PhantomData};
use s2n_codec::EncoderBuffer;
use s2n_quic_core::{
    connection::PeerId,
    crypto::{tls, CryptoSuite, InitialKey},
    event::{self, ConnectionPublisher as _, IntoEvent},
    frame::{ack::AckRanges, crypto::CryptoRef, Ack, ConnectionClose},
    inet::DatagramInfo,
    packet::{
        encoding::{PacketEncoder, PacketEncodingError},
        initial::{CleartextInitial, Initial, ProtectedInitial},
        number::{PacketNumber, PacketNumberRange, PacketNumberSpace, SlidingWindow},
    },
    time::{timer, Timestamp},
    transport,
};
use smallvec::SmallVec;

pub struct InitialSpace<Config: endpoint::Config> {
    pub ack_manager: AckManager,
    //= https://www.rfc-editor.org/rfc/rfc9001#section-4
    //# If QUIC needs to retransmit that data, it MUST use
    //# the same keys even if TLS has already updated to newer keys.
    pub key: <<Config::TLSEndpoint as tls::Endpoint>::Session as CryptoSuite>::InitialKey,
    pub header_key:
        <<Config::TLSEndpoint as tls::Endpoint>::Session as CryptoSuite>::InitialHeaderKey,
    //= https://www.rfc-editor.org/rfc/rfc9001#section-4.9
    //# If packets from a lower encryption level contain
    //# CRYPTO frames, frames that retransmit that data MUST be sent at the
    //# same encryption level.
    pub crypto_stream: CryptoStream,
    pub tx_packet_numbers: TxPacketNumbers,
    pub received_hello_message: bool,
    //= https://www.rfc-editor.org/rfc/rfc9000#section-17.2.5.3
    //# Subsequent Initial packets from the client include the connection ID
    //# and token values from the Retry packet.
    retry_token: Vec<u8>,
    processed_packet_numbers: SlidingWindow,
    recovery_manager: recovery::Manager<Config>,
}

impl<Config: endpoint::Config> fmt::Debug for InitialSpace<Config> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InitialSpace")
            .field("ack_manager", &self.ack_manager)
            .field("tx_packet_numbers", &self.tx_packet_numbers)
            .field("processed_packet_numbers", &self.processed_packet_numbers)
            .field("recovery_manager", &self.recovery_manager)
            .finish()
    }
}

impl<Config: endpoint::Config> InitialSpace<Config> {
    pub fn new(
        key: <<Config::TLSEndpoint as tls::Endpoint>::Session as CryptoSuite>::InitialKey,
        header_key: <<Config::TLSEndpoint as tls::Endpoint>::Session as CryptoSuite>::InitialHeaderKey,
        now: Timestamp,
        ack_manager: AckManager,
    ) -> Self {
        Self {
            ack_manager,
            key,
            header_key,
            crypto_stream: CryptoStream::new(),
            tx_packet_numbers: TxPacketNumbers::new(PacketNumberSpace::Initial, now),
            received_hello_message: false,
            retry_token: Vec::new(),
            processed_packet_numbers: SlidingWindow::default(),
            recovery_manager: recovery::Manager::new(PacketNumberSpace::Initial),
        }
    }

    /// This method gets called when a Retry packet is processed.
    ///
    /// Reset the TLS stack and recover state when the first Retry packet is processed.
    /// Also regenerate the Initial keys based on the new retry_source_connection_id.
    pub fn on_retry_packet(
        &mut self,
        path: &mut path::Path<Config>,
        retry_source_connection_id: &PeerId,
        retry_token: &[u8],
    ) {
        debug_assert!(Config::ENDPOINT_TYPE.is_client());
        self.retry_token = retry_token.to_vec();

        //= https://www.rfc-editor.org/rfc/rfc9000#section-17.2.5.2
        //# Changing the Destination Connection ID field also results in
        //# a change to the keys used to protect the Initial packet.
        let (initial_key, initial_header_key) =
                            <<Config::TLSEndpoint as tls::Endpoint>::Session as CryptoSuite>::InitialKey::new_client(
                                retry_source_connection_id.as_bytes(),
                            );

        self.key = initial_key;
        self.header_key = initial_header_key;

        //= https://www.rfc-editor.org/rfc/rfc9000#section-17.2.5.3
        //# Other than updating the Destination Connection ID and Token fields,
        //# the Initial packet sent by the client is subject to the same
        //# restrictions as the first Initial packet.  A client MUST use the same
        //# cryptographic handshake message it included in this packet.
        self.crypto_stream.on_retry_packet();

        // Reset the recovery state; discarding any previous Initial packets that
        // might have been sent/lost.
        self.recovery_manager.on_retry_packet(path);
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
        handshake_status: &HandshakeStatus,
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
        let payload = transmission::Transmission {
            config: <PhantomData<Config>>::default(),
            outcome: &mut outcome,
            packet_number,
            payload: transmission::early::Payload {
                ack_manager: &mut self.ack_manager,
                crypto_stream: &mut self.crypto_stream,
                packet_number_space: PacketNumberSpace::Initial,
                recovery_manager: &mut self.recovery_manager,
            },
            timestamp: context.timestamp,
            transmission_constraint,
            transmission_mode: context.transmission_mode,
            tx_packet_numbers: &mut self.tx_packet_numbers,
            path_id: context.path_id,
            publisher: context.publisher,
            packet_interceptor: context.packet_interceptor,
        };

        let packet = Initial {
            version: context.quic_version,
            destination_connection_id,
            source_connection_id: context.path_manager[context.path_id].local_connection_id,
            token: self.retry_token.as_slice(),
            packet_number,
            payload,
        };

        let (_protected_packet, buffer) = packet.encode_packet(
            &self.key,
            &self.header_key,
            packet_number_encoder,
            context.min_packet_len,
            buffer,
        )?;

        let time_sent = context.timestamp;
        let path_id = context.path_id;
        let (recovery_manager, mut recovery_context) =
            self.recovery(handshake_status, path_id, context.path_manager);
        recovery_manager.on_packet_sent(
            packet_number,
            outcome,
            time_sent,
            context.ecn,
            &mut recovery_context,
            context.publisher,
        );

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
                packet_number_space: PacketNumberSpace::Initial,
            },
            timestamp: context.timestamp,
            transmission_constraint: transmission::Constraint::None,
            transmission_mode: transmission::Mode::Normal,
            tx_packet_numbers: &mut self.tx_packet_numbers,
            path_id: context.path_id,
            publisher: context.publisher,
            packet_interceptor: context.packet_interceptor,
        };

        let packet = Initial {
            version: context.quic_version,
            destination_connection_id,
            source_connection_id: context.path_manager[context.path_id].local_connection_id,
            token: &[][..],
            packet_number,
            payload,
        };

        let (_protected_packet, buffer) = packet.encode_packet(
            &self.key,
            &self.header_key,
            packet_number_encoder,
            context.min_packet_len,
            buffer,
        )?;

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

    /// Called when the connection timer expired
    pub fn on_timeout<Pub: event::ConnectionPublisher>(
        &mut self,
        handshake_status: &HandshakeStatus,
        path_id: path::Id,
        path_manager: &mut path::Manager<Config>,
        random_generator: &mut Config::RandomGenerator,
        timestamp: Timestamp,
        publisher: &mut Pub,
    ) {
        self.ack_manager.on_timeout(timestamp);

        let (recovery_manager, mut context) =
            self.recovery(handshake_status, path_id, path_manager);
        recovery_manager.on_timeout(timestamp, random_generator, &mut context, publisher);
    }

    /// Called before the Initial packet space is discarded
    pub fn on_discard<Pub: event::ConnectionPublisher>(
        &mut self,
        path: &mut Path<Config>,
        path_id: path::Id,
        publisher: &mut Pub,
    ) {
        publisher.on_key_space_discarded(event::builder::KeySpaceDiscarded {
            space: event::builder::KeySpace::Initial,
        });
        self.recovery_manager
            .on_packet_number_space_discarded(path, path_id, publisher);
    }

    pub fn requires_probe(&self) -> bool {
        self.recovery_manager.requires_probe()
    }

    /// Returns the Packet Number to be used when decoding incoming packets
    pub fn packet_number_decoder(&self) -> PacketNumber {
        self.ack_manager.largest_received_packet_number_acked()
    }

    /// Returns the Packet Number to be used when encoding outgoing packets
    fn packet_number_encoder(&self) -> PacketNumber {
        self.tx_packet_numbers.largest_sent_packet_number_acked()
    }

    fn recovery<'a>(
        &'a mut self,
        handshake_status: &'a HandshakeStatus,
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
                crypto_stream: &mut self.crypto_stream,
                tx_packet_numbers: &mut self.tx_packet_numbers,
                handshake_status,
                config: PhantomData,
                path_id,
                path_manager,
            },
        )
    }

    /// Validate packets in the Initial packet space
    pub fn validate_and_decrypt_packet<'a, Pub: event::ConnectionPublisher>(
        &self,
        protected: ProtectedInitial<'a>,
        path_id: path::Id,
        path: &path::Path<Config>,
        publisher: &mut Pub,
    ) -> Result<CleartextInitial<'a>, ProcessingError> {
        let packet_number_decoder = self.packet_number_decoder();
        let packet = protected
            .unprotect(&self.header_key, packet_number_decoder)
            .map_err(|err| {
                publisher.on_packet_dropped(event::builder::PacketDropped {
                    reason: event::builder::PacketDropReason::UnprotectFailed {
                        space: event::builder::KeySpace::Initial,
                        path: path_event!(path, path_id),
                    },
                });
                err
            })?;

        if self.is_duplicate(packet.packet_number, path_id, path, publisher) {
            return Err(ProcessingError::DuplicatePacket);
        }

        let packet_header =
            event::builder::PacketHeader::new(packet.packet_number, publisher.quic_version());
        let decrypted = packet.decrypt(&self.key).map_err(|err| {
            publisher.on_packet_dropped(event::builder::PacketDropped {
                reason: event::builder::PacketDropReason::DecryptionFailed {
                    packet_header,
                    path: path_event!(path, path_id),
                },
            });
            err
        })?;

        if Config::ENDPOINT_TYPE.is_client() && !decrypted.token.is_empty() {
            //= https://www.rfc-editor.org/rfc/rfc9000#section-17.2.2
            //# Initial packets sent by the server MUST set the Token Length field
            //# to 0; clients that receive an Initial packet with a non-zero Token
            //# Length field MUST either discard the packet or generate a
            //# connection error of type PROTOCOL_VIOLATION.
            publisher.on_packet_dropped(event::builder::PacketDropped {
                reason: event::builder::PacketDropReason::NonEmptyRetryToken {
                    path: path_event!(path, path_id),
                },
            });
            return Err(ProcessingError::NonEmptyRetryToken);
        }

        Ok(decrypted)
    }

    fn parse_client_hello<Pub: event::ConnectionPublisher>(
        &mut self,
        publisher: &mut Pub,
    ) -> Result<(), transport::Error> {
        debug_assert!(Config::ENDPOINT_TYPE.is_server());
        if let Some(payload) = self.parse_hello(tls::HandshakeType::ClientHello)? {
            publisher.on_tls_client_hello(event::builder::TlsClientHello { payload: &payload });
        }
        Ok(())
    }

    fn parse_server_hello<Pub: event::ConnectionPublisher>(
        &mut self,
        publisher: &mut Pub,
    ) -> Result<(), transport::Error> {
        debug_assert!(Config::ENDPOINT_TYPE.is_client());
        if let Some(payload) = self.parse_hello(tls::HandshakeType::ServerHello)? {
            publisher.on_tls_server_hello(event::builder::TlsServerHello { payload: &payload });
        }
        Ok(())
    }

    fn parse_hello(
        &mut self,
        msg_type: tls::HandshakeType,
    ) -> Result<Option<SmallVec<[&[u8]; 5]>>, transport::Error> {
        debug_assert!(!self.received_hello_message);

        let crypto_stream = &self.crypto_stream.rx;
        debug_assert_eq!(crypto_stream.consumed_len(), 0);

        let mut chunks = crypto_stream.iter().peekable();
        let buffer = s2n_codec::DecoderBuffer::new(chunks.peek().unwrap_or(&&[][..]));

        let header = if let Ok((header, _)) = buffer.decode::<tls::HandshakeHeader>() {
            header
        } else {
            // we don't have enough data to parse the header so wait until later
            return Ok(None);
        };

        if header.msg_type() != Some(msg_type) {
            return Err(transport::Error::PROTOCOL_VIOLATION
                .with_reason("first TLS message should be a hello message"));
        }

        let len = header.len() as u64;

        // TODO make this configurable:
        //      https://github.com/aws/s2n-quic/issues/1001
        const MAX_HELLO_SIZE: u64 = 2 << 16;

        if len > MAX_HELLO_SIZE {
            return Err(transport::Error::CRYPTO_BUFFER_EXCEEDED
                .with_reason("hello message cannot exceed 16k"));
        }

        // wait until we have more chunks
        if crypto_stream.total_received_len() < len {
            return Ok(None);
        }

        self.received_hello_message = true;

        let payload = chunks
            .enumerate()
            .map(|(idx, chunk)| {
                if idx == 0 {
                    // trim off the message header
                    &chunk[core::mem::size_of::<tls::HandshakeHeader>()..]
                } else {
                    chunk
                }
            })
            .collect();

        Ok(Some(payload))
    }
}

impl<Config: endpoint::Config> timer::Provider for InitialSpace<Config> {
    #[inline]
    fn timers<Q: timer::Query>(&self, query: &mut Q) -> timer::Result {
        self.ack_manager.timers(query)?;
        self.recovery_manager.timers(query)?;

        Ok(())
    }
}

impl<Config: endpoint::Config> transmission::interest::Provider for InitialSpace<Config> {
    #[inline]
    fn transmission_interest<Q: transmission::interest::Query>(
        &self,
        query: &mut Q,
    ) -> transmission::interest::Result {
        self.ack_manager.transmission_interest(query)?;
        self.crypto_stream.transmission_interest(query)?;
        self.recovery_manager.transmission_interest(query)?;
        Ok(())
    }
}

impl<Config: endpoint::Config> connection::finalization::Provider for InitialSpace<Config> {
    fn finalization_status(&self) -> connection::finalization::Status {
        // there's nothing in here that hold up finalizing a connection
        connection::finalization::Status::Idle
    }
}

struct RecoveryContext<'a, Config: endpoint::Config> {
    ack_manager: &'a mut AckManager,
    crypto_stream: &'a mut CryptoStream,
    tx_packet_numbers: &'a mut TxPacketNumbers,
    handshake_status: &'a HandshakeStatus,
    config: PhantomData<Config>,
    path_id: path::Id,
    path_manager: &'a mut path::Manager<Config>,
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
        _publisher: &mut Pub,
    ) {
        self.crypto_stream.on_packet_ack(packet_number_range);
    }

    fn on_packet_ack(&mut self, timestamp: Timestamp, packet_number_range: &PacketNumberRange) {
        self.ack_manager
            .on_packet_ack(timestamp, packet_number_range);
    }

    fn on_packet_loss<Pub: event::ConnectionPublisher>(
        &mut self,
        packet_number_range: &PacketNumberRange,
        _publisher: &mut Pub,
    ) {
        self.crypto_stream.on_packet_loss(packet_number_range);
        self.ack_manager.on_packet_loss(packet_number_range);
    }

    fn on_rtt_update(&mut self) {}
}

//= https://www.rfc-editor.org/rfc/rfc9000#section-17.2.2
//# The payload of an Initial packet includes a CRYPTO frame (or frames)
//# containing a cryptographic handshake message, ACK frames, or both.
//# PING, PADDING, and CONNECTION_CLOSE frames of type 0x1c are also
//# permitted.  An endpoint that receives an Initial packet containing
//# other frames can either discard the packet as spurious or treat it as
//# a connection error.
impl<Config: endpoint::Config> PacketSpace<Config> for InitialSpace<Config> {
    const INVALID_FRAME_ERROR: &'static str = "invalid frame in initial space";

    fn handle_crypto_frame<Pub: event::ConnectionPublisher>(
        &mut self,
        frame: CryptoRef,
        _datagram: &DatagramInfo,
        _path: &mut Path<Config>,
        publisher: &mut Pub,
    ) -> Result<(), transport::Error> {
        self.crypto_stream.on_crypto_frame(frame)?;

        // try to parse out the hello message if we haven't yet
        if !self.received_hello_message {
            match Config::ENDPOINT_TYPE {
                endpoint::Type::Server => self.parse_client_hello(publisher)?,
                endpoint::Type::Client => self.parse_server_hello(publisher)?,
            }
        }

        Ok(())
    }

    fn handle_ack_frame<A: AckRanges, Pub: event::ConnectionPublisher>(
        &mut self,
        frame: Ack<A>,
        timestamp: Timestamp,
        path_id: path::Id,
        path_manager: &mut path::Manager<Config>,
        handshake_status: &mut HandshakeStatus,
        _local_id_registry: &mut connection::LocalIdRegistry,
        random_generator: &mut Config::RandomGenerator,
        publisher: &mut Pub,
    ) -> Result<(), transport::Error> {
        let (recovery_manager, mut context) =
            self.recovery(handshake_status, path_id, path_manager);
        recovery_manager.on_ack_frame(timestamp, frame, random_generator, &mut context, publisher)
    }

    fn handle_connection_close_frame(
        &mut self,
        frame: ConnectionClose,
        _timestamp: Timestamp,
        _path: &mut Path<Config>,
    ) -> Result<(), transport::Error> {
        //= https://www.rfc-editor.org/rfc/rfc9000#section-17.2.2
        //# CONNECTION_CLOSE frames of type 0x1c are also
        //# permitted.

        if frame.tag() != 0x1c {
            return Err(transport::Error::PROTOCOL_VIOLATION);
        }

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

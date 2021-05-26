// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    connection::{self, ConnectionTransmissionContext, ProcessingError},
    endpoint, path,
    path::Path,
    processed_packet::ProcessedPacket,
    recovery,
    recovery::congestion_controller,
    space::{
        rx_packet_numbers::AckManager, CryptoStream, HandshakeStatus, PacketSpace, TxPacketNumbers,
    },
    transmission,
};
use core::marker::PhantomData;
use s2n_codec::EncoderBuffer;
use s2n_quic_core::{
    crypto::{tls, CryptoSuite},
    frame::{ack::AckRanges, crypto::CryptoRef, Ack, ConnectionClose},
    inet::DatagramInfo,
    packet::{
        encoding::{PacketEncoder, PacketEncodingError},
        initial::{CleartextInitial, Initial, ProtectedInitial},
        number::{
            PacketNumber, PacketNumberRange, PacketNumberSpace, SlidingWindow, SlidingWindowError,
        },
    },
    time::Timestamp,
    transport,
};

pub struct InitialSpace<Config: endpoint::Config> {
    pub ack_manager: AckManager,
    //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#4
    //# If QUIC needs to retransmit that data, it MUST use
    //# the same keys even if TLS has already updated to newer keys.
    pub key: <<Config::TLSEndpoint as tls::Endpoint>::Session as CryptoSuite>::InitialKey,
    pub header_key:
        <<Config::TLSEndpoint as tls::Endpoint>::Session as CryptoSuite>::InitialHeaderKey,
    //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#4.9
    //# If packets from a lower encryption level contain
    //# CRYPTO frames, frames that retransmit that data MUST be sent at the
    //# same encryption level.
    pub crypto_stream: CryptoStream,
    pub tx_packet_numbers: TxPacketNumbers,
    processed_packet_numbers: SlidingWindow,
    recovery_manager: recovery::Manager,
}

impl<Config: endpoint::Config> InitialSpace<Config> {
    pub fn new(
        key: <<Config::TLSEndpoint as tls::Endpoint>::Session as CryptoSuite>::InitialKey,
        header_key: <<Config::TLSEndpoint as tls::Endpoint>::Session as CryptoSuite>::InitialHeaderKey,
        now: Timestamp,
        ack_manager: AckManager,
    ) -> Self {
        let max_ack_delay = ack_manager.ack_settings.max_ack_delay;
        Self {
            ack_manager,
            key,
            header_key,
            crypto_stream: CryptoStream::new(),
            tx_packet_numbers: TxPacketNumbers::new(PacketNumberSpace::Initial, now),
            processed_packet_numbers: SlidingWindow::default(),
            recovery_manager: recovery::Manager::new(PacketNumberSpace::Initial, max_ack_delay),
        }
    }

    /// Returns true if the packet number has already been processed
    pub fn is_duplicate(&self, packet_number: PacketNumber) -> bool {
        match self.processed_packet_numbers.check(packet_number) {
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
        handshake_status: &HandshakeStatus,
        buffer: EncoderBuffer<'a>,
    ) -> Result<(transmission::Outcome, EncoderBuffer<'a>), PacketEncodingError<'a>> {
        let token = &[][..]; // TODO
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
        let mut outcome = transmission::Outcome {
            packet_number,
            ..Default::default()
        };

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
            tx_packet_numbers: &mut self.tx_packet_numbers,
        };

        let packet = Initial {
            version: context.quic_version,
            destination_connection_id: context.path().peer_connection_id.as_ref(),
            source_connection_id: context.source_connection_id.as_ref(),
            token,
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
        recovery_manager.on_packet_sent(packet_number, outcome, time_sent, &mut recovery_context);

        Ok((outcome, buffer))
    }

    pub fn on_transmit_close<'a>(
        &mut self,
        context: &mut ConnectionTransmissionContext<Config>,
        connection_close: &ConnectionClose,
        buffer: EncoderBuffer<'a>,
    ) -> Result<EncoderBuffer<'a>, PacketEncodingError<'a>> {
        let packet_number = self.tx_packet_numbers.next();

        let packet_number_encoder = self.packet_number_encoder();
        let mut outcome = transmission::Outcome {
            packet_number,
            ..Default::default()
        };

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
            tx_packet_numbers: &mut self.tx_packet_numbers,
        };

        let packet = Initial {
            version: context.quic_version,
            destination_connection_id: context.path().peer_connection_id.as_ref(),
            source_connection_id: context.source_connection_id.as_ref(),
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

        Ok(buffer)
    }

    /// Signals the connection was previously blocked by anti-amplification limits
    /// but is now no longer limited.
    pub fn on_amplification_unblocked(
        &mut self,
        path: &Path<<Config::CongestionControllerEndpoint as congestion_controller::Endpoint>::CongestionController>,
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

    /// Returns all of the component timers
    pub fn timers(&self) -> impl Iterator<Item = Timestamp> {
        core::iter::empty()
            .chain(self.ack_manager.timers())
            .chain(self.recovery_manager.timers())
    }

    /// Called when the connection timer expired
    pub fn on_timeout(
        &mut self,
        handshake_status: &HandshakeStatus,
        path_id: path::Id,
        path_manager: &mut path::Manager<Config::CongestionControllerEndpoint>,
        timestamp: Timestamp,
    ) {
        self.ack_manager.on_timeout(timestamp);

        let (recovery_manager, mut context) =
            self.recovery(handshake_status, path_id, path_manager);
        recovery_manager.on_timeout(timestamp, path_id, &mut context);
    }

    /// Called before the Initial packet space is discarded
    pub fn on_discard(
        &mut self,
        path: &mut Path<<Config::CongestionControllerEndpoint as congestion_controller::Endpoint>::CongestionController>,
    ) {
        self.recovery_manager.on_packet_number_space_discarded(path);
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
        path_manager: &'a mut path::Manager<Config::CongestionControllerEndpoint>,
    ) -> (&'a mut recovery::Manager, RecoveryContext<'a, Config>) {
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
    pub fn validate_and_decrypt_packet<'a>(
        &self,
        protected: ProtectedInitial<'a>,
    ) -> Result<CleartextInitial<'a>, ProcessingError> {
        let packet_number_decoder = self.packet_number_decoder();
        let packet = protected.unprotect(&self.header_key, packet_number_decoder)?;

        if self.is_duplicate(packet.packet_number) {
            return Err(ProcessingError::DuplicatePacket);
        }

        Ok(packet.decrypt(&self.key)?)
    }
}

impl<Config: endpoint::Config> transmission::interest::Provider for InitialSpace<Config> {
    fn transmission_interest(&self) -> transmission::Interest {
        transmission::Interest::default()
            + self.ack_manager.transmission_interest()
            + self.crypto_stream.transmission_interest()
            + self.recovery_manager.transmission_interest()
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
    path_manager: &'a mut path::Manager<Config::CongestionControllerEndpoint>,
}

impl<'a, Config: endpoint::Config> recovery::Context<<Config::CongestionControllerEndpoint as congestion_controller::Endpoint>::CongestionController>
    for RecoveryContext<'a, Config>
{
    const ENDPOINT_TYPE: endpoint::Type = Config::ENDPOINT_TYPE;

    fn is_handshake_confirmed(&self) -> bool {
        self.handshake_status.is_confirmed()
    }

    fn path(
        &self,
    ) -> &Path<<Config::CongestionControllerEndpoint as congestion_controller::Endpoint>::CongestionController>
    {
        &self.path_manager[self.path_id]
    }

    fn path_mut(&mut self) -> &mut Path<<Config::CongestionControllerEndpoint as congestion_controller::Endpoint>::CongestionController>{
        &mut self.path_manager[self.path_id]
    }

    fn path_by_id(&self, path_id: path::Id) -> &path::Path<<Config::CongestionControllerEndpoint as congestion_controller::Endpoint>::CongestionController> {
        &self.path_manager[path_id]
    }

    fn path_mut_by_id(&mut self, path_id: path::Id) -> &mut path::Path<<Config::CongestionControllerEndpoint as congestion_controller::Endpoint>::CongestionController> {
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
        self.crypto_stream.on_packet_ack(packet_number_range);
    }

    fn on_packet_ack(&mut self, datagram: &DatagramInfo, packet_number_range: &PacketNumberRange) {
        self.ack_manager
            .on_packet_ack(datagram, packet_number_range);
    }

    fn on_packet_loss(&mut self, packet_number_range: &PacketNumberRange) {
        self.crypto_stream.on_packet_loss(packet_number_range);
        self.ack_manager.on_packet_loss(packet_number_range);
    }

    fn on_rtt_update(&mut self) {}
}

//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#17.2.2
//# The payload of an Initial packet includes a CRYPTO frame (or frames)
//# containing a cryptographic handshake message, ACK frames, or both.
//# PING, PADDING, and CONNECTION_CLOSE frames of type 0x1c are also
//# permitted.  An endpoint that receives an Initial packet containing
//# other frames can either discard the packet as spurious or treat it as
//# a connection error.
impl<Config: endpoint::Config> PacketSpace<Config> for InitialSpace<Config> {
    const INVALID_FRAME_ERROR: &'static str = "invalid frame in initial space";

    fn handle_crypto_frame(
        &mut self,
        frame: CryptoRef,
        _datagram: &DatagramInfo,
        _path: &mut Path<<Config::CongestionControllerEndpoint as congestion_controller::Endpoint>::CongestionController>,
    ) -> Result<(), transport::Error> {
        self.crypto_stream.on_crypto_frame(frame)?;

        Ok(())
    }

    fn handle_ack_frame<A: AckRanges>(
        &mut self,
        frame: Ack<A>,
        datagram: &DatagramInfo,
        path_id: path::Id,
        path_manager: &mut path::Manager<Config::CongestionControllerEndpoint>,
        handshake_status: &mut HandshakeStatus,
        _local_id_registry: &mut connection::LocalIdRegistry,
    ) -> Result<(), transport::Error> {
        let (recovery_manager, mut context) =
            self.recovery(handshake_status, path_id, path_manager);
        recovery_manager.on_ack_frame(datagram, frame, &mut context)
    }

    fn handle_connection_close_frame(
        &mut self,
        frame: ConnectionClose,
        _datagram: &DatagramInfo,
        _path: &mut Path<<Config::CongestionControllerEndpoint as congestion_controller::Endpoint>::CongestionController>,
    ) -> Result<(), transport::Error> {
        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#17.2.2
        //# CONNECTION_CLOSE frames of type 0x1c are also
        //# permitted.

        if frame.tag() != 0x1c {
            return Err(transport::Error::PROTOCOL_VIOLATION);
        }

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

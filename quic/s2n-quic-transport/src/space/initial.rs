use crate::{
    connection::{self, ConnectionIdMapperRegistration, ConnectionTransmissionContext},
    path,
    processed_packet::ProcessedPacket,
    recovery,
    space::{
        rx_packet_numbers::AckManager, CryptoStream, HandshakeStatus, PacketSpace, TxPacketNumbers,
    },
    transmission,
};
use core::marker::PhantomData;
use s2n_codec::EncoderBuffer;
use s2n_quic_core::{
    crypto::CryptoSuite,
    endpoint,
    frame::{ack::AckRanges, crypto::CryptoRef, Ack, ConnectionClose},
    inet::DatagramInfo,
    packet::{
        encoding::{PacketEncoder, PacketEncodingError},
        initial::Initial,
        number::{
            PacketNumber, PacketNumberRange, PacketNumberSpace, SlidingWindow, SlidingWindowError,
        },
    },
    path::Path,
    time::Timestamp,
    transport::error::TransportError,
};

pub struct InitialSpace<Config: connection::Config> {
    pub ack_manager: AckManager,
    //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#4
    //# If QUIC needs to retransmit that data, it MUST use
    //# the same keys even if TLS has already updated to newer keys.
    pub crypto: <Config::TLSSession as CryptoSuite>::InitialCrypto,
    //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#4.9
    //# If packets from a lower encryption level contain
    //# CRYPTO frames, frames that retransmit that data MUST be sent at the
    //# same encryption level.
    pub crypto_stream: CryptoStream,
    pub tx_packet_numbers: TxPacketNumbers,
    processed_packet_numbers: SlidingWindow,
    recovery_manager: recovery::Manager,
}

impl<Config: connection::Config> InitialSpace<Config> {
    pub fn new(
        crypto: <Config::TLSSession as CryptoSuite>::InitialCrypto,
        now: Timestamp,
        ack_manager: AckManager,
    ) -> Self {
        let max_ack_delay = ack_manager.ack_settings.max_ack_delay;
        Self {
            ack_manager,
            crypto,
            crypto_stream: CryptoStream::new(),
            tx_packet_numbers: TxPacketNumbers::new(PacketNumberSpace::Initial, now),
            processed_packet_numbers: SlidingWindow::default(),
            recovery_manager: recovery::Manager::new(PacketNumberSpace::Initial, max_ack_delay),
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
        handshake_status: &HandshakeStatus,
        buffer: EncoderBuffer<'a>,
    ) -> Result<EncoderBuffer<'a>, PacketEncodingError<'a>> {
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

        let mut outcome = transmission::Outcome::default();

        let payload = transmission::Transmission {
            ack_manager: &mut self.ack_manager,
            config: <PhantomData<Config>>::default(),
            outcome: &mut outcome,
            packet_number,
            payload: transmission::early::Payload {
                crypto_stream: &mut self.crypto_stream,
                packet_number_space: PacketNumberSpace::Initial,
            },
            recovery_manager: &mut self.recovery_manager,
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

        let (_protected_packet, buffer) =
            packet.encode_packet(&self.crypto, packet_number_encoder, buffer)?;

        let time_sent = context.timestamp;
        let (recovery_manager, mut recovery_context) =
            self.recovery(context.path_mut(), handshake_status);
        recovery_manager.on_packet_sent(packet_number, outcome, time_sent, &mut recovery_context);

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
        handshake_status: &HandshakeStatus,
        timestamp: Timestamp,
    ) {
        self.ack_manager.on_timeout(timestamp);

        let (recovery_manager, mut context) = self.recovery(path, handshake_status);
        recovery_manager.on_timeout(timestamp, &mut context)
    }

    /// Called before the Initial packet space is discarded
    pub fn on_discard(&mut self, path: &mut Path<Config::CongestionController>) {
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
        path: &'a mut Path<Config::CongestionController>,
        handshake_status: &'a HandshakeStatus,
    ) -> (&'a mut recovery::Manager, RecoveryContext<'a, Config>) {
        (
            &mut self.recovery_manager,
            RecoveryContext {
                ack_manager: &mut self.ack_manager,
                crypto_stream: &mut self.crypto_stream,
                tx_packet_numbers: &mut self.tx_packet_numbers,
                handshake_status,
                config: PhantomData,
                path,
            },
        )
    }
}

impl<Config: connection::Config> transmission::interest::Provider for InitialSpace<Config> {
    fn transmission_interest(&self) -> transmission::Interest {
        transmission::Interest::default()
            + self.ack_manager.transmission_interest()
            + self.crypto_stream.transmission_interest()
            + self.recovery_manager.transmission_interest()
    }
}

impl<Config: connection::Config> connection::finalization::Provider for InitialSpace<Config> {
    fn finalization_status(&self) -> connection::finalization::Status {
        // there's nothing in here that hold up finalizing a connection
        connection::finalization::Status::Idle
    }
}

struct RecoveryContext<'a, Config: connection::Config> {
    ack_manager: &'a mut AckManager,
    crypto_stream: &'a mut CryptoStream,
    tx_packet_numbers: &'a mut TxPacketNumbers,
    handshake_status: &'a HandshakeStatus,
    config: PhantomData<Config>,
    path: &'a mut Path<Config::CongestionController>,
}

impl<'a, Config: connection::Config> recovery::Context<Config::CongestionController>
    for RecoveryContext<'a, Config>
{
    const ENDPOINT_TYPE: endpoint::Type = Config::ENDPOINT_TYPE;

    fn is_handshake_confirmed(&self) -> bool {
        self.handshake_status.is_confirmed()
    }

    fn path(&self) -> &Path<Config::CongestionController> {
        self.path
    }

    fn path_mut(&mut self) -> &mut Path<Config::CongestionController> {
        &mut self.path
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
}

//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#17.2.2
//# The payload of an Initial packet includes a CRYPTO frame (or frames)
//# containing a cryptographic handshake message, ACK frames, or both.
//# PING, PADDING, and CONNECTION_CLOSE frames of type 0x1c are also
//# permitted.  An endpoint that receives an Initial packet containing
//# other frames can either discard the packet as spurious or treat it as
//# a connection error.
impl<Config: connection::Config> PacketSpace<Config> for InitialSpace<Config> {
    const INVALID_FRAME_ERROR: &'static str = "invalid frame in initial space";

    fn handle_crypto_frame(
        &mut self,
        frame: CryptoRef,
        _datagram: &DatagramInfo,
        _path: &mut Path<Config::CongestionController>,
    ) -> Result<(), TransportError> {
        self.crypto_stream.on_crypto_frame(frame)?;

        Ok(())
    }

    fn handle_ack_frame<A: AckRanges>(
        &mut self,
        frame: Ack<A>,
        datagram: &DatagramInfo,
        path_id: path::Id,
        path_manager: &mut path::Manager<Config::CongestionController>,
        handshake_status: &mut HandshakeStatus,
        _connection_id_mapper_registration: &mut ConnectionIdMapperRegistration,
    ) -> Result<(), TransportError> {
        let (recovery_manager, mut context) =
            self.recovery(&mut path_manager[path_id], handshake_status);
        recovery_manager.on_ack_frame(datagram, frame, &mut context)
    }

    fn handle_connection_close_frame(
        &mut self,
        frame: ConnectionClose,
        _datagram: &DatagramInfo,
        _path: &mut Path<Config::CongestionController>,
    ) -> Result<(), TransportError> {
        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#17.2.2
        //# CONNECTION_CLOSE frames of type 0x1c are also
        //# permitted.

        if frame.tag() != 0x1c {
            return Err(TransportError::PROTOCOL_VIOLATION);
        }

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

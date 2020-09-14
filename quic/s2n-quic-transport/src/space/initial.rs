use crate::{
    connection::{self, ConnectionTransmissionContext},
    frame_exchange_interests::{FrameExchangeInterestProvider, FrameExchangeInterests},
    processed_packet::ProcessedPacket,
    recovery,
    space::{
        rx_packet_numbers::AckManager, CryptoStream, EarlyTransmission, PacketSpace,
        TxPacketNumbers,
    },
};
use core::marker::PhantomData;
use s2n_codec::EncoderBuffer;
use s2n_quic_core::{
    crypto::CryptoSuite,
    endpoint::EndpointType,
    frame::{ack::AckRanges, crypto::CryptoRef, Ack},
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
    pub crypto: <Config::TLSSession as CryptoSuite>::InitialCrypto,
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
            recovery_manager: recovery::Manager::new(max_ack_delay),
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
        context: &ConnectionTransmissionContext,
        buffer: EncoderBuffer<'a>,
    ) -> Result<EncoderBuffer<'a>, PacketEncodingError<'a>> {
        let token = &[][..]; // TODO
        let packet_number = self.tx_packet_numbers.next();
        let packet_number_encoder = self.packet_number_encoder();
        let (crypto, payload) = self.transmission(context, packet_number);

        let packet = Initial {
            version: context.quic_version,
            destination_connection_id: context.path.peer_connection_id.as_ref(),
            source_connection_id: context.source_connection_id.as_ref(),
            token,
            packet_number,
            payload,
        };

        let (_protected_packet, buffer) =
            packet.encode_packet(crypto, packet_number_encoder, buffer)?;

        Ok(buffer)
    }

    /// Returns all of the component timers
    pub fn timers(&self) -> impl Iterator<Item = &Timestamp> {
        core::iter::empty()
            .chain(self.ack_manager.timers())
            .chain(self.recovery_manager.timers())
    }

    /// Called when the connection timer expired
    pub fn on_timeout(&mut self, timestamp: Timestamp) -> recovery::LossInfo {
        self.ack_manager.on_timeout(timestamp);

        let (recovery_manager, mut context) = self.recovery();
        recovery_manager.on_timeout(timestamp, &mut context)
    }

    pub fn on_packets_sent(
        &mut self,
        path: &Path,
        pto_backoff: u32,
        timestamp: Timestamp,
        is_handshake_confirmed: bool,
    ) {
        self.recovery_manager.update(
            path,
            pto_backoff,
            timestamp,
            PacketNumberSpace::Initial,
            is_handshake_confirmed,
        )
    }

    /// Returns the Packet Number to be used when decoding incoming packets
    pub fn packet_number_decoder(&self) -> PacketNumber {
        self.ack_manager.largest_received_packet_number_acked()
    }

    pub fn bytes_in_flight(&self) -> u64 {
        self.recovery_manager.bytes_in_flight()
    }

    /// Returns the Packet Number to be used when encoding outgoing packets
    fn packet_number_encoder(&self) -> PacketNumber {
        self.tx_packet_numbers.largest_sent_packet_number_acked()
    }

    fn transmission<'a>(
        &'a mut self,
        context: &'a ConnectionTransmissionContext,
        packet_number: PacketNumber,
    ) -> (
        &'a <Config::TLSSession as CryptoSuite>::InitialCrypto,
        EarlyTransmission<'a>,
    ) {
        (
            &self.crypto,
            EarlyTransmission {
                ack_manager: &mut self.ack_manager,
                crypto_stream: &mut self.crypto_stream,
                context,
                packet_number,
                recovery_manager: &mut self.recovery_manager,
                tx_packet_numbers: &mut self.tx_packet_numbers,
            },
        )
    }

    fn recovery(&mut self) -> (&mut recovery::Manager, RecoveryContext<Config>) {
        (
            &mut self.recovery_manager,
            RecoveryContext {
                ack_manager: &mut self.ack_manager,
                crypto_stream: &mut self.crypto_stream,
                tx_packet_numbers: &mut self.tx_packet_numbers,
                config: PhantomData,
            },
        )
    }
}

impl<Config: connection::Config> FrameExchangeInterestProvider for InitialSpace<Config> {
    fn frame_exchange_interests(&self) -> FrameExchangeInterests {
        FrameExchangeInterests::default()
            + self.ack_manager.frame_exchange_interests()
            + self.crypto_stream.frame_exchange_interests()
            + self.recovery_manager.frame_exchange_interests()
    }
}

struct RecoveryContext<'a, Config> {
    ack_manager: &'a mut AckManager,
    crypto_stream: &'a mut CryptoStream,
    tx_packet_numbers: &'a mut TxPacketNumbers,
    config: PhantomData<Config>,
}

impl<'a, Config: connection::Config> recovery::Context for RecoveryContext<'a, Config> {
    const SPACE: PacketNumberSpace = PacketNumberSpace::Initial;
    const ENDPOINT_TYPE: EndpointType = Config::ENDPOINT_TYPE;

    fn is_handshake_confirmed(&self) -> bool {
        panic!("Handshake status is not currently available in the initial space")
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

//= https://tools.ietf.org/id/draft-ietf-quic-transport-27.txt#17.2.2
//# The payload of an Initial packet includes a CRYPTO frame (or frames)
//# containing a cryptographic handshake message, ACK frames, or both.
//# PING, PADDING, and CONNECTION_CLOSE frames are also permitted.  An
//# endpoint that receives an Initial packet containing other frames can
//# either discard the packet as spurious or treat it as a connection
//# error.
impl<Config: connection::Config> PacketSpace for InitialSpace<Config> {
    const INVALID_FRAME_ERROR: &'static str = "invalid frame in initial space";

    fn handle_crypto_frame(
        &mut self,
        frame: CryptoRef,
        _datagram: &DatagramInfo,
        _path: &mut Path,
    ) -> Result<(), TransportError> {
        self.crypto_stream.on_crypto_frame(frame)?;

        Ok(())
    }

    fn handle_ack_frame<A: AckRanges>(
        &mut self,
        frame: Ack<A>,
        datagram: &DatagramInfo,
        path: &mut Path,
        pto_backoff: u32,
    ) -> Result<recovery::LossInfo, TransportError> {
        let (recovery_manager, mut context) = self.recovery();
        recovery_manager.on_ack_frame(datagram, frame, path, pto_backoff, &mut context)
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

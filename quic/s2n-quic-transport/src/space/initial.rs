use crate::{
    connection::ConnectionTransmissionContext,
    frame_exchange_interests::{FrameExchangeInterestProvider, FrameExchangeInterests},
    processed_packet::ProcessedPacket,
    recovery::RecoveryManager,
    space::{
        rx_packet_numbers::AckManager, CryptoStream, EarlyTransmission, PacketSpace,
        TxPacketNumbers,
    },
};
use s2n_codec::EncoderBuffer;
use s2n_quic_core::{
    crypto::CryptoSuite,
    frame::{ack::AckRanges, crypto::CryptoRef, Ack},
    inet::DatagramInfo,
    packet::{
        encoding::{PacketEncoder, PacketEncodingError},
        initial::Initial,
        number::{PacketNumber, PacketNumberSpace, SlidingWindow, SlidingWindowError},
    },
    path::Path,
    time::Timestamp,
    transport::error::TransportError,
};

#[derive(Debug)]
pub struct InitialSpace<Suite: CryptoSuite> {
    pub ack_manager: AckManager,
    pub crypto: Suite::InitialCrypto,
    pub crypto_stream: CryptoStream,
    pub tx_packet_numbers: TxPacketNumbers,
    processed_packet_numbers: SlidingWindow,
    recovery_manager: RecoveryManager,
}

impl<Suite: CryptoSuite> InitialSpace<Suite> {
    pub fn new(crypto: Suite::InitialCrypto, now: Timestamp, ack_manager: AckManager) -> Self {
        let max_ack_delay = ack_manager.ack_settings.max_ack_delay;
        Self {
            ack_manager,
            crypto,
            crypto_stream: CryptoStream::new(),
            tx_packet_numbers: TxPacketNumbers::new(PacketNumberSpace::Initial, now),
            processed_packet_numbers: SlidingWindow::default(),
            recovery_manager: RecoveryManager::new(PacketNumberSpace::Initial, max_ack_delay),
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
            destination_connection_id: context.destination_connection_id.as_ref(),
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
        self.ack_manager.timers()
    }

    /// Called when the connection timer expired
    pub fn on_timeout(&mut self, timestamp: Timestamp) {
        self.ack_manager.on_timeout(timestamp);
    }

    /// Returns the Packet Number to be used when decoding incoming packets
    pub fn packet_number_decoder(&self) -> PacketNumber {
        self.ack_manager.largest_received_packet_number_acked()
    }

    /// Returns the Packet Number to be used when encoding outgoing packets
    fn packet_number_encoder(&self) -> PacketNumber {
        self.tx_packet_numbers.largest_sent_packet_number_acked()
    }

    fn transmission<'a>(
        &'a mut self,
        context: &'a ConnectionTransmissionContext,
        packet_number: PacketNumber,
    ) -> (&'a Suite::InitialCrypto, EarlyTransmission<'a>) {
        (
            &self.crypto,
            EarlyTransmission {
                ack_manager: &mut self.ack_manager,
                crypto_stream: &mut self.crypto_stream,
                context,
                packet_number,
                tx_packet_numbers: &mut self.tx_packet_numbers,
            },
        )
    }

    pub fn loss_time(&self) -> Option<Timestamp> {
        self.recovery_manager.loss_time()
    }

    pub fn probe_timeout(&self, path: &Path, now: Timestamp) -> Option<Timestamp> {
        self.recovery_manager.probe_timeout(path, now)
    }
}

impl<Suite: CryptoSuite> FrameExchangeInterestProvider for InitialSpace<Suite> {
    fn frame_exchange_interests(&self) -> FrameExchangeInterests {
        FrameExchangeInterests::default()
            + self.ack_manager.frame_exchange_interests()
            + self.crypto_stream.frame_exchange_interests()
    }
}

//= https://tools.ietf.org/id/draft-ietf-quic-transport-27.txt#17.2.2
//# The payload of an Initial packet includes a CRYPTO frame (or frames)
//# containing a cryptographic handshake message, ACK frames, or both.
//# PING, PADDING, and CONNECTION_CLOSE frames are also permitted.  An
//# endpoint that receives an Initial packet containing other frames can
//# either discard the packet as spurious or treat it as a connection
//# error.
impl<Suite: CryptoSuite> PacketSpace for InitialSpace<Suite> {
    const INVALID_FRAME_ERROR: &'static str = "invalid frame in initial space";

    fn handle_crypto_frame(
        &mut self,
        _datagram: &DatagramInfo,
        frame: CryptoRef,
    ) -> Result<(), TransportError> {
        self.crypto_stream.on_crypto_frame(frame)?;

        Ok(())
    }

    fn handle_ack_frame<A: AckRanges>(
        &mut self,
        datagram: &DatagramInfo,
        frame: Ack<A>,
    ) -> Result<(), TransportError> {
        // TODO process ack delay
        // TODO process ECN

        for ack_range in frame.ack_ranges() {
            let (start, end) = ack_range.into_inner();

            let pn_space = PacketNumberSpace::Initial;
            let start = pn_space.new_packet_number(start);
            let end = pn_space.new_packet_number(end);

            let ack_set = start..=end;

            self.tx_packet_numbers.on_packet_ack(datagram, &ack_set)?;
            self.crypto_stream.on_packet_ack(&ack_set);
            self.ack_manager.on_packet_ack(datagram, &ack_set);
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

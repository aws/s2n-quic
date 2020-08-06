use crate::{
    connection::{self, ConnectionInterests, ConnectionTransmissionContext},
    frame_exchange_interests::FrameExchangeInterestProvider,
    processed_packet::ProcessedPacket,
    space::{
        rx_packet_numbers::AckManager, ApplicationTransmission, HandshakeStatus, PacketSpace,
        TxPacketNumbers,
    },
    stream::AbstractStreamManager,
};
use s2n_codec::EncoderBuffer;
use s2n_quic_core::{
    crypto::CryptoSuite,
    frame::{
        ack::AckRanges, crypto::CryptoRef, stream::StreamRef, Ack, DataBlocked, HandshakeDone,
        MaxData, MaxStreamData, MaxStreams, NewConnectionID, NewToken, PathChallenge, PathResponse,
        ResetStream, RetireConnectionID, StopSending, StreamDataBlocked, StreamsBlocked,
    },
    inet::DatagramInfo,
    packet::{
        encoding::{PacketEncoder, PacketEncodingError},
        number::{PacketNumber, PacketNumberSpace, SlidingWindow, SlidingWindowError},
        short::{KeyPhase, Short, SpinBit},
    },
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
    /// Records if the handshake is pending or done, which is communicated to the peer
    pub handshake_status: HandshakeStatus,
    processed_packet_numbers: SlidingWindow,
}

impl<Config: connection::Config> ApplicationSpace<Config> {
    pub fn new(
        crypto: <Config::TLSSession as CryptoSuite>::OneRTTCrypto,
        now: Timestamp,
        stream_manager: AbstractStreamManager<Config::Stream>,
        ack_manager: AckManager,
    ) -> Self {
        Self {
            tx_packet_numbers: TxPacketNumbers::new(PacketNumberSpace::ApplicationData, now),
            ack_manager,
            key_phase: KeyPhase::Zero,
            spin_bit: SpinBit::Zero,
            stream_manager,
            crypto,
            handshake_status: HandshakeStatus::default(),
            processed_packet_numbers: SlidingWindow::default(),
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
        let packet_number = self.tx_packet_numbers.next();
        let packet_number_encoder = self.packet_number_encoder();
        let key_phase = self.key_phase;
        let spin_bit = self.spin_bit;
        let (crypto, payload) = self.transmission(context, packet_number);

        let packet = Short {
            destination_connection_id: context.path.destination_connection_id.as_ref(),
            spin_bit,
            key_phase,
            packet_number,
            payload,
        };

        let (_protected_packet, buffer) =
            packet.encode_packet(crypto, packet_number_encoder, buffer)?;

        Ok(buffer)
    }

    pub fn interests(&self) -> ConnectionInterests {
        // TODO: Will default() prevent finalization, since it might set finalization to false?
        ConnectionInterests::default()
            + self.ack_manager.frame_exchange_interests()
            + self.handshake_status.frame_exchange_interests()
            + self.stream_manager.interests()
    }

    /// Signals the handshake is done
    pub fn on_handshake_done(&mut self) {
        if Config::ENDPOINT_TYPE.is_server() {
            self.handshake_status.on_handshake_done();
        }
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
    ) -> (
        &'a <Config::TLSSession as CryptoSuite>::OneRTTCrypto,
        ApplicationTransmission<'a, Config::Stream>,
    ) {
        // TODO: What about ZeroRTTCrypto?
        (
            &self.crypto,
            ApplicationTransmission {
                ack_manager: &mut self.ack_manager,
                context,
                handshake_status: &mut self.handshake_status,
                packet_number,
                stream_manager: &mut self.stream_manager,
                tx_packet_numbers: &mut self.tx_packet_numbers,
            },
        )
    }
}

impl<Config: connection::Config> PacketSpace for ApplicationSpace<Config> {
    const INVALID_FRAME_ERROR: &'static str = "invalid frame in application space";

    fn handle_crypto_frame(
        &mut self,
        _datagram: &DatagramInfo,
        frame: CryptoRef,
    ) -> Result<(), TransportError> {
        Err(TransportError::INTERNAL_ERROR
            .with_reason("crypto frames are not currently supported in application space")
            .with_frame_type(frame.tag().into()))
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

            let pn_space = PacketNumberSpace::ApplicationData;
            let start = pn_space.new_packet_number(start);
            let end = pn_space.new_packet_number(end);

            let ack_set = start..=end;

            self.tx_packet_numbers.on_packet_ack(datagram, &ack_set)?;
            self.handshake_status.on_packet_ack(&ack_set);
            self.stream_manager.on_packet_ack(&ack_set);
            self.ack_manager.on_packet_ack(datagram, &ack_set);
        }

        Ok(())
    }

    fn handle_stream_frame(
        &mut self,
        _datagram: &DatagramInfo,
        frame: StreamRef,
    ) -> Result<(), TransportError> {
        self.stream_manager.on_data(&frame)
    }

    fn handle_data_blocked_frame(
        &mut self,
        _datagram: &DatagramInfo,
        frame: DataBlocked,
    ) -> Result<(), TransportError> {
        self.stream_manager.on_data_blocked(frame)
    }

    fn handle_max_data_frame(
        &mut self,
        _datagram: &DatagramInfo,
        frame: MaxData,
    ) -> Result<(), TransportError> {
        self.stream_manager.on_max_data(frame)
    }

    fn handle_max_stream_data_frame(
        &mut self,
        _datagram: &DatagramInfo,
        frame: MaxStreamData,
    ) -> Result<(), TransportError> {
        self.stream_manager.on_max_stream_data(&frame)
    }

    fn handle_max_streams_frame(
        &mut self,
        _datagram: &DatagramInfo,
        frame: MaxStreams,
    ) -> Result<(), TransportError> {
        self.stream_manager.on_max_streams(&frame)
    }

    fn handle_reset_stream_frame(
        &mut self,
        _datagram: &DatagramInfo,
        frame: ResetStream,
    ) -> Result<(), TransportError> {
        self.stream_manager.on_reset_stream(&frame)
    }

    fn handle_stop_sending_frame(
        &mut self,
        _datagram: &DatagramInfo,
        frame: StopSending,
    ) -> Result<(), TransportError> {
        self.stream_manager.on_stop_sending(&frame)
    }

    fn handle_stream_data_blocked_frame(
        &mut self,
        _datagram: &DatagramInfo,
        frame: StreamDataBlocked,
    ) -> Result<(), TransportError> {
        self.stream_manager.on_stream_data_blocked(&frame)
    }

    fn handle_streams_blocked_frame(
        &mut self,
        _datagram: &DatagramInfo,
        frame: StreamsBlocked,
    ) -> Result<(), TransportError> {
        self.stream_manager.on_streams_blocked(&frame)
    }

    fn handle_new_token_frame(
        &mut self,
        _datagram: &DatagramInfo,
        frame: NewToken,
    ) -> Result<(), TransportError> {
        // TODO
        eprintln!("UNIMPLEMENTED APPLICATION FRAME {:?}", frame);
        Ok(())
    }

    fn handle_new_connection_id_frame(
        &mut self,
        _datagram: &DatagramInfo,
        frame: NewConnectionID,
    ) -> Result<(), TransportError> {
        // TODO
        eprintln!("UNIMPLEMENTED APPLICATION FRAME {:?}", frame);
        Ok(())
    }

    fn handle_retire_connection_id_frame(
        &mut self,
        _datagram: &DatagramInfo,
        frame: RetireConnectionID,
    ) -> Result<(), TransportError> {
        // TODO
        eprintln!("UNIMPLEMENTED APPLICATION FRAME {:?}", frame);
        Ok(())
    }

    fn handle_path_challenge_frame(
        &mut self,
        _datagram: &DatagramInfo,
        frame: PathChallenge,
    ) -> Result<(), TransportError> {
        // TODO
        eprintln!("UNIMPLEMENTED APPLICATION FRAME {:?}", frame);
        Ok(())
    }

    fn handle_path_response_frame(
        &mut self,
        _datagram: &DatagramInfo,
        frame: PathResponse,
    ) -> Result<(), TransportError> {
        // TODO map this frame to a Path
        eprintln!("UNIMPLEMENTED APPLICATION FRAME {:?}", frame);
        Ok(())
    }

    fn handle_handshake_done_frame(
        &mut self,
        _datagram: &DatagramInfo,
        frame: HandshakeDone,
    ) -> Result<(), TransportError> {
        //= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#19.20
        //# A server MUST
        //# treat receipt of a HANDSHAKE_DONE frame as a connection error of type
        //# PROTOCOL_VIOLATION.

        if Config::ENDPOINT_TYPE.is_server() {
            return Err(TransportError::PROTOCOL_VIOLATION
                .with_reason("Clients MUST NOT send HANDSHAKE_DONE frames")
                .with_frame_type(frame.tag().into()));
        }

        self.handshake_status.on_handshake_done_received();

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

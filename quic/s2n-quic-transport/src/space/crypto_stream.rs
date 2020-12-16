use crate::{
    buffer::StreamReceiveBuffer,
    contexts::WriteContext,
    sync::{ChunkToFrameWriter, DataSender, OutgoingDataFlowController},
    transmission,
};
use s2n_quic_core::{
    ack_set::AckSet,
    frame::{crypto::CryptoRef, MaxPayloadSizeForFrame},
    packet::number::PacketNumber,
    transport::error::TransportError,
    varint::VarInt,
};

pub type TxCryptoStream = DataSender<CryptoFlowController, CryptoChunkToFrameWriter>;

/// Serializes and writes `Crypto` frames
#[derive(Debug, Default)]
pub struct CryptoChunkToFrameWriter {}

impl ChunkToFrameWriter for CryptoChunkToFrameWriter {
    type StreamId = ();

    fn get_max_frame_size(&self, _stream_id: Self::StreamId, data_size: usize) -> usize {
        CryptoRef::get_max_frame_size(data_size)
    }

    fn max_payload_size(
        &self,
        _stream_id: Self::StreamId,
        max_frame_size: usize,
        offset: VarInt,
    ) -> MaxPayloadSizeForFrame {
        CryptoRef::max_payload_size(max_frame_size, offset)
    }

    fn write_value_as_frame<W: WriteContext>(
        &self,
        _stream_id: Self::StreamId,
        offset: VarInt,
        data: &[u8],
        _is_last_frame: bool,
        _is_fin: bool,
        context: &mut W,
    ) -> Option<PacketNumber> {
        context.write_frame(&CryptoRef { offset, data })
    }
}

/// Serializes and writes `Crypto` frames
#[derive(Debug, Default)]
pub struct CryptoFlowController {}

/// There is no control flow for crypto data
impl OutgoingDataFlowController for CryptoFlowController {
    fn acquire_flow_control_window(&mut self, _min_offset: VarInt, _size: usize) -> VarInt {
        VarInt::MAX
    }

    fn is_blocked(&self) -> bool {
        false
    }

    fn clear_blocked(&mut self) {}

    fn finish(&mut self) {}
}

#[derive(Debug)]
pub struct CryptoStream {
    pub tx: TxCryptoStream,
    pub rx: StreamReceiveBuffer,
    pub is_finished: bool,
}

const TX_MAX_BUFFER_CAPACITY: u32 = 4096;

impl Default for CryptoStream {
    fn default() -> Self {
        Self::new()
    }
}

impl CryptoStream {
    pub fn new() -> Self {
        Self {
            tx: TxCryptoStream::new(Default::default(), TX_MAX_BUFFER_CAPACITY),
            rx: StreamReceiveBuffer::default(),
            is_finished: false,
        }
    }

    pub fn can_send(&self) -> bool {
        !self.is_finished && self.tx.available_buffer_space() > 0
    }

    pub fn finish(&mut self) -> Result<(), TransportError> {
        self.is_finished = true;
        self.tx.finish();

        //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#4.1.3
        //# When TLS
        //# provides keys for a higher encryption level, if there is data from
        //# a previous encryption level that TLS has not consumed, this MUST
        //# be treated as a connection error of type PROTOCOL_VIOLATION.
        if self.rx.is_empty() {
            Ok(())
        } else {
            Err(TransportError::PROTOCOL_VIOLATION)
        }
    }

    pub fn on_crypto_frame(&mut self, frame: CryptoRef) -> Result<(), TransportError> {
        //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#4.1.3
        //# *  If the packet is from a previously installed encryption level, it
        //# MUST NOT contain data that extends past the end of previously
        //# received data in that flow.  Implementations MUST treat any
        //# violations of this requirement as a connection error of type
        //# PROTOCOL_VIOLATION.

        if self.is_finished && frame.offset + frame.data.len() > self.rx.total_received_len() {
            return Err(TransportError::PROTOCOL_VIOLATION);
        }

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#7.5
        //= type=TODO
        //= tracking-issue=356
        //= feature=Crypto buffer limits
        //# Implementations MUST support buffering at least 4096 bytes of data
        //# received in out-of-order CRYPTO frames.

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#7.5
        //= type=TODO
        //= tracking-issue=356
        //= feature=Crypto buffer limits
        //# Endpoints MAY choose to
        //# allow more data to be buffered during the handshake.

        //TODO we need to limit the buffer size here

        self.rx.write_at(frame.offset, frame.data).map_err(|_| {
            //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#7.5
            //# If an endpoint does not expand its buffer, it MUST close
            //# the connection with a CRYPTO_BUFFER_EXCEEDED error code.

            TransportError::CRYPTO_BUFFER_EXCEEDED
        })?;

        Ok(())
    }

    /// This method gets called when a packet delivery got acknowledged
    pub fn on_packet_ack<A: AckSet>(&mut self, ack_set: &A) {
        self.tx.on_packet_ack(ack_set);
    }

    /// This method gets called when a packet loss is reported
    pub fn on_packet_loss<A: AckSet>(&mut self, ack_set: &A) {
        self.tx.on_packet_loss(ack_set);
    }
}

impl transmission::interest::Provider for CryptoStream {
    fn transmission_interest(&self) -> transmission::Interest {
        self.tx.transmission_interest()
    }
}

use crate::{
    buffer::StreamReceiveBuffer,
    contexts::WriteContext,
    frame_exchange_interests::{FrameExchangeInterestProvider, FrameExchangeInterests},
    sync::{ChunkToFrameWriter, DataSender, OutgoingDataFlowController},
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

    pub fn finish(&mut self) {
        self.is_finished = true;
        self.tx.finish();
    }

    pub fn on_crypto_frame(&mut self, frame: CryptoRef) -> Result<(), TransportError> {
        // TODO check that the data ends before the previous finalized length
        if self.is_finished {
            // the frame is a duplicate
            return Ok(());
        }

        self.rx
            .write_at(frame.offset, frame.data)
            .map_err(|_| TransportError::CRYPTO_BUFFER_EXCEEDED)?;

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

impl FrameExchangeInterestProvider for CryptoStream {
    fn frame_exchange_interests(&self) -> FrameExchangeInterests {
        self.tx.frame_exchange_interests()
    }
}

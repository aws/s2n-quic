use crate::{
    connection::ConnectionTransmissionContext,
    contexts::WriteContext,
    frame_exchange_interests::{FrameExchangeInterestProvider, FrameExchangeInterests},
    recovery,
    space::{rx_packet_numbers::AckManager, HandshakeStatus, TxPacketNumbers},
    stream::{AbstractStreamManager, StreamTrait},
};
use s2n_codec::{Encoder, EncoderBuffer, EncoderValue};
use s2n_quic_core::{
    frame::{
        ack_elicitation::{AckElicitable, AckElicitation},
        congestion_controlled::CongestionControlled,
        Padding,
    },
    packet::{encoding::PacketPayloadEncoder, number::PacketNumber},
    time::Timestamp,
};

pub struct ApplicationTransmission<'a, StreamType: StreamTrait> {
    pub ack_manager: &'a mut AckManager,
    pub context: &'a ConnectionTransmissionContext,
    pub handshake_status: &'a mut HandshakeStatus,
    pub packet_number: PacketNumber,
    pub recovery_manager: &'a mut recovery::Manager,
    pub stream_manager: &'a mut AbstractStreamManager<StreamType>,
    pub tx_packet_numbers: &'a mut TxPacketNumbers,
}

impl<'a, StreamType: StreamTrait> PacketPayloadEncoder for ApplicationTransmission<'a, StreamType> {
    fn encoding_size_hint<E: Encoder>(&mut self, _encoder: &E, minimum_len: usize) -> usize {
        // TODO ask the stream manager. We need to return something that assures that
        // - either Padding gets written
        // - or the StreamManager can write a `Stream` frame without length information (which must
        //   be the last frame) of sufficient size.
        // Note that we can not write a short `Stream` frame without length information and then
        // pad it.
        if self.frame_exchange_interests().transmission {
            minimum_len.max(1)
        } else {
            0
        }
    }

    fn encode(&mut self, buffer: &mut EncoderBuffer, minimum_len: usize, overhead_len: usize) {
        debug_assert!(
            buffer.is_empty(),
            "the implementation assumes an empty buffer"
        );

        let mut context = ApplicationTransmissionContext {
            ack_elicitation: AckElicitation::default(),
            buffer,
            context: self.context,
            packet_number: self.packet_number,
            is_congestion_controlled: false,
        };

        let did_send_ack = self.ack_manager.on_transmit(&mut context);

        // TODO: Handle errors
        let _ = self.handshake_status.on_transmit(&mut context);
        let _ = self.stream_manager.on_transmit(&mut context);
        self.recovery_manager.on_transmit(&mut context);

        if did_send_ack {
            // inform the ack manager the packet is populated
            self.ack_manager.on_transmit_complete(&mut context);
        }

        if !context.buffer.is_empty() {
            // Add padding up to minimum_len
            let length = minimum_len.saturating_sub(context.buffer.len());
            if length > 0 {
                context.buffer.encode(&Padding { length });
            }

            self.tx_packet_numbers.on_transmit(self.packet_number);

            self.recovery_manager.on_packet_sent(
                context.packet_number,
                context.ack_elicitation,
                context.is_congestion_controlled,
                overhead_len + context.buffer.len(),
                context.current_time(),
            )
        }
    }
}

pub struct ApplicationTransmissionContext<'a, 'b> {
    ack_elicitation: AckElicitation,
    buffer: &'a mut EncoderBuffer<'b>,
    context: &'a ConnectionTransmissionContext,
    packet_number: PacketNumber,
    is_congestion_controlled: bool,
}

impl<'a, 'b> WriteContext for ApplicationTransmissionContext<'a, 'b> {
    type ConnectionContext = ConnectionTransmissionContext;

    fn current_time(&self) -> Timestamp {
        self.context.timestamp
    }

    fn connection_context(&self) -> &Self::ConnectionContext {
        &self.context
    }

    fn write_frame<Frame: EncoderValue + AckElicitable + CongestionControlled>(
        &mut self,
        frame: &Frame,
    ) -> Option<PacketNumber> {
        if frame.encoding_size() > self.buffer.remaining_capacity() {
            return None;
        }
        self.buffer.encode(frame);
        self.ack_elicitation |= frame.ack_elicitation();
        self.is_congestion_controlled |= frame.is_congestion_controlled();

        Some(self.packet_number)
    }

    fn ack_elicitation(&self) -> AckElicitation {
        self.ack_elicitation
    }

    fn packet_number(&self) -> PacketNumber {
        self.packet_number
    }

    fn reserve_minimum_space_for_frame(&mut self, min_size: usize) -> Result<usize, ()> {
        let cap = self.buffer.remaining_capacity();
        if cap < min_size {
            Err(())
        } else {
            Ok(cap)
        }
    }
}

impl<'a, StreamType: StreamTrait> FrameExchangeInterestProvider
    for ApplicationTransmission<'a, StreamType>
{
    fn frame_exchange_interests(&self) -> FrameExchangeInterests {
        FrameExchangeInterests {
            transmission: self.stream_manager.interests().transmission,
            ..Default::default()
        } + self.ack_manager.frame_exchange_interests()
            + self.handshake_status.frame_exchange_interests()
    }
}

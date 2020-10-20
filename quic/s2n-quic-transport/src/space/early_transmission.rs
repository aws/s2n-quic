use crate::{
    connection::{self, transmission::Outcome, ConnectionTransmissionContext},
    contexts::WriteContext,
    frame_exchange_interests::{FrameExchangeInterestProvider, FrameExchangeInterests},
    recovery,
    space::{rx_packet_numbers::AckManager, CryptoStream, TxPacketNumbers},
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

pub struct EarlyTransmission<'a, Config: connection::Config> {
    pub ack_manager: &'a mut AckManager,
    pub context: &'a ConnectionTransmissionContext<'a, Config>,
    pub crypto_stream: &'a mut CryptoStream,
    pub packet_number: PacketNumber,
    pub recovery_manager: &'a mut recovery::Manager,
    pub tx_packet_numbers: &'a mut TxPacketNumbers,
    pub outcome: &'a mut Outcome,
}

impl<'a, Config: connection::Config> PacketPayloadEncoder for EarlyTransmission<'a, Config> {
    fn encoding_size_hint<E: Encoder>(&mut self, _encoder: &E, minimum_len: usize) -> usize {
        // TODO return the minimum length required to encode a crypto frame + a certain amount of data
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

        let mut context = EarlyTransmissionContext {
            outcome: self.outcome,
            packet_number: self.packet_number,
            buffer,
            context: self.context,
        };

        let did_send_ack = self.ack_manager.on_transmit(&mut context);

        let _ = self.crypto_stream.tx.on_transmit((), &mut context);
        self.recovery_manager.on_transmit(&mut context);

        if did_send_ack {
            // inform the ack manager the packet is populated
            self.ack_manager.on_transmit_complete(&mut context);
        }

        // TODO add required padding if client

        if !context.buffer.is_empty() {
            // Add padding up to minimum_len
            let length = minimum_len.saturating_sub(context.buffer.len());
            if length > 0 {
                context.buffer.encode(&Padding { length });
            }

            self.tx_packet_numbers.on_transmit(self.packet_number);
            self.outcome.bytes_sent = overhead_len + buffer.len();
        }
    }
}

pub struct EarlyTransmissionContext<'a, 'b, Config: connection::Config> {
    outcome: &'a mut Outcome,
    buffer: &'a mut EncoderBuffer<'b>,
    context: &'a ConnectionTransmissionContext<'a, Config>,
    packet_number: PacketNumber,
}

impl<'a, 'b, Config: connection::Config> WriteContext for EarlyTransmissionContext<'a, 'b, Config> {
    type ConnectionContext = ConnectionTransmissionContext<'a, Config>;

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
        self.outcome.ack_elicitation |= frame.ack_elicitation();
        self.outcome.is_congestion_controlled |= frame.is_congestion_controlled();
        Some(self.packet_number)
    }

    fn ack_elicitation(&self) -> AckElicitation {
        self.outcome.ack_elicitation
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

impl<'a, Config: connection::Config> FrameExchangeInterestProvider
    for EarlyTransmission<'a, Config>
{
    fn frame_exchange_interests(&self) -> FrameExchangeInterests {
        FrameExchangeInterests::default()
            + self.ack_manager.frame_exchange_interests()
            + self.crypto_stream.frame_exchange_interests()
    }
}

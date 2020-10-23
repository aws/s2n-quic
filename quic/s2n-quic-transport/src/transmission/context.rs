use crate::{
    connection::{self, ConnectionTransmissionContext},
    contexts::WriteContext,
    transmission,
};
use s2n_codec::{Encoder, EncoderBuffer, EncoderValue};
use s2n_quic_core::{
    frame::{
        ack_elicitation::{AckElicitable, AckElicitation},
        congestion_controlled::CongestionControlled,
    },
    packet::number::PacketNumber,
    time::Timestamp,
};

pub struct Context<'a, 'b, Config: connection::Config> {
    pub outcome: &'a mut transmission::Outcome,
    pub buffer: &'a mut EncoderBuffer<'b>,
    pub context: &'a ConnectionTransmissionContext<'a, Config>,
    pub packet_number: PacketNumber,
    pub transmission_constraint: transmission::Constraint,
}

impl<'a, 'b, Config: connection::Config> WriteContext for Context<'a, 'b, Config> {
    type ConnectionContext = ConnectionTransmissionContext<'a, Config>;

    fn current_time(&self) -> Timestamp {
        self.context.timestamp
    }

    fn connection_context(&self) -> &Self::ConnectionContext {
        &self.context
    }

    fn transmission_constraint(&self) -> transmission::Constraint {
        self.transmission_constraint
    }

    fn write_frame<Frame: EncoderValue + AckElicitable + CongestionControlled>(
        &mut self,
        frame: &Frame,
    ) -> Option<PacketNumber> {
        if frame.encoding_size() > self.buffer.remaining_capacity() {
            return None;
        }

        if cfg!(debug_assertions) {
            match self.transmission_constraint() {
                transmission::Constraint::AmplificationLimited => {
                    unreachable!("frames should not be written when we're amplication limited")
                }
                transmission::Constraint::CongestionLimited => {
                    assert!(!frame.is_congestion_controlled());
                }
                transmission::Constraint::RetransmissionOnly => {}
                transmission::Constraint::None => {}
            }
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

// Overrides a context's transmission constraint to allow only retransmissions to be written to
// packets
pub struct RetransmissionContext<'a, C: WriteContext> {
    context: &'a mut C,
}

impl<'a, C: WriteContext> RetransmissionContext<'a, C> {
    pub fn new(context: &'a mut C) -> Self {
        Self { context }
    }
}

impl<'a, C: WriteContext> WriteContext for RetransmissionContext<'a, C> {
    type ConnectionContext = C::ConnectionContext;

    fn current_time(&self) -> Timestamp {
        self.context.current_time()
    }

    fn connection_context(&self) -> &Self::ConnectionContext {
        self.context.connection_context()
    }

    fn transmission_constraint(&self) -> transmission::Constraint {
        debug_assert!(
            self.context.transmission_constraint().can_retransmit(),
            "retransmission ability should be checked before using RetransmissionContext"
        );

        transmission::Constraint::RetransmissionOnly
    }

    fn write_frame<Frame: EncoderValue + AckElicitable + CongestionControlled>(
        &mut self,
        frame: &Frame,
    ) -> Option<PacketNumber> {
        self.context.write_frame(frame)
    }

    fn ack_elicitation(&self) -> AckElicitation {
        self.context.ack_elicitation()
    }

    fn packet_number(&self) -> PacketNumber {
        self.context.packet_number()
    }

    fn reserve_minimum_space_for_frame(&mut self, min_size: usize) -> Result<usize, ()> {
        self.context.reserve_minimum_space_for_frame(min_size)
    }
}

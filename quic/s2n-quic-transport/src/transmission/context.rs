// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{contexts::WriteContext, endpoint, transmission, transmission::Mode};
use core::marker::PhantomData;
use s2n_codec::{Encoder, EncoderBuffer, EncoderValue};
use s2n_quic_core::{
    frame::{
        ack_elicitation::{AckElicitable, AckElicitation},
        congestion_controlled::CongestionControlled,
        path_validation::Probing as PathValidationProbing,
    },
    packet::number::PacketNumber,
    time::Timestamp,
};

pub struct Context<'a, 'b, Config: endpoint::Config> {
    pub outcome: &'a mut transmission::Outcome,
    pub buffer: &'a mut EncoderBuffer<'b>,
    pub packet_number: PacketNumber,
    pub transmission_constraint: transmission::Constraint,
    pub transmission_mode: transmission::Mode,
    pub timestamp: Timestamp,
    pub header_len: usize,
    pub tag_len: usize,
    pub config: PhantomData<Config>,
}

impl<'a, 'b, Config: endpoint::Config> WriteContext for Context<'a, 'b, Config> {
    fn current_time(&self) -> Timestamp {
        self.timestamp
    }

    fn transmission_constraint(&self) -> transmission::Constraint {
        self.transmission_constraint
    }

    fn transmission_mode(&self) -> Mode {
        self.transmission_mode
    }

    fn remaining_capacity(&self) -> usize {
        self.buffer.remaining_capacity()
    }

    fn write_frame<
        Frame: EncoderValue + AckElicitable + CongestionControlled + PathValidationProbing,
    >(
        &mut self,
        frame: &Frame,
    ) -> Option<PacketNumber> {
        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#9.1
        //# A packet containing only probing frames is a "probing packet", and a
        //# packet containing any other frame is a "non-probing packet".
        if self.transmission_mode == Mode::PathValidation {
            debug_assert!(frame.probe().is_validation_probing());
        }

        if cfg!(debug_assertions) {
            match self.transmission_constraint() {
                transmission::Constraint::AmplificationLimited => {
                    unreachable!("frames should not be written when we're amplification limited")
                }
                transmission::Constraint::CongestionLimited => {
                    assert!(!frame.is_congestion_controlled());
                }
                transmission::Constraint::RetransmissionOnly => {}
                transmission::Constraint::None => {}
            }
        }

        self.write_frame_forced(frame)
    }

    fn write_frame_forced<Frame: EncoderValue + AckElicitable + CongestionControlled>(
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

    fn local_endpoint_type(&self) -> endpoint::Type {
        Config::ENDPOINT_TYPE
    }

    fn header_len(&self) -> usize {
        self.header_len
    }

    fn tag_len(&self) -> usize {
        self.tag_len
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
    fn current_time(&self) -> Timestamp {
        self.context.current_time()
    }

    fn transmission_constraint(&self) -> transmission::Constraint {
        debug_assert!(
            self.context.transmission_constraint().can_retransmit(),
            "retransmission ability should be checked before using RetransmissionContext"
        );

        transmission::Constraint::RetransmissionOnly
    }

    fn transmission_mode(&self) -> Mode {
        self.context.transmission_mode()
    }

    fn remaining_capacity(&self) -> usize {
        self.context.remaining_capacity()
    }

    fn write_frame<
        Frame: EncoderValue + AckElicitable + CongestionControlled + PathValidationProbing,
    >(
        &mut self,
        frame: &Frame,
    ) -> Option<PacketNumber> {
        self.context.write_frame(frame)
    }

    fn write_frame_forced<Frame: EncoderValue + AckElicitable + CongestionControlled>(
        &mut self,
        frame: &Frame,
    ) -> Option<PacketNumber> {
        self.context.write_frame_forced(frame)
    }

    fn ack_elicitation(&self) -> AckElicitation {
        self.context.ack_elicitation()
    }

    fn packet_number(&self) -> PacketNumber {
        self.context.packet_number()
    }

    fn local_endpoint_type(&self) -> endpoint::Type {
        self.context.local_endpoint_type()
    }

    fn header_len(&self) -> usize {
        self.context.header_len()
    }

    fn tag_len(&self) -> usize {
        self.context.tag_len()
    }
}

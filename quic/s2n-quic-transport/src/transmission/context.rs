// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{contexts::WriteContext, endpoint, path, transmission, transmission::Mode};
use core::marker::PhantomData;
use s2n_codec::{Encoder, EncoderBuffer, EncoderValue};
use s2n_quic_core::{
    event::{self, IntoEvent, Publisher as _},
    frame::{
        ack_elicitation::{AckElicitable, AckElicitation},
        congestion_controlled::CongestionControlled,
        path_validation::Probing as PathValidationProbing,
    },
    packet::number::PacketNumber,
    time::Timestamp,
};

pub struct Context<'a, 'b, 'sub, Config: endpoint::Config> {
    pub outcome: &'a mut transmission::Outcome,
    pub buffer: &'a mut EncoderBuffer<'b>,
    pub packet_number: PacketNumber,
    pub transmission_constraint: transmission::Constraint,
    pub transmission_mode: transmission::Mode,
    pub timestamp: Timestamp,
    pub header_len: usize,
    pub tag_len: usize,
    pub config: PhantomData<Config>,
    pub path_id: path::Id,
    pub publisher:
        &'a mut event::PublisherSubscriber<'sub, <Config as endpoint::Config>::EventSubscriber>,
}

impl<'a, 'b, 'sub, Config: endpoint::Config> Context<'a, 'b, 'sub, Config> {
    #[inline]
    fn check_frame_constraint<
        Frame: AckElicitable + CongestionControlled + PathValidationProbing,
    >(
        &self,
        frame: &Frame,
    ) {
        // only apply checks with debug_assertions enabled
        if !cfg!(debug_assertions) {
            return;
        }

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#9
        //# Servers do not send non-
        //# probing packets (see Section 9.1) toward a client address until they
        //# see a non-probing packet from that address.
        //
        // The transmission_mode PathValidation is used by the non-active path
        // to only transmit probing frames. A packet containing only probing
        // frames is also a probing packet.
        if self.transmission_mode == Mode::PathValidationOnly {
            assert!(frame.path_validation().is_probing());
        }

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
}

impl<'a, 'b, 'sub, Config: endpoint::Config> WriteContext for Context<'a, 'b, 'sub, Config> {
    fn current_time(&self) -> Timestamp {
        self.timestamp
    }

    #[inline]
    fn transmission_constraint(&self) -> transmission::Constraint {
        self.transmission_constraint
    }

    #[inline]
    fn transmission_mode(&self) -> Mode {
        self.transmission_mode
    }

    #[inline]
    fn remaining_capacity(&self) -> usize {
        self.buffer.remaining_capacity()
    }

    #[inline]
    fn write_frame<Frame>(&mut self, frame: &Frame) -> Option<PacketNumber>
    where
        Frame: EncoderValue + AckElicitable + CongestionControlled + PathValidationProbing,
        for<'frame> &'frame Frame: IntoEvent<event::builder::Frame>,
    {
        self.check_frame_constraint(frame);
        self.write_frame_forced(frame)
    }

    #[inline]
    fn write_fitted_frame<Frame>(&mut self, frame: &Frame) -> PacketNumber
    where
        Frame: EncoderValue + AckElicitable + CongestionControlled + PathValidationProbing,
        for<'frame> &'frame Frame: IntoEvent<event::builder::Frame>,
    {
        self.check_frame_constraint(frame);
        debug_assert!(frame.encoding_size() <= self.buffer.remaining_capacity());

        self.buffer.encode(frame);
        self.outcome.ack_elicitation |= frame.ack_elicitation();
        self.outcome.is_congestion_controlled |= frame.is_congestion_controlled();

        self.publisher.on_frame_sent(event::builder::FrameSent {
            packet_header: event::builder::PacketHeader {
                packet_type: self.packet_number.into_event(),
                version: self.publisher.quic_version(),
            },
            path_id: self.path_id.into_event(),
            frame: frame.into_event(),
        });
        self.packet_number
    }

    fn write_frame_forced<Frame>(&mut self, frame: &Frame) -> Option<PacketNumber>
    where
        Frame: EncoderValue + AckElicitable + CongestionControlled,
        for<'frame> &'frame Frame: IntoEvent<event::builder::Frame>,
    {
        if frame.encoding_size() > self.buffer.remaining_capacity() {
            return None;
        }

        self.buffer.encode(frame);
        self.outcome.ack_elicitation |= frame.ack_elicitation();
        self.outcome.is_congestion_controlled |= frame.is_congestion_controlled();

        self.publisher.on_frame_sent(event::builder::FrameSent {
            packet_header: event::builder::PacketHeader {
                packet_type: self.packet_number.into_event(),
                version: self.publisher.quic_version(),
            },
            path_id: self.path_id.into_event(),
            frame: frame.into_event(),
        });
        Some(self.packet_number)
    }

    #[inline]
    fn ack_elicitation(&self) -> AckElicitation {
        self.outcome.ack_elicitation
    }

    #[inline]
    fn packet_number(&self) -> PacketNumber {
        self.packet_number
    }

    #[inline]
    fn local_endpoint_type(&self) -> endpoint::Type {
        Config::ENDPOINT_TYPE
    }

    #[inline]
    fn header_len(&self) -> usize {
        self.header_len
    }

    #[inline]
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
    #[inline]
    fn current_time(&self) -> Timestamp {
        self.context.current_time()
    }

    #[inline]
    fn transmission_constraint(&self) -> transmission::Constraint {
        debug_assert!(
            self.context.transmission_constraint().can_retransmit(),
            "retransmission ability should be checked before using RetransmissionContext"
        );

        transmission::Constraint::RetransmissionOnly
    }

    #[inline]
    fn transmission_mode(&self) -> Mode {
        self.context.transmission_mode()
    }

    #[inline]
    fn remaining_capacity(&self) -> usize {
        self.context.remaining_capacity()
    }

    #[inline]
    fn write_frame<Frame>(&mut self, frame: &Frame) -> Option<PacketNumber>
    where
        Frame: EncoderValue + AckElicitable + CongestionControlled + PathValidationProbing,
        for<'frame> &'frame Frame: IntoEvent<event::builder::Frame>,
    {
        self.context.write_frame(frame)
    }

    #[inline]
    fn write_fitted_frame<Frame>(&mut self, frame: &Frame) -> PacketNumber
    where
        Frame: EncoderValue + AckElicitable + CongestionControlled + PathValidationProbing,
        for<'frame> &'frame Frame: IntoEvent<event::builder::Frame>,
    {
        self.context.write_fitted_frame(frame)
    }

    fn write_frame_forced<Frame>(&mut self, frame: &Frame) -> Option<PacketNumber>
    where
        Frame: EncoderValue + AckElicitable + CongestionControlled,
        for<'frame> &'frame Frame: IntoEvent<event::builder::Frame>,
    {
        self.context.write_frame_forced(frame)
    }

    #[inline]
    fn ack_elicitation(&self) -> AckElicitation {
        self.context.ack_elicitation()
    }

    #[inline]
    fn packet_number(&self) -> PacketNumber {
        self.context.packet_number()
    }

    #[inline]
    fn local_endpoint_type(&self) -> endpoint::Type {
        self.context.local_endpoint_type()
    }

    #[inline]
    fn header_len(&self) -> usize {
        self.context.header_len()
    }

    #[inline]
    fn tag_len(&self) -> usize {
        self.context.tag_len()
    }
}

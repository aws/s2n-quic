// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::transmission::{self, WriteContext};
use core::ops::RangeInclusive;
use s2n_codec::EncoderValue;
use s2n_quic_core::{frame, packet::number::PacketNumberSpace};

pub struct Payload<'a> {
    pub connection_close: &'a frame::ConnectionClose<'a>,
    pub packet_number_space: PacketNumberSpace,
}

impl<'a> super::Payload for Payload<'a> {
    fn size_hint(&self, range: RangeInclusive<usize>) -> usize {
        (*range.start()).max(self.connection_close.encoding_size())
    }

    fn on_transmit<W: WriteContext>(&mut self, context: &mut W) {
        context.write_frame(self.connection_close);
    }

    fn packet_number_space(&self) -> PacketNumberSpace {
        self.packet_number_space
    }
}

impl<'a> transmission::interest::Provider for Payload<'a> {
    #[inline]
    fn transmission_interest<Q: transmission::interest::Query>(
        &self,
        query: &mut Q,
    ) -> transmission::interest::Result {
        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-34.txt#3
        //# Packets containing frames besides ACK or CONNECTION_CLOSE frames
        //# count toward congestion control limits and are considered in-
        //# flight.

        // this packet only contains a CONNECTION_CLOSE so bypass the CC
        query.on_forced()
    }
}

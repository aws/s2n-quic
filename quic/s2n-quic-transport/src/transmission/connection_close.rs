// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::transmission::{self, WriteContext};
use core::ops::RangeInclusive;
use s2n_codec::EncoderValue;
use s2n_quic_core::frame;

pub struct Payload<'a> {
    pub connection_close: &'a frame::ConnectionClose<'a>,
}

impl super::Payload for Payload<'_> {
    fn size_hint(&self, range: RangeInclusive<usize>) -> usize {
        (*range.start()).max(self.connection_close.encoding_size())
    }

    fn on_transmit<W: WriteContext>(&mut self, context: &mut W) {
        context.write_frame(self.connection_close);
    }
}

impl transmission::interest::Provider for Payload<'_> {
    #[inline]
    fn transmission_interest<Q: transmission::interest::Query>(
        &self,
        query: &mut Q,
    ) -> transmission::interest::Result {
        //= https://www.rfc-editor.org/rfc/rfc9002#section-3
        //# Packets containing frames besides ACK or CONNECTION_CLOSE frames
        //# count toward congestion control limits and are considered to be in
        //# flight.

        // this packet only contains a CONNECTION_CLOSE so bypass the CC
        query.on_forced()
    }
}

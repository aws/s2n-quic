// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    endpoint,
    stream::{AbstractStreamManager, StreamTrait as Stream},
    transmission::WriteContext,
};
use s2n_quic_core::{
    datagram::{Endpoint, Sender},
    frame,
};

pub struct Manager<Config: endpoint::Config> {
    sender: <<Config as endpoint::Config>::DatagramEndpoint as Endpoint>::Sender,
    // TODO: Remove this warning once Receiver is implemented
    #[allow(dead_code)]
    receiver: <<Config as endpoint::Config>::DatagramEndpoint as Endpoint>::Receiver,
}

impl<Config: endpoint::Config> Manager<Config> {
    pub fn new(
        sender: <<Config as endpoint::Config>::DatagramEndpoint as Endpoint>::Sender,
        receiver: <<Config as endpoint::Config>::DatagramEndpoint as Endpoint>::Receiver,
    ) -> Self {
        Self { sender, receiver }
    }

    pub fn on_transmit<S: Stream, W: WriteContext>(
        &mut self,
        context: &mut W,
        stream_manager: &mut AbstractStreamManager<S>,
    ) {
        let mut packet = Packet {
            context,
            pending_streams: stream_manager.pending_streams(),
        };
        self.sender.on_transmit(&mut packet);
    }
}
struct Packet<'a, C: WriteContext> {
    context: &'a mut C,
    pending_streams: bool,
}

const FRAME_TYPE_LEN: usize = 1;

impl<'a, C: WriteContext> s2n_quic_core::datagram::Packet for Packet<'a, C> {
    /// Returns the remaining space in the packet
    fn remaining_capacity(&self) -> usize {
        self.context.remaining_capacity()
    }

    /// Returns the largest datagram that can fit in space remaining in the packet
    fn maximum_datagram_payload(&self) -> usize {
        let space = self.context.remaining_capacity();
        // In the case where the user writes the largest datagram possible
        // we don't factor in the size of the Length field as
        // it will be the last frame in the packet.
        space - FRAME_TYPE_LEN
    }

    /// Writes a single datagram to a packet
    fn write_datagram(&mut self, data: &[u8]) {
        let remaining_capacity = self.context.remaining_capacity();
        let data_len = data.len();
        let is_last_frame = remaining_capacity == FRAME_TYPE_LEN + data_len;
        let frame = frame::Datagram {
            is_last_frame,
            data,
        };
        self.context.write_frame(&frame);
    }

    // Returns whether or not there is reliable data ready to send
    fn pending_streams(&self) -> bool {
        self.pending_streams
    }
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    endpoint,
    stream::{AbstractStreamManager, StreamTrait as Stream},
    transmission::{interest, WriteContext},
};
use s2n_codec::EncoderValue;
use s2n_quic_core::{
    datagram::{Endpoint, Sender, WriteError},
    frame,
    varint::VarInt,
};

// Contains the datagram sender and receiver implementations.
//
// Used to call datagram callbacks during packet transmission and
// packet processing.
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

    /// A callback that allows users to write datagrams directly to the packet.
    pub fn on_transmit<S: Stream, W: WriteContext>(
        &mut self,
        context: &mut W,
        stream_manager: &mut AbstractStreamManager<S>,
    ) {
        let mut packet = Packet {
            context,
            has_pending_streams: stream_manager.has_pending_streams(),
        };
        self.sender.on_transmit(&mut packet);
    }
}

impl<Config: endpoint::Config> interest::Provider for Manager<Config> {
    #[inline]
    fn transmission_interest<Q: interest::Query>(&self, query: &mut Q) -> interest::Result {
        if self.sender.has_transmission_interest() {
            query.on_new_data()?;
        }
        Ok(())
    }
}

struct Packet<'a, C: WriteContext> {
    context: &'a mut C,
    has_pending_streams: bool,
}

const FRAME_TYPE_LEN: usize = 1;
const MAX_LEN_VALUE: usize = 8;

impl<'a, C: WriteContext> s2n_quic_core::datagram::Packet for Packet<'a, C> {
    /// Returns the remaining space in the packet
    fn remaining_capacity(&self) -> usize {
        let space = self.context.remaining_capacity();
        // Remove the frame type length and the maximum length value
        space
            .saturating_sub(FRAME_TYPE_LEN)
            .saturating_sub(MAX_LEN_VALUE)
    }

    /// Writes a single datagram to a packet
    fn write_datagram(&mut self, data: &[u8]) -> Result<(), WriteError> {
        let remaining_capacity = self.context.remaining_capacity();
        let data_len = data.len();
        let is_last_frame = remaining_capacity == FRAME_TYPE_LEN + data_len;

        if !is_last_frame {
            let encoded_length = match VarInt::new(data_len as u64) {
                Ok(encoded_length) => encoded_length,
                Err(_e) => return Err(WriteError::DatagramIsTooLarge),
            };
            // Calculates the complete size of the encoded datagram, including the
            // VarInt size and frame size
            let encoded_datagram_size = encoded_length.encoding_size() + FRAME_TYPE_LEN + data_len;

            if encoded_datagram_size > remaining_capacity {
                return Err(WriteError::DatagramIsTooLarge);
            }
        }

        let frame = frame::Datagram {
            is_last_frame,
            data,
        };
        self.context.write_frame(&frame);

        Ok(())
    }

    // Returns whether or not there is reliable data ready to send
    fn has_pending_streams(&self) -> bool {
        self.has_pending_streams
    }
}

#[test]
fn write_datagrams() {
    use crate::{
        contexts::testing::{MockWriteContext, OutgoingFrameBuffer},
        transmission::{Constraint, Mode},
    };
    use s2n_quic_core::datagram::Packet as _;

    let mut frame_buffer = OutgoingFrameBuffer::new();
    let packet_size = 5;
    frame_buffer.set_max_packet_size(Some(packet_size));
    let mut write_context = MockWriteContext::new(
        s2n_quic_platform::time::now(),
        &mut frame_buffer,
        Constraint::None,
        Mode::Normal,
        endpoint::Type::Server,
    );

    let mut packet = Packet {
        context: &mut write_context,
        has_pending_streams: false,
    };
    let max_datagram = vec![1, 2, 3];
    let too_large_datagram = vec![1, 2, 3, 4];
    assert!(packet.write_datagram(&max_datagram).is_ok());
    packet.context.frame_buffer.clear();
    assert!(packet.write_datagram(&too_large_datagram).is_err());
}

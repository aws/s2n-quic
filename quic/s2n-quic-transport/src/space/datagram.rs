// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    endpoint,
    stream::{AbstractStreamManager, StreamTrait as Stream},
    transmission::{interest, WriteContext},
};
use s2n_codec::EncoderValue;
use s2n_quic_core::{
    datagram::{Endpoint, Receiver, Sender, WriteError},
    frame::{self, datagram::DatagramRef},
    varint::VarInt,
};

// Contains the datagram sender and receiver implementations.
//
// Used to call datagram callbacks during packet transmission and
// packet processing.
pub struct Manager<Config: endpoint::Config> {
    sender: <<Config as endpoint::Config>::DatagramEndpoint as Endpoint>::Sender,
    receiver: <<Config as endpoint::Config>::DatagramEndpoint as Endpoint>::Receiver,
    max_datagram_payload: u64,
}

impl<Config: endpoint::Config> Manager<Config> {
    pub fn new(
        sender: <<Config as endpoint::Config>::DatagramEndpoint as Endpoint>::Sender,
        receiver: <<Config as endpoint::Config>::DatagramEndpoint as Endpoint>::Receiver,
        max_datagram_payload: u64,
    ) -> Self {
        Self {
            sender,
            receiver,
            max_datagram_payload,
        }
    }

    /// A callback that allows users to write datagrams directly to the packet.
    pub fn on_transmit<S: Stream, W: WriteContext>(
        &mut self,
        context: &mut W,
        stream_manager: &mut AbstractStreamManager<S>,
        datagrams_prioritized: bool,
    ) {
        let mut packet = Packet {
            context,
            has_pending_streams: stream_manager.has_pending_streams(),
            datagrams_prioritized,
            max_datagram_payload: self.max_datagram_payload,
        };
        self.sender.on_transmit(&mut packet);
    }

    // A callback that allows users to access datagrams directly after they are
    // received.
    pub fn on_datagram_frame(&self, datagram: DatagramRef) {
        self.receiver.on_datagram(datagram.data);
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
    datagrams_prioritized: bool,
    max_datagram_payload: u64,
}

impl<'a, C: WriteContext> s2n_quic_core::datagram::Packet for Packet<'a, C> {
    /// Returns the remaining space in the packet
    fn remaining_capacity(&self) -> usize {
        let space = self.context.remaining_capacity();
        // Remove the frame type length and the maximum length value
        space
            .saturating_sub(frame::datagram::DATAGRAM_TAG.encoding_size())
            .saturating_sub(VarInt::MAX.encoding_size())
    }

    /// Writes a single datagram to a packet
    fn write_datagram(&mut self, data: &[u8]) -> Result<(), WriteError> {
        if data.len() as u64 > self.max_datagram_payload {
            return Err(WriteError::ExceedsPeerTransportLimits);
        }
        let remaining_capacity = self.context.remaining_capacity();
        let data_len = data.len();
        let is_last_frame =
            remaining_capacity == frame::datagram::DATAGRAM_TAG.encoding_size() + data_len;
        let frame = frame::Datagram {
            is_last_frame,
            data,
        };
        self.context
            .write_frame(&frame)
            .ok_or(WriteError::ExceedsPacketCapacity)?;

        Ok(())
    }

    /// Returns whether or not there is reliable data ready to send
    fn has_pending_streams(&self) -> bool {
        self.has_pending_streams
    }

    /// Returns whether or not datagrams are prioritized in this packet or not
    fn datagrams_prioritized(&self) -> bool {
        self.datagrams_prioritized
    }
}

#[test]
// Check that our default on_transmit function doesn't continue to pop datagrams
// off the send queue if the remaining packet space is too small to send datagrams.
fn has_written_test() {
    use crate::contexts::testing::{MockWriteContext, OutgoingFrameBuffer};
    use s2n_quic_core::transmission;

    let mut default_sender = s2n_quic_core::datagram::default::DefaultSender::builder()
        .build()
        .unwrap();
    let datagram_0 = bytes::Bytes::from_static(&[1, 2, 3]);
    let datagram_1 = bytes::Bytes::from_static(&[4, 5, 6]);
    default_sender.send_datagram(datagram_0);
    default_sender.send_datagram(datagram_1);

    let mut frame_buffer = OutgoingFrameBuffer::new();
    // Packet size is just enough to write the first datagram with some
    // room left over, but not enough to write the second.
    frame_buffer.set_max_packet_size(Some(15));
    let mut context = MockWriteContext::new(
        s2n_quic_platform::time::now(),
        &mut frame_buffer,
        transmission::Constraint::None,
        transmission::Mode::Normal,
        endpoint::Type::Server,
    );
    let mut packet = Packet {
        context: &mut context,
        has_pending_streams: false,
        datagrams_prioritized: false,
        max_datagram_payload: 100,
    };
    default_sender.on_transmit(&mut packet);

    // Successfully wrote one datagram to the packet
    assert_eq!(packet.context.frame_buffer.frames.len(), 1);
    // Packet still has capacity to write datagrams
    assert!(s2n_quic_core::datagram::Packet::remaining_capacity(&packet) > 0);
    // Send queue is not completely depleted
    assert!(!default_sender.queue.is_empty());
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use s2n_quic_core::{
    ack,
    inet::DatagramInfo,
    packet::number::{PacketNumber, PacketNumberSpace},
    time::Timestamp,
    transport,
    varint::VarInt,
};

/// Context for tracking transmission of packet numbers
#[derive(Debug)]
pub struct TxPacketNumbers {
    largest_sent_acked: (PacketNumber, Timestamp),
    next: PacketNumber,
}

impl TxPacketNumbers {
    pub fn new(packet_space: PacketNumberSpace, now: Timestamp) -> Self {
        let initial_packet_number = packet_space.new_packet_number(VarInt::from_u8(0));
        Self {
            largest_sent_acked: (initial_packet_number, now),
            next: initial_packet_number,
        }
    }

    /// This method gets called when a packet delivery got acknowledged
    pub fn on_packet_ack<A: ack::Set>(
        &mut self,
        datagram: &DatagramInfo,
        ack_set: &A,
    ) -> Result<(), transport::Error> {
        let largest = ack_set.largest();

        //= https://www.rfc-editor.org/rfc/rfc9000#section-13.1
        //# An endpoint SHOULD treat receipt of an acknowledgment for a packet it
        //# did not send as a connection error of type PROTOCOL_VIOLATION, if it
        //# is able to detect the condition.

        if largest >= self.next {
            return Err(transport::Error::PROTOCOL_VIOLATION
                .with_reason("received an ACK for a packet that was not sent"));
        }

        // record the largest packet acked
        if largest > self.largest_sent_acked.0 {
            self.largest_sent_acked = (largest, datagram.timestamp);
        }

        Ok(())
    }

    /// Called after a packet is transmitted with a given packet number
    pub fn on_transmit(&mut self, packet_number: PacketNumber) {
        //= https://www.rfc-editor.org/rfc/rfc9000#section-12.3
        //# A QUIC endpoint MUST NOT reuse a packet number within the same packet
        //# number space in one connection.  If the packet number for sending
        //# reaches 2^62-1, the sender MUST close the connection without sending
        //# a CONNECTION_CLOSE frame or any further packets; an endpoint MAY send
        //# a Stateless Reset (Section 10.3) in response to further packets that
        //# it receives.

        // Assuming a constant TX rate of 10,000 packets/second, it would take
        // (2^62 - 1)packets / 10,000packets-per-second  ~= 4.6e14seconds = ~14,000,000years to overflow.
        // It's probably OK to just panic as there will be other things to worry about at that point.

        self.next = packet_number.next().expect("packet number overflowed");
    }

    /// Returns the next packet number in the sequence
    pub fn next(&self) -> PacketNumber {
        self.next
    }

    /// Returns the largest packet number that the endpoint sent that
    /// was ACKed by the peer
    pub fn largest_sent_packet_number_acked(&self) -> PacketNumber {
        self.largest_sent_acked.0
    }
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use s2n_quic_core::{
    ack,
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
    // skipped packet number used for detecting an Optimistic Ack attack
    skip_packet_number: Option<PacketNumber>,
}

impl TxPacketNumbers {
    pub fn new(packet_space: PacketNumberSpace, now: Timestamp) -> Self {
        let initial_packet_number = packet_space.new_packet_number(VarInt::from_u8(0));
        Self {
            largest_sent_acked: (initial_packet_number, now),
            next: initial_packet_number,
            skip_packet_number: None,
        }
    }

    /// This method gets called when a packet delivery got acknowledged
    pub fn on_packet_ack<A: ack::Set>(
        &mut self,
        timestamp: Timestamp,
        ack_set: &A,
        lowest_tracking_packet_number: PacketNumber,
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

        if let Some(skip_packet_number) = self.skip_packet_number {
            debug_assert_eq!(
                skip_packet_number.space(),
                PacketNumberSpace::ApplicationData,
                "only start skipping packets after the handshake is complete"
            );

            //= https://www.rfc-editor.org/rfc/rfc9000#section-21.4
            //# An endpoint that acknowledges packets it has not received might cause
            //# a congestion controller to permit sending at rates beyond what the
            //# network supports.  An endpoint MAY skip packet numbers when sending
            //# packets to detect this behavior.  An endpoint can then immediately
            //# close the connection with a connection error of type PROTOCOL_VIOLATION
            if ack_set.contains(skip_packet_number) {
                return Err(transport::Error::PROTOCOL_VIOLATION
                    .with_reason("received an ACK for a packet that was not sent"));
            }

            // Verify the peer didn't send the skipped packet number before clearing it.
            //
            // Packet skipping is implemented to mitigate the Optimistic Ack attack. To
            // correctly detect an attack, we track a skipped packet until all packets
            // <= skip_packet_number + 1 have been marked as acknowledged or lost. The assumption
            // is that the attacker gains little from ACKing a packet less than the
            // largest packet ACKed.
            //
            // For example:
            // Assume we are initially tracking packets 2-9 and have skipped
            // packet 4. The skip_packet_number + 1 packet is calculated as packet 5 (p).
            // We validate the peer behavior and can clear skip_packet_number once we stop
            // tracking packet 5.
            //
            // ```
            // Initial tracking state: skipped packet 4 (s = 4)
            //    [ 2 3 5 6 7 8 9 ]
            //          p
            //
            //    RX: AckRange(2..4)
            //
            // Not verified: still tracking (p)
            //    [ 5 6 7 8 9 ]
            //      p
            //
            //    RX: AckRange(5)
            //
            // Verified: peer did not send an ack for packet 4
            //    [ 6 7 8 9 ]
            // ```
            let skip_plus_one = skip_packet_number
                .next()
                .expect("expect next packet_number");
            if lowest_tracking_packet_number > skip_plus_one {
                self.skip_packet_number = None;
            }
        }

        // record the largest packet acked
        if largest > self.largest_sent_acked.0 {
            self.largest_sent_acked = (largest, timestamp);
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

    // Only skip a packet number after verifying the peer did not send
    // the previous one.
    pub fn should_skip_packet_number(&self) -> bool {
        self.skip_packet_number.is_none()
    }

    pub fn set_skip_packet_number(&mut self, skip_packet_number: PacketNumber) {
        debug_assert_eq!(
            skip_packet_number.space(),
            PacketNumberSpace::ApplicationData,
            "only start skipping packets after the handshake is complete"
        );
        self.skip_packet_number = Some(skip_packet_number);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::time::Duration;
    use s2n_quic_core::packet::number::PacketNumberRange;

    // Test behavior around tracking and clearing skip_packet_number value.
    //
    // Test setup:
    //   ack_set: [ 2 3 4 5 6]
    //   skip_packet_number: 3
    //   skip_plus_one: 4
    #[test]
    fn test_err_on_skip_packet_number() {
        let skip_packet_number =
            PacketNumberSpace::ApplicationData.new_packet_number(VarInt::from_u8(3));
        let mut skip_plus_one =
            PacketNumberSpace::ApplicationData.new_packet_number(VarInt::from_u8(4));
        let start = PacketNumberSpace::ApplicationData.new_packet_number(VarInt::from_u8(2));
        let end = PacketNumberSpace::ApplicationData.new_packet_number(VarInt::from_u8(6));
        let mut ack_set = PacketNumberRange::new(start, end);

        let timestamp = unsafe { Timestamp::from_duration(Duration::from_millis(10)) };
        let mut tx = TxPacketNumbers::new(PacketNumberSpace::ApplicationData, timestamp);
        // Set initial tx state. `tx.next` needs to be > than the largest ack received
        tx.next = end.next().unwrap();

        // Happy case: skip_packet_number = None
        assert!(tx.on_packet_ack(timestamp, &ack_set, skip_plus_one).is_ok());

        // Error if ack_set contains skip_packet_number
        tx.set_skip_packet_number(skip_packet_number);
        assert!(tx
            .on_packet_ack(timestamp, &ack_set, skip_plus_one)
            .is_err());

        // Pass if ack_set doesn't contain skip_packet_number
        ack_set = PacketNumberRange::new(skip_plus_one, end);
        assert!(tx.on_packet_ack(timestamp, &ack_set, skip_plus_one).is_ok());

        // Assert that skip_packet_number has not been cleared since it has not been verified.
        assert!(tx.skip_packet_number.is_some());

        // Assert skip_packet_number is cleared once we stop tracking the skip_plus_one packet number
        skip_plus_one = skip_plus_one.next().unwrap();
        assert!(tx.on_packet_ack(timestamp, &ack_set, skip_plus_one).is_ok());
        assert!(tx.skip_packet_number.is_none());
    }
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    recovery::{context::Context, manager::Manager},
    space::rx_packet_numbers::ack_ranges::AckRanges,
};
use core::ops::RangeInclusive;
use s2n_quic_core::{
    connection, frame,
    inet::DatagramInfo,
    packet::number::{PacketNumberRange, PacketNumberSpace},
    recovery::{congestion_controller, cubic::CubicCongestionController, CongestionController},
    time::Timestamp,
    varint::VarInt,
};

#[derive(Debug)]
pub struct Endpoint;

impl congestion_controller::Endpoint for Endpoint {
    type CongestionController = CubicCongestionController;

    fn new_congestion_controller(
        &mut self,
        _: congestion_controller::PathInfo,
    ) -> Self::CongestionController {
        todo!()
    }
}

// Helper function that will call on_ack_frame with the given packet numbers
pub fn ack_packets<CC: CongestionController, Ctx: Context<CC>>(
    range: RangeInclusive<u8>,
    ack_receive_time: Timestamp,
    context: &mut Ctx,
    manager: &mut Manager,
) {
    let acked_packets = PacketNumberRange::new(
        manager
            .space
            .new_packet_number(VarInt::from_u8(*range.start())),
        manager
            .space
            .new_packet_number(VarInt::from_u8(*range.end())),
    );

    let datagram = DatagramInfo {
        timestamp: ack_receive_time,
        remote_address: Default::default(),
        payload_len: 0,
        ecn: Default::default(),
        destination_connection_id: connection::LocalId::TEST_ID,
    };

    let mut ack_range = AckRanges::new(acked_packets.count());

    for acked_packet in acked_packets {
        ack_range.insert_packet_number(acked_packet);
    }

    let frame = frame::Ack {
        ack_delay: VarInt::from_u8(10),
        ack_ranges: (&ack_range),
        ecn_counts: None,
    };

    let _ = manager.on_ack_frame(&datagram, frame, context);

    for packet in acked_packets {
        assert!(manager.sent_packets.get(packet).is_none());
    }
}

#[cfg(any(test, feature = "testing"))]
mod test {
    use super::*;
    use crate::{path, recovery::context::testing::MockContext, transmission};
    use core::time::Duration;
    use s2n_quic_core::frame::ack_elicitation::AckElicitation;

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#5.1
    //= type=test
    //# To avoid generating multiple RTT samples for a single packet, an ACK
    //# frame SHOULD NOT be used to update RTT estimates if it does not newly
    //# acknowledge the largest acknowledged packet.
    #[test]
    fn no_rtt_update_when_not_acknowledging_the_largest_acknowledged_packet() {
        let space = PacketNumberSpace::ApplicationData;
        let mut manager = Manager::new(space, Duration::from_millis(100));
        let packet_bytes = 128;
        let mut context = MockContext::default();

        let time_sent = s2n_quic_platform::time::now() + Duration::from_secs(10);

        // Send 2 packets
        manager.on_packet_sent(
            space.new_packet_number(VarInt::from_u8(0)),
            transmission::Outcome {
                ack_elicitation: AckElicitation::Eliciting,
                is_congestion_controlled: true,
                bytes_sent: packet_bytes,
                packet_number: space.new_packet_number(VarInt::from_u8(1)),
            },
            time_sent,
            path::Id::new(0),
            &mut context,
        );
        manager.on_packet_sent(
            space.new_packet_number(VarInt::from_u8(1)),
            transmission::Outcome {
                ack_elicitation: AckElicitation::Eliciting,
                is_congestion_controlled: true,
                bytes_sent: packet_bytes,
                packet_number: space.new_packet_number(VarInt::from_u8(1)),
            },
            time_sent,
            path::Id::new(0),
            &mut context,
        );

        assert_eq!(manager.sent_packets.iter().count(), 2);

        // Ack packet 1
        let ack_receive_time = time_sent + Duration::from_millis(500);
        ack_packets(1..=1, ack_receive_time, &mut context, &mut manager);

        // New rtt estimate because the largest packet was newly acked
        assert_eq!(context.path.congestion_controller.on_rtt_update, 1);
        assert_eq!(
            manager.largest_acked_packet,
            Some(space.new_packet_number(VarInt::from_u8(1)))
        );
        assert_eq!(
            context.path.rtt_estimator.latest_rtt(),
            Duration::from_millis(500)
        );
        assert_eq!(1, context.on_rtt_update_count);

        // Ack packets 0 and 1
        let ack_receive_time = time_sent + Duration::from_millis(1500);
        ack_packets(0..=1, ack_receive_time, &mut context, &mut manager);

        // No new rtt estimate because the largest packet was not newly acked
        assert_eq!(context.path.congestion_controller.on_rtt_update, 1);
        assert_eq!(
            context.path.rtt_estimator.latest_rtt(),
            Duration::from_millis(500)
        );
        assert_eq!(1, context.on_rtt_update_count);
    }

    #[test]
    fn persistent_congestion() {
        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#7.6.2
        //= type=test
        //# A sender that does not have state for all packet
        //# number spaces or an implementation that cannot compare send times
        //# across packet number spaces MAY use state for just the packet number
        //# space that was acknowledged.
        let space = PacketNumberSpace::ApplicationData;
        let mut manager = Manager::new(space, Duration::from_millis(100));
        let mut context = MockContext::default();
        let time_zero = s2n_quic_platform::time::now() + Duration::from_secs(10);
        // The RFC doesn't mention it, but it is implied that the first RTT sample has already
        // been received when this example begins, otherwise packet #2 would not be considered
        // part of the persistent congestion period.
        context.path.rtt_estimator.update_rtt(
            Duration::from_millis(10),
            Duration::from_millis(700),
            s2n_quic_platform::time::now(),
            true,
            space,
        );

        let outcome = transmission::Outcome {
            ack_elicitation: AckElicitation::Eliciting,
            is_congestion_controlled: true,
            bytes_sent: 1,
            packet_number: space.new_packet_number(VarInt::from_u8(1)),
        };

        // t=0: Send packet #1 (app data)
        manager.on_packet_sent(
            space.new_packet_number(VarInt::from_u8(1)),
            outcome,
            time_zero,
            path::Id::new(0),
            &mut context,
        );

        // t=1: Send packet #2 (app data)
        manager.on_packet_sent(
            space.new_packet_number(VarInt::from_u8(2)),
            outcome,
            time_zero + Duration::from_secs(1),
            path::Id::new(0),
            &mut context,
        );

        // t=1.2: Recv acknowledgement of #1
        ack_packets(
            1..=1,
            time_zero + Duration::from_millis(1200),
            &mut context,
            &mut manager,
        );

        // t=2-6: Send packets #3 - #7 (app data)
        for t in 2..=6 {
            manager.on_packet_sent(
                space.new_packet_number(VarInt::from_u8(t + 1)),
                outcome,
                time_zero + Duration::from_secs(t.into()),
                path::Id::new(0),
                &mut context,
            );
        }

        // t=8: Send packet #8 (PTO 1)
        manager.on_packet_sent(
            space.new_packet_number(VarInt::from_u8(8)),
            outcome,
            time_zero + Duration::from_secs(8),
            path::Id::new(0),
            &mut context,
        );

        // t=12: Send packet #9 (PTO 2)
        manager.on_packet_sent(
            space.new_packet_number(VarInt::from_u8(9)),
            outcome,
            time_zero + Duration::from_secs(12),
            path::Id::new(0),
            &mut context,
        );

        // t=12.2: Recv acknowledgement of #9
        ack_packets(
            9..=9,
            time_zero + Duration::from_millis(12200),
            &mut context,
            &mut manager,
        );

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#7.6.3
        //# Packets 2 through 8 are declared lost when the acknowledgement for
        //# packet 9 is received at t = 12.2.
        assert_eq!(7, context.on_packet_loss_count);

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#7.6.3
        //# The congestion period is calculated as the time between the oldest
        //# and newest lost packets: 8 - 1 = 7.
        assert!(
            context.path.rtt_estimator.persistent_congestion_threshold() < Duration::from_secs(7)
        );
        assert_eq!(
            Some(true),
            context.path.congestion_controller.persistent_congestion
        );
        assert_eq!(context.path.rtt_estimator.first_rtt_sample(), None);

        // t=20: Send packet #10
        manager.on_packet_sent(
            space.new_packet_number(VarInt::from_u8(10)),
            outcome,
            time_zero + Duration::from_secs(20),
            path::Id::new(0),
            &mut context,
        );

        // t=21: Recv acknowledgement of #10
        ack_packets(
            10..=10,
            time_zero + Duration::from_secs(21),
            &mut context,
            &mut manager,
        );

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#5.2
        //= type=test
        //# Endpoints SHOULD set the min_rtt to the newest RTT sample after
        //# persistent congestion is established.
        assert_eq!(context.path.rtt_estimator.min_rtt(), Duration::from_secs(1));
        assert_eq!(
            context.path.rtt_estimator.smoothed_rtt(),
            Duration::from_secs(1)
        );
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#7.6
    //= type=test
    #[test]
    fn persistent_congestion_multiple_periods() {
        let space = PacketNumberSpace::ApplicationData;
        let mut manager = Manager::new(space, Duration::from_millis(100));
        let mut context = MockContext::new(Duration::from_millis(0), true);
        let time_zero = s2n_quic_platform::time::now() + Duration::from_secs(10);

        let outcome = transmission::Outcome {
            ack_elicitation: AckElicitation::Eliciting,
            is_congestion_controlled: true,
            bytes_sent: 1,
            packet_number: space.new_packet_number(VarInt::from_u8(1)),
        };

        // t=0: Send packet #1 (app data)
        manager.on_packet_sent(
            space.new_packet_number(VarInt::from_u8(1)),
            outcome,
            time_zero,
            path::Id::new(0),
            &mut context,
        );

        // t=1: Send packet #2 (app data)
        manager.on_packet_sent(
            space.new_packet_number(VarInt::from_u8(2)),
            outcome,
            time_zero + Duration::from_secs(1),
            path::Id::new(0),
            &mut context,
        );

        // t=1.2: Recv acknowledgement of #1
        ack_packets(
            1..=1,
            time_zero + Duration::from_millis(1200),
            &mut context,
            &mut manager,
        );

        // t=2-6: Send packets #3 - #7 (app data)
        for t in 2..=6 {
            manager.on_packet_sent(
                space.new_packet_number(VarInt::from_u8(t + 1)),
                outcome,
                time_zero + Duration::from_secs(t.into()),
                path::Id::new(0),
                &mut context,
            );
        }

        // Skip packet #8, which ends one persistent congestion period.

        // t=8: Send packet #9 (app data)
        manager.on_packet_sent(
            space.new_packet_number(VarInt::from_u8(9)),
            outcome,
            time_zero + Duration::from_secs(8),
            path::Id::new(0),
            &mut context,
        );

        // t=20: Send packet #10 (app data)
        manager.on_packet_sent(
            space.new_packet_number(VarInt::from_u8(10)),
            outcome,
            time_zero + Duration::from_secs(20),
            path::Id::new(0),
            &mut context,
        );

        // t=30: Send packet #11 (app data)
        manager.on_packet_sent(
            space.new_packet_number(VarInt::from_u8(11)),
            outcome,
            time_zero + Duration::from_secs(30),
            path::Id::new(0),
            &mut context,
        );

        // t=30.2: Recv acknowledgement of #11
        ack_packets(
            11..=11,
            time_zero + Duration::from_millis(30200),
            &mut context,
            &mut manager,
        );

        // Packets 2 though 7 and 9-10 should be lost
        assert_eq!(8, context.on_packet_loss_count);

        // The largest contiguous period of lost packets is #9 (sent at t8) to #10 (sent at t20)
        assert!(
            context.path.rtt_estimator.persistent_congestion_threshold() < Duration::from_secs(12)
        );
        assert_eq!(
            Some(true),
            context.path.congestion_controller.persistent_congestion
        );
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#7.6.2
    //= type=test
    //# The persistent congestion period SHOULD NOT start until there is at
    //# least one RTT sample.
    #[test]
    fn persistent_congestion_period_does_not_start_until_rtt_sample() {
        let space = PacketNumberSpace::ApplicationData;
        let mut manager = Manager::new(space, Duration::from_millis(100));
        let mut context = MockContext::default();
        let time_zero = s2n_quic_platform::time::now() + Duration::from_secs(10);

        let outcome = transmission::Outcome {
            ack_elicitation: AckElicitation::Eliciting,
            is_congestion_controlled: true,
            bytes_sent: 1,
            packet_number: space.new_packet_number(VarInt::from_u8(1)),
        };

        // t=0: Send packet #1 (app data)
        manager.on_packet_sent(
            space.new_packet_number(VarInt::from_u8(1)),
            outcome,
            time_zero,
            path::Id::new(0),
            &mut context,
        );

        // t=10: Send packet #2 (app data)
        manager.on_packet_sent(
            space.new_packet_number(VarInt::from_u8(2)),
            outcome,
            time_zero + Duration::from_secs(10),
            path::Id::new(0),
            &mut context,
        );

        // t=20: Send packet #3 (app data)
        manager.on_packet_sent(
            space.new_packet_number(VarInt::from_u8(3)),
            outcome,
            time_zero + Duration::from_secs(20),
            path::Id::new(0),
            &mut context,
        );

        // t=20.1: Recv acknowledgement of #3. The first RTT sample is collected
        //         now, at t=20.1
        ack_packets(
            3..=3,
            time_zero + Duration::from_millis(20100),
            &mut context,
            &mut manager,
        );

        // There is no persistent congestion, because the lost packets were all
        // sent prior to the first RTT sample
        assert_eq!(context.path.congestion_controller.on_packets_lost, 1);
        assert_eq!(
            context.path.congestion_controller.persistent_congestion,
            Some(false)
        );
    }
}

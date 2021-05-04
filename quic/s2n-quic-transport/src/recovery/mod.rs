// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

pub use context::Context;
pub use manager::*;
/// re-export core
pub use s2n_quic_core::recovery::*;
pub use sent_packets::*;

mod context;
mod manager;
mod pto;
mod recovery_testing;
mod sent_packets;

#[cfg(any(test, feature = "testing"))]
pub mod testing {
    use super::*;
    use crate::{
        recovery::{context::Context, manager::Manager},
        space::rx_packet_numbers::ack_ranges::AckRanges,
    };
    use core::ops::RangeInclusive;
    use s2n_quic_core::{
        connection, frame, inet::DatagramInfo, packet::number::PacketNumberRange,
        recovery::CongestionController, time::Timestamp, varint::VarInt,
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
}

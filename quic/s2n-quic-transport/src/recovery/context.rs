// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::path::Path;
use core::time::Duration;
use s2n_quic_core::{
    connection, endpoint,
    inet::DatagramInfo,
    packet::number::{PacketNumber, PacketNumberRange},
    recovery::{
        congestion_controller::testing::mock::CongestionController as MockCongestionController,
        CongestionController, RttEstimator,
    },
    transport,
};

pub trait Context<CC: CongestionController> {
    const ENDPOINT_TYPE: endpoint::Type;

    fn is_handshake_confirmed(&self) -> bool;

    fn path(&self) -> &Path<CC>;

    fn path_mut(&mut self) -> &mut Path<CC>;

    fn validate_packet_ack(
        &mut self,
        datagram: &DatagramInfo,
        packet_number_range: &PacketNumberRange,
    ) -> Result<(), transport::Error>;

    fn on_new_packet_ack(
        &mut self,
        datagram: &DatagramInfo,
        packet_number_range: &PacketNumberRange,
    );
    fn on_packet_ack(&mut self, datagram: &DatagramInfo, packet_number_range: &PacketNumberRange);
    fn on_packet_loss(&mut self, packet_number_range: &PacketNumberRange);
    fn on_rtt_update(&mut self);
}

pub(crate) mod mock {
    use super::*;
    // use core::time::Duration;
    // use s2n_quic_core::{
    //     connection, endpoint,
    //     inet::DatagramInfo,
    //     packet::number::{PacketNumber, PacketNumberRange},
    //     recovery::{
    //         congestion_controller::testing::mock::CongestionController as MockCongestionController,
    //         RttEstimator,
    //     },
    //     transport,
    // };
    use std::collections::HashSet;

    pub(crate) struct MockContext {
        pub validate_packet_ack_count: u8,
        pub on_new_packet_ack_count: u8,
        pub on_packet_ack_count: u8,
        pub on_packet_loss_count: u8,
        pub on_rtt_update_count: u8,
        pub path: Path<MockCongestionController>,
        pub lost_packets: HashSet<PacketNumber>,
    }

    impl MockContext {
        pub fn new(max_ack_delay: Duration, peer_validated: bool) -> Self {
            let path = Path::new(
                Default::default(),
                connection::PeerId::TEST_ID,
                RttEstimator::new(max_ack_delay),
                MockCongestionController::default(),
                peer_validated,
            );
            Self {
                validate_packet_ack_count: 0,
                on_new_packet_ack_count: 0,
                on_packet_ack_count: 0,
                on_packet_loss_count: 0,
                on_rtt_update_count: 0,
                path,
                lost_packets: HashSet::default(),
            }
        }
    }

    impl Default for MockContext {
        fn default() -> Self {
            Self::new(Duration::from_millis(10), true)
        }
    }

    impl Context<MockCongestionController> for MockContext {
        const ENDPOINT_TYPE: endpoint::Type = endpoint::Type::Client;

        fn is_handshake_confirmed(&self) -> bool {
            true
        }

        fn path(&self) -> &Path<MockCongestionController> {
            &self.path
        }

        fn path_mut(&mut self) -> &mut Path<MockCongestionController> {
            &mut self.path
        }

        fn validate_packet_ack(
            &mut self,
            _datagram: &DatagramInfo,
            _packet_number_range: &PacketNumberRange,
        ) -> Result<(), transport::Error> {
            self.validate_packet_ack_count += 1;
            Ok(())
        }

        fn on_new_packet_ack(
            &mut self,
            _datagram: &DatagramInfo,
            _packet_number_range: &PacketNumberRange,
        ) {
            self.on_new_packet_ack_count += 1;
        }

        fn on_packet_ack(
            &mut self,
            _datagram: &DatagramInfo,
            _packet_number_range: &PacketNumberRange,
        ) {
            self.on_packet_ack_count += 1;
        }

        fn on_packet_loss(&mut self, packet_number_range: &PacketNumberRange) {
            self.on_packet_loss_count += 1;
            self.lost_packets.insert(packet_number_range.start());
        }

        fn on_rtt_update(&mut self) {
            self.on_rtt_update_count += 1;
        }
    }
}

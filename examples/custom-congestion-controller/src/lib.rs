// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

/// Example implementation of a custom congestion controller algorithm.
///
/// NOTE: The `CongestionController` trait is considered unstable and may be subject to change
///       in a future release.
pub mod custom_congestion_controller {
    use s2n_quic::provider::{
        congestion_controller,
        congestion_controller::{
            CongestionController, Publisher, RandomGenerator, RttEstimator, Timestamp,
        },
    };

    /// Define a congestion controller containing any state you wish to track.
    /// For this example, we track the size of the congestion window in bytes and
    /// the number of bytes in flight.
    #[derive(Debug, Clone)]
    pub struct MyCongestionController {
        congestion_window: u32,
        bytes_in_flight: u32,
    }

    /// The following is a simple implementation of the `CongestionController` trait
    /// that increases the congestion window by the number of bytes acknowledged and
    /// decreases the congestion window by half when packets are lost.
    #[allow(unused)]
    impl CongestionController for MyCongestionController {
        type PacketInfo = ();

        fn congestion_window(&self) -> u32 {
            self.congestion_window
        }

        fn bytes_in_flight(&self) -> u32 {
            self.bytes_in_flight
        }

        fn is_congestion_limited(&self) -> bool {
            self.congestion_window < self.bytes_in_flight
        }

        fn requires_fast_retransmission(&self) -> bool {
            false
        }

        fn on_packet_sent<Pub: Publisher>(
            &mut self,
            time_sent: Timestamp,
            sent_bytes: usize,
            app_limited: Option<bool>,
            rtt_estimator: &RttEstimator,
            publisher: &mut Pub,
        ) -> Self::PacketInfo {
            self.bytes_in_flight += sent_bytes as u32;
        }

        fn on_rtt_update<Pub: Publisher>(
            &mut self,
            time_sent: Timestamp,
            now: Timestamp,
            rtt_estimator: &RttEstimator,
            publisher: &mut Pub,
        ) {
            // no op
        }

        fn on_ack<Pub: Publisher>(
            &mut self,
            newest_acked_time_sent: Timestamp,
            bytes_acknowledged: usize,
            newest_acked_packet_info: Self::PacketInfo,
            rtt_estimator: &RttEstimator,
            random_generator: &mut dyn RandomGenerator,
            ack_receive_time: Timestamp,
            publisher: &mut Pub,
        ) {
            self.bytes_in_flight -= bytes_acknowledged as u32;
            self.congestion_window += bytes_acknowledged as u32;
        }

        fn on_packet_lost<Pub: Publisher>(
            &mut self,
            lost_bytes: u32,
            packet_info: Self::PacketInfo,
            persistent_congestion: bool,
            new_loss_burst: bool,
            random_generator: &mut dyn RandomGenerator,
            timestamp: Timestamp,
            publisher: &mut Pub,
        ) {
            self.bytes_in_flight -= lost_bytes;
            self.congestion_window = (self.congestion_window as f32 * 0.5) as u32;
        }

        fn on_explicit_congestion<Pub: Publisher>(
            &mut self,
            ce_count: u64,
            event_time: Timestamp,
            publisher: &mut Pub,
        ) {
            self.congestion_window = (self.congestion_window as f32 * 0.5) as u32;
        }

        fn on_mtu_update<Pub: Publisher>(&mut self, max_data_size: u16, publisher: &mut Pub) {
            // no op
        }

        fn on_packet_discarded<Pub: Publisher>(&mut self, bytes_sent: usize, publisher: &mut Pub) {
            self.bytes_in_flight -= bytes_sent as u32;
        }

        fn earliest_departure_time(&self) -> Option<Timestamp> {
            None
        }
    }

    // Define an endpoint for the custom congestion controller so it may be used as a
    // congestion controller provider to the s2n-quic server or client
    #[derive(Debug, Default)]
    pub struct MyCongestionControllerEndpoint {}

    impl congestion_controller::Endpoint for MyCongestionControllerEndpoint {
        type CongestionController = MyCongestionController;

        // This method will be called whenever a new congestion controller instance is needed.
        fn new_congestion_controller(
            &mut self,
            path_info: congestion_controller::PathInfo,
        ) -> Self::CongestionController {
            MyCongestionController {
                congestion_window: path_info.max_datagram_size as u32,
                bytes_in_flight: 0,
            }
        }
    }
}

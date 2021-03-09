// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{inet::SocketAddress, path::MINIMUM_MTU, recovery::RTTEstimator, time::Timestamp};
use core::fmt::Debug;

pub trait Endpoint: 'static {
    type CongestionController: CongestionController;

    fn new_congestion_controller(&mut self, path_info: PathInfo) -> Self::CongestionController;
}

#[derive(Debug)]
#[non_exhaustive]
pub struct PathInfo<'a> {
    pub remote_address: &'a SocketAddress,
    pub alpn: Option<&'a [u8]>,
    pub max_datagram_size: u16,
}

impl<'a> PathInfo<'a> {
    pub fn new(remote_address: &'a SocketAddress) -> Self {
        Self {
            remote_address,
            alpn: None,
            max_datagram_size: MINIMUM_MTU,
        }
    }
}

pub trait CongestionController: 'static + Clone + Send + Debug {
    /// Returns the size of the current congestion window in bytes
    fn congestion_window(&self) -> u32;

    /// Returns `true` if the congestion window does not have sufficient
    /// space for a packet of `max_datagram_size` considering the current
    /// bytes in flight
    fn is_congestion_limited(&self) -> bool;

    /// Returns `true` if the current state of the congestion controller
    /// requires a packet to be transmitted without respecting the
    /// available congestion window
    fn requires_fast_retransmission(&self) -> bool;

    /// Invoked whenever a congestion controlled packet is sent
    fn on_packet_sent(&mut self, time_sent: Timestamp, sent_bytes: usize);

    /// Invoked each time the round trip time is updated, which is whenever the
    /// largest acknowledged packet in an ACK frame is newly acknowledged
    fn on_rtt_update(&mut self, time_sent: Timestamp, rtt_estimator: &RTTEstimator);

    /// Invoked for each newly acknowledged packet
    fn on_packet_ack(
        &mut self,
        largest_acked_time_sent: Timestamp,
        bytes_sent: usize,
        rtt_estimator: &RTTEstimator,
        ack_receive_time: Timestamp,
    );

    /// Invoked when packets are declared lost
    fn on_packets_lost(
        &mut self,
        lost_bytes: u32,
        persistent_congestion: bool,
        timestamp: Timestamp,
    );

    /// Invoked from on_packets_lost, but is also directly invoked when
    /// the Explicit Congestion Notification counter increases.
    fn on_congestion_event(&mut self, event_time: Timestamp);

    /// Invoked when the path maximum transmission unit is updated.
    fn on_mtu_update(&mut self, max_data_size: u16);

    /// Invoked for each packet discarded when a packet number space is discarded.
    fn on_packet_discarded(&mut self, bytes_sent: usize);
}

#[cfg(any(test, feature = "testing"))]
pub mod testing {
    use super::*;
    use crate::recovery::RTTEstimator;

    #[derive(Clone, Copy, Debug, Default, PartialEq)]
    pub struct Unlimited {}

    impl CongestionController for Unlimited {
        fn congestion_window(&self) -> u32 {
            u32::max_value()
        }

        fn is_congestion_limited(&self) -> bool {
            false
        }

        fn requires_fast_retransmission(&self) -> bool {
            false
        }

        fn on_packet_sent(&mut self, _time_sent: Timestamp, _bytes_sent: usize) {}
        fn on_rtt_update(&mut self, _time_sent: Timestamp, _rtt_estimator: &RTTEstimator) {}

        fn on_packet_ack(
            &mut self,
            _largest_acked_time_sent: Timestamp,
            _sent_bytes: usize,
            _rtt_estimator: &RTTEstimator,
            _ack_receive_time: Timestamp,
        ) {
        }

        fn on_packets_lost(
            &mut self,
            _lost_bytes: u32,
            _persistent_congestion: bool,
            _timestamp: Timestamp,
        ) {
        }

        fn on_congestion_event(&mut self, _event_time: Timestamp) {}

        fn on_mtu_update(&mut self, _max_data_size: u16) {}

        fn on_packet_discarded(&mut self, _bytes_sent: usize) {}
    }

    #[derive(Clone, Copy, Debug, Default, PartialEq)]
    pub struct MockCongestionController {
        pub bytes_in_flight: u32,
        pub lost_bytes: u32,
        pub persistent_congestion: Option<bool>,
        pub on_packets_lost: u32,
        pub on_rtt_update: u32,
    }

    impl CongestionController for MockCongestionController {
        fn congestion_window(&self) -> u32 {
            u32::max_value()
        }

        fn is_congestion_limited(&self) -> bool {
            false
        }
        fn requires_fast_retransmission(&self) -> bool {
            false
        }

        fn on_packet_sent(&mut self, _time_sent: Timestamp, bytes_sent: usize) {
            self.bytes_in_flight += bytes_sent as u32
        }
        fn on_rtt_update(&mut self, _time_sent: Timestamp, _rtt_estimator: &RTTEstimator) {
            self.on_rtt_update += 1
        }

        fn on_packet_ack(
            &mut self,
            _largest_acked_time_sent: Timestamp,
            _sent_bytes: usize,
            _rtt_estimator: &RTTEstimator,
            _ack_receive_time: Timestamp,
        ) {
        }

        fn on_packets_lost(
            &mut self,
            lost_bytes: u32,
            persistent_congestion: bool,
            _timestamp: Timestamp,
        ) {
            self.bytes_in_flight = self.bytes_in_flight.saturating_sub(lost_bytes);
            self.lost_bytes += lost_bytes;
            self.persistent_congestion = Some(persistent_congestion);
            self.on_packets_lost += 1;
        }

        fn on_congestion_event(&mut self, _event_time: Timestamp) {}
        fn on_mtu_update(&mut self, _max_data_size: u16) {}
        fn on_packet_discarded(&mut self, bytes_sent: usize) {
            self.bytes_in_flight = self.bytes_in_flight.saturating_sub(bytes_sent as u32);
        }
    }
}

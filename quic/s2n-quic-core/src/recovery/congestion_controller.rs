// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    event::{api::SocketAddress, IntoEvent},
    inet,
    path::MINIMUM_MTU,
    random,
    recovery::RttEstimator,
    time::Timestamp,
};
use core::fmt::Debug;

pub trait Endpoint: 'static + Debug + Send {
    type CongestionController: CongestionController;

    fn new_congestion_controller(&mut self, path_info: PathInfo) -> Self::CongestionController;
}

#[derive(Debug)]
#[non_exhaustive]
pub struct PathInfo<'a> {
    pub remote_address: SocketAddress<'a>,
    pub application_protocol: Option<&'a [u8]>,
    pub max_datagram_size: u16,
}

impl<'a> PathInfo<'a> {
    #[allow(deprecated)]
    pub fn new(remote_address: &'a inet::SocketAddress) -> Self {
        Self {
            remote_address: remote_address.into_event(),
            application_protocol: None,
            max_datagram_size: MINIMUM_MTU,
        }
    }
}

pub trait CongestionController: 'static + Clone + Send + Debug {
    /// Additional metadata about a packet to track until a sent packet
    /// is either acknowledged or declared lost
    type PacketInfo: Copy + Send + Sized + Debug;

    /// Returns the size of the current congestion window in bytes
    fn congestion_window(&self) -> u32;

    /// Returns the current bytes in flight
    fn bytes_in_flight(&self) -> u32;

    /// Returns `true` if the congestion window does not have sufficient
    /// space for a packet of `max_datagram_size` considering the current
    /// bytes in flight
    fn is_congestion_limited(&self) -> bool;

    /// Returns `true` if the current state of the congestion controller
    /// requires a packet to be transmitted without respecting the
    /// available congestion window
    fn requires_fast_retransmission(&self) -> bool;

    /// Invoked when a packet is sent
    ///
    /// The `PacketInfo` returned by this method will be passed to `on_packet_ack` if
    /// the packet is acknowledged and the packet was the newest acknowledged in the ACK frame,
    /// or to `on_packet_lost` if the packet was declared lost.
    ///
    /// Note: Sent bytes may be 0 in the case the packet being sent contains only ACK frames.
    /// These pure ACK packets are not congestion-controlled to ensure congestion control
    /// does not impede congestion feedback.
    fn on_packet_sent(
        &mut self,
        time_sent: Timestamp,
        sent_bytes: usize,
        rtt_estimator: &RttEstimator,
    ) -> Self::PacketInfo;

    /// Invoked each time the round trip time is updated, which is whenever the
    /// newest acknowledged packet in an ACK frame is newly acknowledged
    fn on_rtt_update(&mut self, time_sent: Timestamp, rtt_estimator: &RttEstimator);

    /// Invoked when an acknowledgement of one or more previously unacknowledged packets is received
    ///
    /// Generally the `bytes_acknowledged` value is aggregated over all newly acknowledged packets, though
    /// it is possible this method may be called multiple times for one acknowledgement. In either
    /// case, `newest_acked_time_sent` and `newest_acked_packet_info` represent the newest acknowledged
    /// packet contributing to `bytes_acknowledged`.
    fn on_ack<Rnd: random::Generator>(
        &mut self,
        newest_acked_time_sent: Timestamp,
        bytes_acknowledged: usize,
        newest_acked_packet_info: Self::PacketInfo,
        rtt_estimator: &RttEstimator,
        random_generator: &mut Rnd,
        ack_receive_time: Timestamp,
    );

    /// Invoked when a packet is declared lost
    ///
    /// `new_loss_burst` is true if the lost packet is the first in a
    /// contiguous series of lost packets. This can be used for measuring or
    /// filtering out noise from burst losses.
    fn on_packet_lost<Rnd: random::Generator>(
        &mut self,
        lost_bytes: u32,
        packet_info: Self::PacketInfo,
        persistent_congestion: bool,
        new_loss_burst: bool,
        random_generator: &mut Rnd,
        timestamp: Timestamp,
    );

    /// Invoked from on_packets_lost, but is also directly invoked when
    /// the Explicit Congestion Notification counter increases.
    fn on_congestion_event(&mut self, event_time: Timestamp);

    /// Invoked when the path maximum transmission unit is updated.
    fn on_mtu_update(&mut self, max_data_size: u16);

    /// Invoked for each packet discarded when a packet number space is discarded.
    fn on_packet_discarded(&mut self, bytes_sent: usize);

    /// Returns the earliest time that a packet may be transmitted.
    ///
    /// If the time is in the past or is `None`, the packet should be transmitted immediately.
    fn earliest_departure_time(&self) -> Option<Timestamp>;

    /// The maximum number of bytes for an aggregation of packets scheduled and transmitted together.
    ///
    /// If the value is `None`, the congestion controller does not influence the send aggregation.
    ///
    /// The effect of this value is dependent on platform support for GSO (Generic Segmentation
    /// Offload) as well as the configured `MaxSegments` value.
    fn send_quantum(&self) -> Option<usize> {
        None
    }
}

#[cfg(any(test, feature = "testing"))]
pub mod testing {
    use super::*;
    use crate::recovery::RttEstimator;

    pub mod unlimited {
        use super::*;

        #[derive(Debug, Default)]
        pub struct Endpoint {}

        impl super::Endpoint for Endpoint {
            type CongestionController = CongestionController;

            fn new_congestion_controller(
                &mut self,
                _path_info: super::PathInfo,
            ) -> Self::CongestionController {
                CongestionController::default()
            }
        }

        #[derive(Clone, Copy, Debug, Default, PartialEq)]
        pub struct CongestionController {}

        impl super::CongestionController for CongestionController {
            type PacketInfo = ();

            fn congestion_window(&self) -> u32 {
                u32::max_value()
            }

            fn bytes_in_flight(&self) -> u32 {
                0
            }

            fn is_congestion_limited(&self) -> bool {
                false
            }

            fn requires_fast_retransmission(&self) -> bool {
                false
            }

            fn on_packet_sent(
                &mut self,
                _time_sent: Timestamp,
                _bytes_sent: usize,
                _rtt_estimator: &RttEstimator,
            ) {
            }

            fn on_rtt_update(&mut self, _time_sent: Timestamp, _rtt_estimator: &RttEstimator) {}

            fn on_ack<Rnd: random::Generator>(
                &mut self,
                _newest_acked_time_sent: Timestamp,
                _sent_bytes: usize,
                _newest_acked_packet_info: Self::PacketInfo,
                _rtt_estimator: &RttEstimator,
                _random_generator: &mut Rnd,
                _ack_receive_time: Timestamp,
            ) {
            }

            fn on_packet_lost<Rnd: random::Generator>(
                &mut self,
                _lost_bytes: u32,
                _packet_info: Self::PacketInfo,
                _persistent_congestion: bool,
                _new_loss_burst: bool,
                _random_generator: &mut Rnd,
                _timestamp: Timestamp,
            ) {
            }

            fn on_congestion_event(&mut self, _event_time: Timestamp) {}

            fn on_mtu_update(&mut self, _max_data_size: u16) {}

            fn on_packet_discarded(&mut self, _bytes_sent: usize) {}

            fn earliest_departure_time(&self) -> Option<Timestamp> {
                None
            }
        }
    }

    pub mod mock {
        use super::*;

        #[derive(Debug, Default)]
        pub struct Endpoint {}

        impl super::Endpoint for Endpoint {
            type CongestionController = CongestionController;

            fn new_congestion_controller(
                &mut self,
                _path_info: super::PathInfo,
            ) -> Self::CongestionController {
                CongestionController::default()
            }
        }

        #[derive(Clone, Copy, Debug, PartialEq)]
        pub struct CongestionController {
            pub bytes_in_flight: u32,
            pub lost_bytes: u32,
            pub persistent_congestion: Option<bool>,
            pub on_packets_lost: u32,
            pub on_rtt_update: u32,
            pub on_packet_ack: u32,
            pub on_mtu_update: u32,
            pub congestion_window: u32,
            pub congestion_events: u32,
            pub requires_fast_retransmission: bool,
            pub loss_bursts: u32,
        }

        impl Default for CongestionController {
            fn default() -> Self {
                Self {
                    bytes_in_flight: 0,
                    lost_bytes: 0,
                    persistent_congestion: None,
                    on_packets_lost: 0,
                    on_rtt_update: 0,
                    on_packet_ack: 0,
                    on_mtu_update: 0,
                    congestion_window: 1500 * 10,
                    congestion_events: 0,
                    requires_fast_retransmission: false,
                    loss_bursts: 0,
                }
            }
        }

        impl super::CongestionController for CongestionController {
            type PacketInfo = ();

            fn congestion_window(&self) -> u32 {
                self.congestion_window
            }

            fn bytes_in_flight(&self) -> u32 {
                self.bytes_in_flight
            }

            fn is_congestion_limited(&self) -> bool {
                self.requires_fast_retransmission || self.bytes_in_flight >= self.congestion_window
            }

            fn requires_fast_retransmission(&self) -> bool {
                self.requires_fast_retransmission
            }

            fn on_packet_sent(
                &mut self,
                _time_sent: Timestamp,
                bytes_sent: usize,
                _rtt_estimator: &RttEstimator,
            ) {
                self.bytes_in_flight += bytes_sent as u32;
                self.requires_fast_retransmission = false;
            }

            fn on_rtt_update(&mut self, _time_sent: Timestamp, _rtt_estimator: &RttEstimator) {
                self.on_rtt_update += 1
            }

            fn on_ack<Rnd: random::Generator>(
                &mut self,
                _newest_acked_time_sent: Timestamp,
                _sent_bytes: usize,
                _newest_acked_packet_info: Self::PacketInfo,
                _rtt_estimator: &RttEstimator,
                _random_generator: &mut Rnd,
                _ack_receive_time: Timestamp,
            ) {
                self.on_packet_ack += 1;
            }

            fn on_packet_lost<Rnd: random::Generator>(
                &mut self,
                lost_bytes: u32,
                _packet_info: Self::PacketInfo,
                persistent_congestion: bool,
                new_loss_burst: bool,
                _random_generator: &mut Rnd,
                _timestamp: Timestamp,
            ) {
                self.bytes_in_flight = self.bytes_in_flight.saturating_sub(lost_bytes);
                self.lost_bytes += lost_bytes;
                self.persistent_congestion = Some(persistent_congestion);
                self.on_packets_lost += 1;
                self.requires_fast_retransmission = true;

                if new_loss_burst {
                    self.loss_bursts += 1;
                }
            }

            fn on_congestion_event(&mut self, _event_time: Timestamp) {
                self.congestion_events += 1;
            }

            fn on_mtu_update(&mut self, _max_data_size: u16) {
                self.on_mtu_update += 1;
            }

            fn on_packet_discarded(&mut self, bytes_sent: usize) {
                self.bytes_in_flight = self.bytes_in_flight.saturating_sub(bytes_sent as u32);
            }

            fn earliest_departure_time(&self) -> Option<Timestamp> {
                None
            }
        }
    }
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    event,
    event::{
        api::SocketAddress,
        builder::{BbrState, SlowStartExitCause},
        IntoEvent,
    },
    inet, path,
    path::InitialMtu,
    random,
    recovery::{
        bandwidth::{Bandwidth, RateSample},
        RttEstimator,
    },
    time::Timestamp,
};
use core::fmt::Debug;
use num_rational::Ratio;
use num_traits::ToPrimitive;

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
    pub fn new(initial_mtu: InitialMtu, remote_address: &'a inet::SocketAddress) -> Self {
        Self {
            remote_address: remote_address.into_event(),
            application_protocol: None,
            max_datagram_size: initial_mtu.max_datagram_size(remote_address),
        }
    }
}

pub trait Publisher {
    /// Invoked when the congestion controller has exited the Slow Start phase
    fn on_slow_start_exited(&mut self, cause: SlowStartExitCause, congestion_window: u32);
    /// Invoked when the delivery rate sample has been updated
    fn on_delivery_rate_sampled(&mut self, rate_sample: RateSample);
    /// Invoked when the pacing rate has been updated
    fn on_pacing_rate_updated(
        &mut self,
        pacing_rate: Bandwidth,
        burst_size: u32,
        pacing_gain: Ratio<u64>,
    );
    /// Invoked when the state of the BBR congestion controller changes
    fn on_bbr_state_changed(&mut self, state: BbrState);
}

/// Wrapper around a `ConnectionPublisher` that forwards congestion control related
/// events to the inner publisher with the necessary context.
pub struct PathPublisher<'a, Pub: event::ConnectionPublisher> {
    publisher: &'a mut Pub,
    path_id: path::Id,
}

impl<'a, Pub: event::ConnectionPublisher> PathPublisher<'a, Pub> {
    /// Constructs a new `Publisher` around the given `event::ConnectionPublisher` and `path_id`
    pub fn new(publisher: &'a mut Pub, path_id: path::Id) -> PathPublisher<Pub> {
        Self { publisher, path_id }
    }
}

impl<'a, Pub: event::ConnectionPublisher> Publisher for PathPublisher<'a, Pub> {
    #[inline]
    fn on_slow_start_exited(&mut self, cause: SlowStartExitCause, congestion_window: u32) {
        self.publisher
            .on_slow_start_exited(event::builder::SlowStartExited {
                path_id: self.path_id.into_event(),
                cause,
                congestion_window,
            });
    }

    #[inline]
    fn on_delivery_rate_sampled(&mut self, rate_sample: RateSample) {
        self.publisher
            .on_delivery_rate_sampled(event::builder::DeliveryRateSampled {
                path_id: self.path_id.into_event(),
                rate_sample: rate_sample.into_event(),
            })
    }

    #[inline]
    fn on_pacing_rate_updated(
        &mut self,
        pacing_rate: Bandwidth,
        burst_size: u32,
        pacing_gain: Ratio<u64>,
    ) {
        self.publisher
            .on_pacing_rate_updated(event::builder::PacingRateUpdated {
                path_id: self.path_id.into_event(),
                bytes_per_second: pacing_rate.as_bytes_per_second(),
                burst_size,
                pacing_gain: pacing_gain
                    .to_f32()
                    .expect("pacing gain should be representable as f32"),
            })
    }

    #[inline]
    fn on_bbr_state_changed(&mut self, state: BbrState) {
        self.publisher
            .on_bbr_state_changed(event::builder::BbrStateChanged {
                path_id: self.path_id.into_event(),
                state,
            })
    }
}

/// An algorithm for controlling congestion.
///
/// NOTE: This trait is considered unstable and can only be implemented by
///       including the `unstable-congestion-controller` feature.
pub trait CongestionController: 'static + Clone + Send + Debug + private::Sealed {
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
    /// `app_limited` indicates whether the application has enough data to send to fill the
    /// congestion window. This value will be `None` for Initial and Handshake packets.
    ///
    /// Note: Sent bytes may be 0 in the case the packet being sent contains only ACK frames.
    /// These pure ACK packets are not congestion-controlled to ensure congestion control
    /// does not impede congestion feedback.
    fn on_packet_sent<Pub: Publisher>(
        &mut self,
        time_sent: Timestamp,
        sent_bytes: usize,
        app_limited: Option<bool>,
        rtt_estimator: &RttEstimator,
        publisher: &mut Pub,
    ) -> Self::PacketInfo;

    /// Invoked each time the round trip time is updated, which is whenever the
    /// newest acknowledged packet in an ACK frame is newly acknowledged
    fn on_rtt_update<Pub: Publisher>(
        &mut self,
        time_sent: Timestamp,
        now: Timestamp,
        rtt_estimator: &RttEstimator,
        publisher: &mut Pub,
    );

    /// Invoked when an acknowledgement of one or more previously unacknowledged packets is received
    ///
    /// Generally the `bytes_acknowledged` value is aggregated over all newly acknowledged packets, though
    /// it is possible this method may be called multiple times for one acknowledgement. In either
    /// case, `newest_acked_time_sent` and `newest_acked_packet_info` represent the newest acknowledged
    /// packet contributing to `bytes_acknowledged`.
    #[allow(clippy::too_many_arguments)]
    fn on_ack<Pub: Publisher>(
        &mut self,
        newest_acked_time_sent: Timestamp,
        bytes_acknowledged: usize,
        newest_acked_packet_info: Self::PacketInfo,
        rtt_estimator: &RttEstimator,
        random_generator: &mut dyn random::Generator,
        ack_receive_time: Timestamp,
        publisher: &mut Pub,
    );

    /// Invoked when a packet is declared lost
    ///
    /// `new_loss_burst` is true if the lost packet is the first in a
    /// contiguous series of lost packets. This can be used for measuring or
    /// filtering out noise from burst losses.
    #[allow(clippy::too_many_arguments)]
    fn on_packet_lost<Pub: Publisher>(
        &mut self,
        lost_bytes: u32,
        packet_info: Self::PacketInfo,
        persistent_congestion: bool,
        new_loss_burst: bool,
        random_generator: &mut dyn random::Generator,
        timestamp: Timestamp,
        publisher: &mut Pub,
    );

    /// Invoked when the Explicit Congestion Notification counter increases.
    ///
    /// `ce_count` represents the incremental number of packets marked with the ECN CE codepoint
    fn on_explicit_congestion<Pub: Publisher>(
        &mut self,
        ce_count: u64,
        event_time: Timestamp,
        publisher: &mut Pub,
    );

    /// Invoked when the path maximum transmission unit is updated.
    fn on_mtu_update<Pub: Publisher>(&mut self, max_data_size: u16, publisher: &mut Pub);

    /// Invoked for each packet discarded when a packet number space is discarded.
    fn on_packet_discarded<Pub: Publisher>(&mut self, bytes_sent: usize, publisher: &mut Pub);

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

// Prevent implementation of the `CongestionController` trait if the
// `unstable-congestion-controller` feature is not turned on.
mod private {
    use cfg_if::cfg_if;

    pub trait Sealed {}

    cfg_if!(
        if #[cfg(any(test, feature = "unstable-congestion-controller", feature = "testing"))] {
            // If `unstable-congestion-controller` is enabled, implement Sealed for any type that
            // otherwise implements `CongestionController`
            impl<T: crate::recovery::CongestionController> Sealed for T {}
        } else {
            // Otherwise only allow the included CUBIC and BBRv2 congestion controllers
            impl Sealed for crate::recovery::CubicCongestionController {}
            impl Sealed for crate::recovery::bbr::BbrCongestionController {}
        }
    );
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

        /// Returning this instead of a `()` ensures the information gets passed back in testing
        #[derive(Clone, Copy, Debug, Default)]
        pub struct PacketInfo(());

        impl super::CongestionController for CongestionController {
            type PacketInfo = PacketInfo;

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

            fn on_packet_sent<Pub: Publisher>(
                &mut self,
                _time_sent: Timestamp,
                _bytes_sent: usize,
                _app_limited: Option<bool>,
                _rtt_estimator: &RttEstimator,
                _publisher: &mut Pub,
            ) -> PacketInfo {
                PacketInfo(())
            }

            fn on_rtt_update<Pub: Publisher>(
                &mut self,
                _time_sent: Timestamp,
                _now: Timestamp,
                _rtt_estimator: &RttEstimator,
                _publisher: &mut Pub,
            ) {
            }

            fn on_ack<Pub: Publisher>(
                &mut self,
                _newest_acked_time_sent: Timestamp,
                _sent_bytes: usize,
                _newest_acked_packet_info: Self::PacketInfo,
                _rtt_estimator: &RttEstimator,
                _random_generator: &mut dyn random::Generator,
                _ack_receive_time: Timestamp,
                _publisher: &mut Pub,
            ) {
            }

            fn on_packet_lost<Pub: Publisher>(
                &mut self,
                _lost_bytes: u32,
                _packet_info: Self::PacketInfo,
                _persistent_congestion: bool,
                _new_loss_burst: bool,
                _random_generator: &mut dyn random::Generator,
                _timestamp: Timestamp,
                _publisher: &mut Pub,
            ) {
            }

            fn on_explicit_congestion<Pub: Publisher>(
                &mut self,
                _ce_count: u64,
                _event_time: Timestamp,
                _publisher: &mut Pub,
            ) {
            }

            fn on_mtu_update<Pub: Publisher>(&mut self, _max_data_size: u16, _publisher: &mut Pub) {
            }

            fn on_packet_discarded<Pub: Publisher>(
                &mut self,
                _bytes_sent: usize,
                _publisher: &mut Pub,
            ) {
            }

            fn earliest_departure_time(&self) -> Option<Timestamp> {
                None
            }
        }
    }

    pub mod mock {
        use super::*;
        use crate::path::RemoteAddress;

        #[derive(Debug, Default)]
        pub struct Endpoint {}

        impl super::Endpoint for Endpoint {
            type CongestionController = CongestionController;

            fn new_congestion_controller(
                &mut self,
                path_info: super::PathInfo,
            ) -> Self::CongestionController {
                CongestionController::new(path_info.remote_address.into())
            }
        }

        #[derive(Clone, Copy, Debug, Default)]
        pub struct PacketInfo {
            remote_address: RemoteAddress,
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
            pub app_limited: Option<bool>,
            pub slow_start: bool,
            pub remote_address: RemoteAddress,
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
                    app_limited: None,
                    slow_start: true,
                    remote_address: RemoteAddress::default(),
                }
            }
        }

        impl CongestionController {
            pub fn new(remote_address: RemoteAddress) -> Self {
                Self {
                    remote_address,
                    ..Default::default()
                }
            }
        }

        impl super::CongestionController for CongestionController {
            type PacketInfo = PacketInfo;

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

            fn on_packet_sent<Pub: Publisher>(
                &mut self,
                _time_sent: Timestamp,
                bytes_sent: usize,
                app_limited: Option<bool>,
                _rtt_estimator: &RttEstimator,
                _publisher: &mut Pub,
            ) -> PacketInfo {
                self.bytes_in_flight += bytes_sent as u32;
                self.requires_fast_retransmission = false;
                self.app_limited = app_limited;
                PacketInfo {
                    remote_address: self.remote_address,
                }
            }

            fn on_rtt_update<Pub: Publisher>(
                &mut self,
                _time_sent: Timestamp,
                _now: Timestamp,
                _rtt_estimator: &RttEstimator,
                _publisher: &mut Pub,
            ) {
                self.on_rtt_update += 1
            }

            fn on_ack<Pub: Publisher>(
                &mut self,
                _newest_acked_time_sent: Timestamp,
                _sent_bytes: usize,
                newest_acked_packet_info: Self::PacketInfo,
                _rtt_estimator: &RttEstimator,
                _random_generator: &mut dyn random::Generator,
                _ack_receive_time: Timestamp,
                _publisher: &mut Pub,
            ) {
                assert_eq!(self.remote_address, newest_acked_packet_info.remote_address);

                self.on_packet_ack += 1;
            }

            fn on_packet_lost<Pub: Publisher>(
                &mut self,
                lost_bytes: u32,
                packet_info: Self::PacketInfo,
                persistent_congestion: bool,
                new_loss_burst: bool,
                _random_generator: &mut dyn random::Generator,
                _timestamp: Timestamp,
                _publisher: &mut Pub,
            ) {
                assert_eq!(self.remote_address, packet_info.remote_address);

                self.bytes_in_flight = self.bytes_in_flight.saturating_sub(lost_bytes);
                self.lost_bytes += lost_bytes;
                self.persistent_congestion = Some(persistent_congestion);
                self.on_packets_lost += 1;
                self.requires_fast_retransmission = true;

                if new_loss_burst {
                    self.loss_bursts += 1;
                }
            }

            fn on_explicit_congestion<Pub: Publisher>(
                &mut self,
                _ce_count: u64,
                _event_time: Timestamp,
                _publisher: &mut Pub,
            ) {
                self.congestion_events += 1;
                self.slow_start = false;
            }

            fn on_mtu_update<Pub: Publisher>(&mut self, _max_data_size: u16, _publisher: &mut Pub) {
                self.on_mtu_update += 1;
            }

            fn on_packet_discarded<Pub: Publisher>(
                &mut self,
                bytes_sent: usize,
                _publisher: &mut Pub,
            ) {
                self.bytes_in_flight = self.bytes_in_flight.saturating_sub(bytes_sent as u32);
            }

            fn earliest_departure_time(&self) -> Option<Timestamp> {
                None
            }
        }
    }
}

#[cfg(test)]
mod fuzz_target;

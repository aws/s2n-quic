use crate::{
    inet::SocketAddress,
    path::MINIMUM_MTU,
    recovery::{loss_info::LossInfo, RTTEstimator},
    time::Timestamp,
};
use core::time::Duration;

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

pub trait CongestionController: 'static + Clone + Send {
    /// Gets the current congestion window size in bytes
    fn congestion_window(&self) -> u32;

    /// Gets the numbers of bytes remaining in the congestion window
    /// considering the current bytes in flight
    fn available_congestion_window(&self) -> u32;

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
        loss_info: LossInfo,
        persistent_congestion_threshold: Duration,
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
        fn available_congestion_window(&self) -> u32 {
            u32::max_value()
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
            _loss_info: LossInfo,
            _persistent_congestion_threshold: Duration,
            _timestamp: Timestamp,
        ) {
        }

        fn on_congestion_event(&mut self, _event_time: Timestamp) {}

        fn on_mtu_update(&mut self, _max_data_size: u16) {}

        fn on_packet_discarded(&mut self, _bytes_sent: usize) {}
    }
}

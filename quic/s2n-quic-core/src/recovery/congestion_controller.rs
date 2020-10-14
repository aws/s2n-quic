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
    fn congestion_window(&self) -> usize;

    fn on_packet_sent(&mut self, time_sent: Timestamp, sent_bytes: usize);

    fn on_rtt_update(&mut self, time_sent: Timestamp, rtt_estimator: &RTTEstimator);

    fn on_packet_ack(
        &mut self,
        time_sent: Timestamp,
        sent_bytes: usize,
        is_limited: bool,
        rtt_estimator: &RTTEstimator,
        ack_receive_time: Timestamp,
    );

    fn on_packets_lost(
        &mut self,
        loss_info: LossInfo,
        persistent_congestion_duration: Duration,
        timestamp: Timestamp,
    );

    fn on_congestion_event(&mut self, event_time: Timestamp);

    fn on_mtu_update(&mut self, max_data_size: usize);
}

#[cfg(any(test, feature = "testing"))]
pub mod testing {
    use super::*;
    use crate::recovery::RTTEstimator;

    #[derive(Clone, Copy, Debug, Default, PartialEq)]
    pub struct MockCC {
        // TODO add fields
        _todo: (),
    }

    impl CongestionController for MockCC {
        // TODO implement callbacks
        fn congestion_window(&self) -> usize {
            usize::max_value()
        }
        fn on_packet_sent(&mut self, _time_sent: Timestamp, _sent_bytes: usize) {}
        fn on_rtt_update(&mut self, _time_sent: Timestamp, _rtt_estimator: &RTTEstimator) {}

        fn on_packet_ack(
            &mut self,
            _time_sent: Timestamp,
            _sent_bytes: usize,
            _is_limited: bool,
            _rtt_estimator: &RTTEstimator,
            _ack_receive_time: Timestamp,
        ) {
        }

        fn on_packets_lost(
            &mut self,
            _loss_info: LossInfo,
            _persistent_congestion_duration: Duration,
            _timestamp: Timestamp,
        ) {
        }

        fn on_congestion_event(&mut self, _event_time: Timestamp) {}

        fn on_mtu_update(&mut self, _max_data_size: usize) {}
    }
}

use crate::{inet::SocketAddress, time::Timestamp};

pub trait Endpoint: 'static {
    type CongestionController: CongestionController;

    fn new_congestion_controller(&mut self, path_info: PathInfo) -> Self::CongestionController;
}

#[derive(Debug)]
#[non_exhaustive]
pub struct PathInfo<'a> {
    pub remote_address: &'a SocketAddress,
    pub alpn: Option<&'a [u8]>,
}

impl<'a> PathInfo<'a> {
    pub fn new(remote_address: &'a SocketAddress) -> Self {
        Self {
            remote_address,
            alpn: None,
        }
    }
}

pub trait CongestionController: 'static + Clone + Send {
    fn on_packet_acked(&self, time_sent: Timestamp, sent_bytes: usize);

    fn on_packets_lost(&self, loss_info: LossInfo);

    fn process_ecn(&self, time_sent: Timestamp);
}

#[cfg(any(test, feature = "testing"))]
pub mod testing {
    use super::*;

    #[derive(Clone, Copy, Debug, Default, PartialEq)]
    pub struct MockCC {
        // TODO add fields
        _todo: (),
    }

    impl CongestionController for MockCC {
        fn on_packet_acked(&self, time_sent: Timestamp, sent_bytes: usize) {
            unimplemented!()
        }

        fn on_packets_lost(&self, loss_info: _) {
            unimplemented!()
        }

        fn process_ecn(&self, time_sent: Timestamp) {
            unimplemented!()
        }
        // TODO implement callbacks
    }
}

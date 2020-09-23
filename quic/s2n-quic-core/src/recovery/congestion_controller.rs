use crate::inet::SocketAddress;

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
    // TODO implement callbacks
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
        // TODO implement callbacks
    }
}

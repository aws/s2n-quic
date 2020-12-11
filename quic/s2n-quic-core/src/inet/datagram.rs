use crate::{
    connection,
    inet::{ExplicitCongestionNotification, SocketAddress},
    time::Timestamp,
};

/// Metadata for a datagram sent/received over the network
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DatagramInfo {
    pub timestamp: Timestamp,
    pub remote_address: SocketAddress,
    pub payload_len: usize,
    pub ecn: ExplicitCongestionNotification,
    pub destination_connection_id: connection::LocalId,
}

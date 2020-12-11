//! This module contains the Manager implementation

use crate::space::EARLY_ACK_SETTINGS;
use s2n_quic_core::{
    connection,
    inet::{DatagramInfo, SocketAddress},
    recovery::{CongestionController, RTTEstimator},
    transport::error::TransportError,
};
use smallvec::SmallVec;

/// re-export core
pub use s2n_quic_core::path::*;

/// The amount of Paths that can be maintained without using the heap
const INLINE_PATH_LEN: usize = 5;

/// The PathManager handles paths for a specific connection.
/// It will handle path validation operations, and track the active path for a connection.
#[derive(Debug)]
pub struct Manager<CC: CongestionController> {
    /// Path array
    paths: SmallVec<[Path<CC>; INLINE_PATH_LEN]>,

    /// Index to the active path
    active: usize,
}

impl<CC: CongestionController> Manager<CC> {
    pub fn new(initial_path: Path<CC>) -> Self {
        Manager {
            paths: SmallVec::from_elem(initial_path, 1),
            active: 0,
        }
    }

    /// Return the active path
    pub fn active_path(&self) -> (Id, &Path<CC>) {
        let id = Id(self.active);
        let path = &self.paths[self.active];
        (id, path)
    }

    /// Return a mutable reference to the active path
    pub fn active_path_mut(&mut self) -> (Id, &mut Path<CC>) {
        let id = Id(self.active);
        let path = &mut self.paths[self.active];
        (id, path)
    }

    /// Returns the Path for the provided address if the PathManager knows about it
    pub fn path(&self, peer_address: &SocketAddress) -> Option<(Id, &Path<CC>)> {
        self.paths
            .iter()
            .enumerate()
            .find(|(_id, path)| *peer_address == path.peer_socket_address)
            .map(|(id, path)| (Id(id), path))
    }

    /// Returns the Path for the provided address if the PathManager knows about it
    pub fn path_mut(&mut self, peer_address: &SocketAddress) -> Option<(Id, &mut Path<CC>)> {
        self.paths
            .iter_mut()
            .enumerate()
            .find(|(_id, path)| *peer_address == path.peer_socket_address)
            .map(|(id, path)| (Id(id), path))
    }

    /// Called when a datagram is received on a connection
    /// Upon success, returns a `(Id, bool)` containing the path ID and a boolean that is
    /// true if the path had been amplification limited prior to receiving the datagram
    /// and is now no longer amplification limited.
    pub fn on_datagram_received<NewCC: FnOnce() -> CC>(
        &mut self,
        datagram: &DatagramInfo,
        is_handshake_confirmed: bool,
        new_congestion_controller: NewCC,
    ) -> Result<(Id, bool), TransportError> {
        if let Some((id, path)) = self.path_mut(&datagram.remote_address) {
            let unblocked = path.on_bytes_received(datagram.payload_len);
            return Ok((id, unblocked));
        }

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#9
        //# The design of QUIC relies on endpoints retaining a stable address
        //# for the duration of the handshake.  An endpoint MUST NOT initiate
        //# connection migration before the handshake is confirmed, as defined
        //# in section 4.1.2 of [QUIC-TLS].
        if is_handshake_confirmed {
            let path = Path::new(
                datagram.remote_address,
                self.active_path().1.peer_connection_id,
                RTTEstimator::new(EARLY_ACK_SETTINGS.max_ack_delay),
                new_congestion_controller(),
                true,
            );
            let id = Id(self.paths.len());
            self.paths.push(path);
            return Ok((id, false));
        }

        Err(TransportError::PROTOCOL_VIOLATION)
    }

    //TODO= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#8.4
    //# On receiving a PATH_CHALLENGE frame, an endpoint MUST respond by
    //# echoing the data contained in the PATH_CHALLENGE frame in a
    //# PATH_RESPONSE frame.
    pub fn on_path_challenge(
        &mut self,
        _peer_address: &SocketAddress,
        _challenge: s2n_quic_core::frame::PathChallenge,
    ) {
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#8.2.3
    //# Path validation succeeds when a PATH_RESPONSE frame is received that
    //# contains the data that was sent in a previous PATH_CHALLENGE frame.
    //# A PATH_RESPONSE frame received on any network path validates the path
    //# on which the PATH_CHALLENGE was sent.
    pub fn on_path_response(
        &mut self,
        peer_address: &SocketAddress,
        response: s2n_quic_core::frame::PathResponse,
    ) {
        if let Some((_id, path)) = self.path_mut(peer_address) {
            // We may have received a duplicate packet, only call the on_validated handler
            // one time.
            if path.is_validated() {
                return;
            }

            if let Some(expected_response) = path.challenge {
                if &expected_response == response.data {
                    path.on_validated();
                }
            }
        }
    }

    /// Called when a token is received that was issued in a Retry packet
    pub fn on_retry_token(&self, _peer_address: &SocketAddress, _token: &[u8]) {}

    /// Called when a token is received that was issued in a NEW_TOKEN frame
    pub fn on_new_token(&self, _peer_address: &SocketAddress, _token: &[u8]) {}

    /// Start the validation process for a path
    pub fn validate_path(&self, _path: Path<CC>) {}

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#10.3
    //# Tokens are
    //# invalidated when their associated connection ID is retired via a
    //# RETIRE_CONNECTION_ID frame (Section 19.16).
    pub fn on_connection_id_retire(&self, _connection_id: &connection::LocalId) {
        // TODO invalidate any tokens issued under this connection id
    }

    pub fn on_connection_id_new(&self, _connection_id: &connection::LocalId) {}

    pub fn on_packet_received(&mut self) {}
}

/// Internal Id of a path in the manager
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Eq, Ord, Hash)]
pub struct Id(usize);

impl<CC: CongestionController> core::ops::Index<Id> for Manager<CC> {
    type Output = Path<CC>;

    fn index(&self, id: Id) -> &Self::Output {
        &self.paths[id.0]
    }
}

impl<CC: CongestionController> core::ops::IndexMut<Id> for Manager<CC> {
    fn index_mut(&mut self, id: Id) -> &mut Self::Output {
        &mut self.paths[id.0]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::time::Duration;
    use s2n_quic_core::{
        inet::{DatagramInfo, ExplicitCongestionNotification},
        recovery::{congestion_controller::testing::Unlimited, RTTEstimator},
        time::Timestamp,
    };
    use std::net::SocketAddr;

    #[test]
    fn get_path_by_address_test() {
        let conn_id = connection::PeerId::try_from_bytes(&[0, 1, 2, 3, 4, 5]).unwrap();
        let first_path = Path::new(
            SocketAddress::default(),
            conn_id,
            RTTEstimator::new(Duration::from_millis(30)),
            Unlimited::default(),
            false,
        );

        let manager = Manager::new(first_path);
        assert_eq!(manager.paths.len(), 1);

        let (_id, matched_path) = manager.path(&SocketAddress::default()).unwrap();
        assert_eq!(matched_path, &first_path);
    }

    #[test]
    fn path_validate_test() {
        let first_conn_id = connection::PeerId::try_from_bytes(&[0, 1, 2, 3, 4, 5]).unwrap();
        let mut first_path = Path::new(
            SocketAddress::default(),
            first_conn_id,
            RTTEstimator::new(Duration::from_millis(30)),
            Unlimited::default(),
            false,
        );
        first_path.challenge = Some([0u8; 8]);

        let mut manager = Manager::new(first_path);
        assert_eq!(manager.paths.len(), 1);
        {
            let (_id, first_path) = manager.path(&first_path.peer_socket_address).unwrap();
            assert_eq!(first_path.is_validated(), false);
        }

        let frame = s2n_quic_core::frame::PathResponse { data: &[0u8; 8] };
        manager.on_path_response(&first_path.peer_socket_address, frame);
        {
            let (_id, first_path) = manager.path(&first_path.peer_socket_address).unwrap();
            assert_eq!(first_path.is_validated(), true);
        }
    }

    #[test]
    fn new_peer_test() {
        let first_conn_id = connection::PeerId::try_from_bytes(&[0, 1, 2, 3, 4, 5]).unwrap();
        let first_path = Path::new(
            SocketAddress::default(),
            first_conn_id,
            RTTEstimator::new(Duration::from_millis(30)),
            Unlimited::default(),
            false,
        );
        let mut manager = Manager::new(first_path);
        assert_eq!(manager.paths.len(), 1);
        let new_addr: SocketAddr = "127.0.0.1:80".parse().unwrap();
        let new_addr = SocketAddress::from(new_addr);

        assert_eq!(manager.path(&SocketAddress::default()).is_some(), true);
        assert_eq!(manager.path(&new_addr), None);
        assert_eq!(manager.paths.len(), 1);

        let datagram = DatagramInfo {
            timestamp: unsafe { Timestamp::from_duration(Duration::from_millis(30)) },
            remote_address: new_addr,
            payload_len: 0,
            ecn: ExplicitCongestionNotification::default(),
            destination_connection_id: connection::LocalId::EMPTY,
        };

        manager
            .on_datagram_received(&datagram, true, Unlimited::default)
            .unwrap();
        assert_eq!(manager.path(&new_addr).is_some(), true);
        assert_eq!(manager.paths.len(), 2);

        let new_addr: SocketAddr = "127.0.0.1:443".parse().unwrap();
        let new_addr = SocketAddress::from(new_addr);
        let datagram = DatagramInfo {
            timestamp: unsafe { Timestamp::from_duration(Duration::from_millis(30)) },
            remote_address: new_addr,
            payload_len: 0,
            ecn: ExplicitCongestionNotification::default(),
            destination_connection_id: connection::LocalId::EMPTY,
        };

        assert_eq!(
            manager
                .on_datagram_received(&datagram, false, Unlimited::default)
                .is_err(),
            true
        );
        assert_eq!(manager.paths.len(), 2);
    }
}

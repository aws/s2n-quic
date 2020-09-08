//! This module contains the Manager implementation

use s2n_quic_core::{connection, inet::SocketAddress, path::Path};
use smallvec::SmallVec;

/// The amount of Paths that can be maintained without using the heap
const INLINE_PATH_LEN: usize = 5;

/// The PathManager handles paths for a specific connection.
/// It will handle path validation operations, and track the active path for a connection.
#[derive(Default)]
pub struct Manager {
    /// Path array
    paths: SmallVec<[Path; INLINE_PATH_LEN]>,

    /// Index to the active path
    active: usize,
}

impl Manager {
    /// Return the active path
    pub fn active_path(&self) -> &Path {
        &self.paths[self.active]
    }

    /// Return a mutable reference to the active path
    pub fn active_path_mut(&mut self) -> &mut Path {
        &mut self.paths[self.active]
    }

    /// Returns whether the socket address belongs to the current peer or an in progress peer
    pub fn is_new_path(&self, peer_address: &SocketAddress) -> bool {
        self.path(peer_address).is_none()
    }

    /// Returns the Path for the connection id if the PathManager knows about it
    pub fn path(&self, peer_address: &SocketAddress) -> Option<&Path> {
        self.paths
            .iter()
            .find(|path| *peer_address == path.peer_socket_address)
    }

    /// Returns the Path for the connection id if the PathManager knows about it
    pub fn path_mut(&mut self, peer_address: &SocketAddress) -> Option<&mut Path> {
        self.paths
            .iter_mut()
            .find(|path| *peer_address == path.peer_socket_address)
    }

    /// Add a new path to the PathManager
    pub fn insert(&mut self, path: Path) {
        self.paths.push(path);
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#8.4
    //# On receiving a PATH_CHALLENGE frame, an endpoint MUST respond
    //# immediately by echoing the data contained in the PATH_CHALLENGE frame
    //# in a PATH_RESPONSE frame.
    pub fn on_path_challenge(
        &mut self,
        _peer_address: &SocketAddress,
        _challenge: s2n_quic_core::frame::PathChallenge,
    ) {
        // TODO  this may be a no-op here. Perhaps the frame handler does the work.
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#8.5
    //# A new address is considered valid when a PATH_RESPONSE frame is
    //# received that contains the data that was sent in a previous
    //# PATH_CHALLENGE.
    pub fn on_path_response(
        &mut self,
        peer_address: &SocketAddress,
        response: s2n_quic_core::frame::PathResponse,
    ) {
        if let Some(path) = self.path_mut(peer_address) {
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
    pub fn validate_path(&self, _path: Path) {}

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-29#10.4
    //# Tokens are invalidated when their associated connection ID is retired via a
    //# RETIRE_CONNECTION_ID frame (Section 19.16).
    pub fn on_connection_id_retire(&self, _connenction_id: &connection::Id) {
        // TODO invalidate any tokens issued under this connection id
    }

    pub fn on_connection_id_new(&self, _connection_id: &connection::Id) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::time::Duration;
    use s2n_quic_core::recovery::RTTEstimator;
    use std::net::SocketAddr;

    #[test]
    fn get_path_by_address_test() {
        let conn_id = connection::Id::try_from_bytes(&[0, 1, 2, 3, 4, 5]).unwrap();
        let inserted_path = Path::new(
            conn_id,
            SocketAddress::default(),
            conn_id,
            RTTEstimator::new(Duration::from_millis(30)),
        );

        let mut manager = Manager::default();
        manager.insert(inserted_path);

        let matched_path = manager.path(&SocketAddress::default()).unwrap();
        assert_eq!(matched_path, &inserted_path);
    }

    #[test]
    fn path_validate_test() {
        let first_conn_id = connection::Id::try_from_bytes(&[0, 1, 2, 3, 4, 5]).unwrap();
        let mut first_path = Path::new(
            first_conn_id,
            SocketAddress::default(),
            first_conn_id,
            RTTEstimator::new(Duration::from_millis(30)),
        );
        first_path.challenge = Some([0u8; 8]);

        let mut manager = Manager::default();
        manager.insert(first_path);
        {
            let first_path = manager.path(&first_path.peer_socket_address).unwrap();
            assert_eq!(first_path.is_validated(), false);
        }

        let frame = s2n_quic_core::frame::PathResponse { data: &[0u8; 8] };
        manager.on_path_response(&first_path.peer_socket_address, frame);
        {
            let first_path = manager.path(&first_path.peer_socket_address).unwrap();
            assert_eq!(first_path.is_validated(), true);
        }
    }

    #[test]
    fn new_peer_test() {
        let first_conn_id = connection::Id::try_from_bytes(&[0, 1, 2, 3, 4, 5]).unwrap();
        let first_path = Path::new(
            first_conn_id,
            SocketAddress::default(),
            first_conn_id,
            RTTEstimator::new(Duration::from_millis(30)),
        );

        let mut manager = Manager::default();
        manager.insert(first_path);
        assert_eq!(manager.is_new_path(&SocketAddress::default()), false);

        let addr: SocketAddr = "127.0.0.1:80".parse().unwrap();
        assert_eq!(manager.is_new_path(&addr.into()), true);
    }
}

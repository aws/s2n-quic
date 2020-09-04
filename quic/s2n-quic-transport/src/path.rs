//! This module contains the Manager implementation

use s2n_quic_core::{connection, inet::SocketAddress, path::Path};
use smallvec::SmallVec;

/// The amount of Paths that can be maintained without using the heap
const STATIC_DEFAULT_PATHS: usize = 5;

/// The PathManager handles paths for a specific connection.
/// It will handle path validation operations, and track the active path for a connection.
#[derive(Default)]
pub struct Manager {
    /// Path array
    paths: SmallVec<[Path; STATIC_DEFAULT_PATHS]>,

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
    pub fn is_new_path(
        &self,
        peer_address: &SocketAddress,
        destination_connection_id: &connection::Id,
    ) -> bool {
        self.path(peer_address, destination_connection_id).is_none()
    }

    /// Returns the Path for the connection id if the PathManager knows about it
    pub fn path(
        &self,
        peer_address: &SocketAddress,
        destination_connection_id: &connection::Id,
    ) -> Option<&Path> {
        if let Some(path_index) = self
            .paths
            .iter()
            .position(|path| self.matching_path(&path, peer_address, destination_connection_id))
        {
            return Some(&self.paths[path_index]);
        }

        None
    }

    /// Returns the Path for the connection id if the PathManager knows about it
    pub fn path_mut(
        &mut self,
        peer_address: &SocketAddress,
        destination_connection_id: &connection::Id,
    ) -> Option<&mut Path> {
        if let Some(path_index) = self
            .paths
            .iter()
            .position(|path| self.matching_path(&path, peer_address, destination_connection_id))
        {
            return Some(&mut self.paths[path_index]);
        }

        None
    }

    /// Add a new path to the PathManager
    pub fn insert(&mut self, path: Path) {
        self.paths.push(path);
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-29#8.4
    //# On receiving a PATH_CHALLENGE frame, an endpoint MUST respond
    //# immediately by echoing the data contained in the PATH_CHALLENGE frame
    //# in a PATH_RESPONSE frame.
    pub fn on_path_challenge(
        &mut self,
        _peer_address: &SocketAddress,
        _destination_connection_id: &connection::Id,
        _challenge: &[u8],
    ) {
        // TODO  this may be a no-op here. Perhaps the frame handler does the work.
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-29#8.5
    //# A new address is considered valid when a PATH_RESPONSE frame is
    //# received that contains the data that was sent in a previous
    //# PATH_CHALLENGE.
    pub fn on_path_response(
        &mut self,
        peer_address: &SocketAddress,
        destination_connection_id: &connection::Id,
        response: &[u8],
    ) {
        if let Some(path) = self.path_mut(peer_address, destination_connection_id) {
            // We may have received a duplicate packet, only call the on_validated handler
            // one time.
            if path.is_validated() {
                return;
            }

            if let Some(expected_response) = path.challenge {
                if expected_response == response {
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

    fn matching_path(
        &self,
        path: &Path,
        peer_address: &SocketAddress,
        destination_connection_id: &connection::Id,
    ) -> bool {
        path.peer_socket_address == *peer_address
            && path.destination_connection_id == *destination_connection_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::time::Duration;
    use s2n_quic_core::recovery::RTTEstimator;

    #[test]
    fn get_path_by_address_test() {
        let first_conn_id = connection::Id::try_from_bytes(&[0, 1, 2, 3, 4, 5]).unwrap();
        let second_conn_id = connection::Id::try_from_bytes(&[5, 4, 3, 2, 1, 0]).unwrap();
        let unused_conn_id = connection::Id::try_from_bytes(&[2, 4, 6, 8, 10, 12]).unwrap();
        let first_path = Path::new(
            first_conn_id,
            SocketAddress::default(),
            first_conn_id,
            RTTEstimator::new(Duration::from_millis(30)),
        );
        let second_path = Path::new(
            second_conn_id,
            SocketAddress::default(),
            second_conn_id,
            RTTEstimator::new(Duration::from_millis(30)),
        );

        let mut manager = Manager::default();
        manager.insert(first_path);
        manager.insert(second_path);

        let first_match = manager
            .path(&SocketAddress::default(), &first_conn_id)
            .unwrap();
        let second_match = manager
            .path(&SocketAddress::default(), &second_conn_id)
            .unwrap();
        assert_eq!(first_match, &first_path);
        assert_eq!(second_match, &second_path);
        assert_eq!(
            manager.path(&SocketAddress::default(), &unused_conn_id),
            None
        );
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
        first_path.challenge = Some([0u8; 32]);

        let mut manager = Manager::default();
        manager.insert(first_path);
        {
            let first_path = manager
                .path(&first_path.peer_socket_address, &first_conn_id)
                .unwrap();
            assert_eq!(first_path.is_validated(), false);
        }
        manager.on_path_response(
            &first_path.peer_socket_address,
            &first_path.destination_connection_id,
            &first_path.challenge.unwrap(),
        );
        {
            let first_path = manager
                .path(&first_path.peer_socket_address, &first_conn_id)
                .unwrap();
            assert_eq!(first_path.is_validated(), true);
        }
    }

    #[test]
    fn new_peer_test() {
        let first_conn_id = connection::Id::try_from_bytes(&[0, 1, 2, 3, 4, 5]).unwrap();
        let new_conn_id = connection::Id::try_from_bytes(&[5, 4, 3, 2, 1, 0]).unwrap();
        let first_path = Path::new(
            first_conn_id,
            SocketAddress::default(),
            first_conn_id,
            RTTEstimator::new(Duration::from_millis(30)),
        );

        let mut manager = Manager::default();
        manager.insert(first_path);
        assert_eq!(
            manager.is_new_path(&SocketAddress::default(), &first_conn_id),
            false
        );
        assert_eq!(
            manager.is_new_path(&SocketAddress::default(), &new_conn_id),
            true
        );
    }
}

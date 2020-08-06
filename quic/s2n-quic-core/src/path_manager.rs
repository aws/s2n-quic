//! This module contains the PathManager implementation

use crate::{
    connection::self,
    inet::SocketAddress,
    path::Path
};
use smallvec::SmallVec;

/// The amount of Paths that can be maintained without using the heap
const STATIC_DEFAULT_PATHS: usize = 5;

/// Track challenge data for a particular path
struct PathSecretTuple(Path, [u8; 32]);

/// The PathManager handles paths for a specific connection.
/// It will handle path validation operations, and track the active path for a connection.
pub struct PathManager {
    /// Path array
    paths: SmallVec<[PathSecretTuple; STATIC_DEFAULT_PATHS]>,

    /// Index to the active path
    active: usize,
}

impl PathManager {
    pub fn new() -> Self {
        Self {
            paths: SmallVec::new(),
            active: 0,
        }
    }

    /// Return the active path
    pub fn get_active_path(&self) -> &Path {
        &self.paths[self.active].0
    }

    /// Return a mutable reference to the active path
    pub fn get_active_path_mut(&mut self) -> &mut Path {
        &mut self.paths[self.active].0
    }

    /// Returns whether the socket address belongs to the current peer or an in progress peer
    pub fn is_new_path(
        &self,
        peer_address: &SocketAddress,
        destination_connection_id: &connection::Id,
    ) -> bool {
        match self.paths.iter().position(|path| {
            path.0.peer_socket_address == *peer_address
                && path.0.destination_connection_id == *destination_connection_id
        }) {
            Some(_) => return false,
            None => return true,
        };
    }

    /// Returns the Path for the connection id if the PathManager knows about it
    pub fn get_path_by_address(
        &self,
        peer_address: &SocketAddress,
        destination_connection_id: &connection::Id,
    ) -> Option<&Path> {
        match self.paths.iter().position(|path| {
            path.0.peer_socket_address == *peer_address
                && path.0.destination_connection_id == *destination_connection_id
        }) {
            Some(path_index) => return Some(&self.paths[path_index].0),
            None => return None,
        };
    }

    /// Returns the Path for the connection id if the PathManager knows about it
    pub fn get_path_address_mut(
        &mut self,
        peer_address: &SocketAddress,
        destination_connection_id: &connection::Id,
    ) -> Option<&mut Path> {
        match self.paths.iter().position(|path| {
            path.0.peer_socket_address == *peer_address
                && path.0.destination_connection_id == *destination_connection_id
        }) {
            Some(path_index) => return Some(&mut self.paths[path_index].0),
            None => return None,
        };
    }

    /// Add a new path to the PathManager
    pub fn add_new_path(&mut self, path: Path) {
        self.paths.push(PathSecretTuple(path, [0u8; 32]));
    }

    /// Called when a PATH_CHALLENGE frame is received
    pub fn on_path_challenge(
        &mut self,
        _peer_address: &SocketAddress,
        _destination_connection_id: &connection::Id,
        _challenge: &[u8],
    ) {
    }

    /// Called when a PATH_RESPONSE frame is received
    pub fn on_path_response(
        &mut self,
        peer_address: &SocketAddress,
        destination_connection_id: &connection::Id,
        response: &[u8],
    ) {
        match self.paths.iter().position(|path| {
            path.0.peer_socket_address == *peer_address
                && path.0.destination_connection_id == *destination_connection_id
        }) {
            Some(path_index) => {
                if response == self.paths[path_index].1 {
                    self.paths[path_index].0.on_validated();
                }
            }
            None => (),
        };
    }

    /// Called when a token is received that was issued in a Retry packet
    pub fn on_retry_token(&self, _peer_address: &SocketAddress, _token: &[u8]) {}

    /// Called when a token is received that was issued in a NEW_TOKEN frame
    pub fn on_new_token(&self, _peer_address: &SocketAddress, _token: &[u8]) {}

    /// Start the validation process for a path
    pub fn validate_path(&self, _path: Path) {}

    pub fn on_connection_id_retire(&self, _connenction_id: &connection::Id) {}

    pub fn on_connection_id_new(&self, _connection_id: &connection::Id) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recovery::RTTEstimator;
    use core::time::Duration;

    #[test]
    fn get_path_by_address_test() {
        let mut manager = PathManager::new();
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

        manager.add_new_path(first_path);
        manager.add_new_path(second_path);

        let first_match = manager
            .get_path_by_address(&SocketAddress::default(), &first_conn_id)
            .unwrap();
        let second_match = manager
            .get_path_by_address(&SocketAddress::default(), &second_conn_id)
            .unwrap();
        assert_eq!(first_match, &first_path);
        assert_eq!(second_match, &second_path);
        assert_eq!(
            manager.get_path_by_address(&SocketAddress::default(), &unused_conn_id),
            None
        );
    }

    #[test]
    fn path_validate_test() {
        let first_conn_id = connection::Id::try_from_bytes(&[0, 1, 2, 3, 4, 5]).unwrap();
        let first_path = Path::new(
            first_conn_id,
            SocketAddress::default(),
            first_conn_id,
            RTTEstimator::new(Duration::from_millis(30)),
        );
        let response = [0u8; 32];

        let mut manager = PathManager::new();
        manager.add_new_path(first_path);
        {
            let first_path = manager
                .get_path_by_address(&first_path.peer_socket_address, &first_conn_id)
                .unwrap();
            assert_eq!(first_path.is_validated(), false);
        }
        manager.on_path_response(
            &first_path.peer_socket_address,
            &first_path.destination_connection_id,
            &response,
        );
        {
            let first_path = manager
                .get_path_by_address(&first_path.peer_socket_address, &first_conn_id)
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

        let mut manager = PathManager::new();
        manager.add_new_path(first_path);
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

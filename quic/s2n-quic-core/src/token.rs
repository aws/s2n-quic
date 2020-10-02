use crate::{connection, inet::SocketAddress};
use core::time::Duration;

pub trait Format {
    const TOKEN_LEN: usize;

    /// Called when a token is needed for a NEW_TOKEN frame.
    fn generate_new_token(
        &mut self,
        peer_address: &SocketAddress,
        destination_connection_id: &connection::Id,
        source_connection_id: &connection::Id,
        output_buffer: &mut [u8],
    ) -> Option<Duration>;

    /// Called when a token is needed for a Retry Packet.
    fn generate_retry_token(
        &mut self,
        peer_address: &SocketAddress,
        destination_connection_id: &connection::Id,
        source_connection_id: &connection::Id,
        output_buffer: &mut [u8],
    ) -> Option<Duration>;

    /// Called to validate a token.
    fn validate_token(
        &mut self,
        peer_address: &SocketAddress,
        destination_connection_id: &connection::Id,
        source_connection_id: &connection::Id,
        token: &[u8],
    ) -> Option<Source>;

    /// Called to return the hash of a token for de-duplication purposes
    fn token_hash<'a>(&self, token: &'a [u8]) -> &'a [u8];
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Source {
    RetryPacket,
    NewTokenFrame,
}

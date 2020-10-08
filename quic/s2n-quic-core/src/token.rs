use crate::{connection, inet::SocketAddress};

pub trait Format {
    const TOKEN_LEN: usize;

    /// Generate a signed token to be delivered in a NEW_TOKEN frame.
    /// This function will only be called if the provider support NEW_TOKEN frames.
    fn generate_new_token(
        &mut self,
        peer_address: &SocketAddress,
        destination_connection_id: &connection::Id,
        source_connection_id: &connection::Id,
        output_buffer: &mut [u8],
    ) -> Option<()>;

    /// Generate a signed token to be delivered in a Retry Packet
    fn generate_retry_token(
        &mut self,
        peer_address: &SocketAddress,
        destination_connection_id: &connection::Id,
        source_connection_id: &connection::Id,
        output_buffer: &mut [u8],
    ) -> Option<()>;

    /// Return the source of a valid token.
    /// If the token is invalid, return None.
    /// Callers should detect duplicate tokens and treat them as invalid.
    fn validate_token(
        &mut self,
        peer_address: &SocketAddress,
        destination_connection_id: &connection::Id,
        source_connection_id: &connection::Id,
        token: &[u8],
    ) -> Option<Source>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Source {
    RetryPacket,
    NewTokenFrame,
}

/// Provides retry token support for an endpoint
use s2n_quic_core::{
    connection::ConnectionId,
    inet::SocketAddress,
};
use core::time::Duration;

//= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#8.1.1
//#   A token sent in a NEW_TOKEN frames or a Retry packet MUST be
//#   constructed in a way that allows the server to identify how it was
//#   provided to a client.  These tokens are carried in the same field,
//#   but require different handling from servers.
#[derive(Debug, PartialEq)]
pub enum TokenType {
    RetryToken,
    NewToken,
}

pub trait Provider {
    type RetryToken: 'static + Send;
    type Error: core::fmt::Display;

    /// Called when a token is needed for a NEW_TOKEN frame.
    fn generate_new_token(
        &mut self,
        peer_address: &SocketAddress,
        destination_connection_id: &ConnectionId,
        source_connection_id: &ConnectionId,
        output_buffer: &mut [u8]) -> (usize, Duration);

    /// Called when a token is needed for a Retry Packet.
    fn generate_retry_token(
        &mut self,
        peer_address: &SocketAddress,
        destination_connection_id: &ConnectionId,
        source_connection_id: &ConnectionId,
        output_buffer: &mut [u8]) -> (usize, Duration);

    /// Called to validate a token.
    fn is_token_valid(&mut self, peer_address: &SocketAddress, destination_connection_id: &ConnectionId, source_connection_id: &ConnectionId, token: &[u8]) -> bool;

    /// Called to return the hash of a token for de-duplication purposes
    fn get_token_hash(&self, token: &[u8]) -> &[u8];
}

    fn start(self) -> Result<Self::RetryToken, Self::Error>;
}

pub use default::Provider as Default;

impl_provider_utils!();

pub mod default {
    #[derive(Debug, Default)]
    pub struct Provider;

    impl super::Provider for Provider {
        type RetryToken = (); // TODO
        type Error = core::convert::Infallible;

        fn start(self) -> Result<Self::RetryToken, Self::Error> {
            // TODO
            Ok(())
        }

impl Provider for Default {

}

impl_provider_utils!();

#[cfg(test)]
mod token_tests {
    use super::*;
    use s2n_codec::{DecoderBufferMut, EncoderBuffer};

    #[test]
    fn test_encoding() {
        let peer_address = SocketAddressV4::new([127, 0, 0, 1], 80).into();
        let dcid = ConnectionId::default();
        let scid = ConnectionId::default();
        let mut token_buffer = vec![0; MAX_ADDRESS_VALIDATION_TOKEN_LEN];

        let nonce: [u8; 16] = [1; 16];
        let mac: [u8; 32] = [2; 32];
        let token = Default {
            token_type: TokenType::NewToken,
            ipv4_peer_address: Some(SocketAddressV4::new([127, 0, 0, 1], 80).into()),
            ipv6_peer_address: None,
            lifetime: 0,
            nonce,
            mac,
        };

        let mut encoder = EncoderBuffer::new(&mut b);
        token.encode(&mut encoder);

        let decoder = DecoderBufferMut::new(&mut b);
        let (decoded_token, _) = decoder.decode::<AddressValidationToken>().unwrap();

        assert_eq!(token.token_type, decoded_token.token_type);
        assert_eq!(token.nonce, decoded_token.nonce);
        assert_eq!(token.mac, decoded_token.mac);
        assert_eq!(token.lifetime, decoded_token.lifetime);
        assert_eq!(token.ipv4_peer_address, decoded_token.ipv4_peer_address);
        assert_eq!(token.ipv6_peer_address, decoded_token.ipv6_peer_address);
    }
}

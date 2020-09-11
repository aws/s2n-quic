//! Default provider for Address Validation
//!
//! Customers will use the default Provider to generate and verify address validation tokens. This
//! means the actual token does not need to be exposed.

use core::time::Duration;
use s2n_quic_core::{connection, inet::SocketAddress};

pub trait Provider {
    type Token: 'static + Send;
    type Error: core::fmt::Display;

    /// Called when a token is needed for a NEW_TOKEN frame.
    fn generate_new_token(
        &mut self,
        peer_address: &SocketAddress,
        destination_connection_id: &connection::Id,
        source_connection_id: &connection::Id,
        output_buffer: &mut [u8],
    ) -> (usize, Duration);

    /// Called when a token is needed for a Retry Packet.
    fn generate_retry_token(
        &mut self,
        peer_address: &SocketAddress,
        destination_connection_id: &connection::Id,
        source_connection_id: &connection::Id,
        output_buffer: &mut [u8],
    ) -> (usize, Duration);

    /// Called to validate a token.
    fn is_token_valid(
        &mut self,
        peer_address: &SocketAddress,
        destination_connection_id: &connection::Id,
        source_connection_id: &connection::Id,
        token: &[u8],
    ) -> bool;

    /// Called to return the hash of a token for de-duplication purposes
    fn get_token_hash(&self, token: &[u8]) -> &[u8];

    fn start(self) -> Result<Self::Token, Self::Error>;
}

pub use default::Provider as Default;

pub mod default {
    use crate::address_validation;
    use core::time::Duration;
    use s2n_codec::{EncoderBuffer, EncoderValue};
    use s2n_quic_core::{connection, inet::SocketAddress};

    #[derive(Debug, Default)]
    pub struct Provider;

    fn random_bytes(output_buffer: &mut [u8]) {
        output_buffer.copy_from_slice(&[1; 32])
    }

    fn generate_unsigned_token() -> address_validation::Token {
        let mut nonce = [0u8; address_validation::NONCE_LEN];
        random_bytes(&mut nonce);

        address_validation::Token {
            version: address_validation::TOKEN_VERSION,
            master_key_id: 0x01,
            key_id: 0x01,
            token_type: address_validation::TokenType::NewToken,
            nonce: nonce,
            hmac: [0; 32],
        }
    }

    fn sign_token(
        _peer_address: &SocketAddress,
        _destination_connection_id: &connection::Id,
        _source_connection_id: &connection::Id,
        _token: &mut address_validation::Token) {
        // TODO sign the token
    }

    impl super::Provider for Provider {
        type Token = (); // TODO
        type Error = core::convert::Infallible;

        fn generate_new_token(
            &mut self,
            peer_address: &SocketAddress,
            destination_connection_id: &connection::Id,
            source_connection_id: &connection::Id,
            mut output_buffer: &mut [u8],
        ) -> (usize, Duration) {
            let mut token = generate_unsigned_token();
            token.token_type = address_validation::TokenType::NewToken;

            sign_token(peer_address, destination_connection_id, source_connection_id, &mut token);

            let mut encoder = EncoderBuffer::new(&mut output_buffer);
            token.encode(&mut encoder);
            (token.encoding_size(), Duration::from_millis(0))
        }

        /// Called when a token is needed for a Retry Packet.
        fn generate_retry_token(
            &mut self,
            peer_address: &SocketAddress,
            destination_connection_id: &connection::Id,
            source_connection_id: &connection::Id,
            mut output_buffer: &mut [u8],
        ) -> (usize, Duration) {
            let mut token = generate_unsigned_token();
            token.token_type = address_validation::TokenType::NewToken;

            sign_token(peer_address, destination_connection_id, source_connection_id, &mut token);

            let mut encoder = EncoderBuffer::new(&mut output_buffer);
            token.encode(&mut encoder);
            (token.encoding_size(), Duration::from_millis(0))
        }

        fn is_token_valid(
            &mut self,
            _peer_address: &SocketAddress,
            _destination_connection_id: &connection::Id,
            _source_connection_id: &connection::Id,
            _token: &[u8],
        ) -> bool {
            false
        }

        fn get_token_hash(&self, _token: &[u8]) -> &[u8] {
            &[0; 32]
        }

        /// Called to validate a token.
        fn start(self) -> Result<Self::Token, Self::Error> {
            // TODO
            Ok(())
        }
    }
}

impl_provider_utils!();

#[cfg(test)]
mod tests {
    use super::*;
    use crate::address_validation;
    use s2n_codec::DecoderBufferMut;

    #[test]
    fn test_token_signing() {
        let peer_address = &SocketAddress::default();
        let connection_id = &connection::Id::try_from_bytes(&[]).unwrap();
        let mut buf = [0u8; 512];
        let mut provider = default::Provider::default();

        let (_size, _lifetime) = provider.generate_new_token(peer_address, connection_id, connection_id, &mut buf);
        let decoder = DecoderBufferMut::new(&mut buf);
        let (decoded_token, _) = decoder.decode::<address_validation::Token>().unwrap();
        assert_eq!(*decoded_token.token_type(), address_validation::TokenType::NewToken);

        let (_size, _lifetime) = provider.generate_retry_token(peer_address, connection_id, connection_id, &mut buf);
        let decoder = DecoderBufferMut::new(&mut buf);
        let (decoded_token, _) = decoder.decode::<address_validation::Token>().unwrap();
        assert_eq!(*decoded_token.token_type(), address_validation::TokenType::NewToken);

    }

}

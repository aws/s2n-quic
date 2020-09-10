/// Provides address validation token support for an endpoint
use s2n_quic_core::{address_validation_token::{AddressValidationToken, TokenType}, connection, inet::SocketAddress};
use core::time::Duration;

pub trait Provider {
    type AddressValidationToken: 'static + Send;
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

    fn start(self) -> Result<Self::AddressValidationToken, Self::Error>;
}

pub use default::Provider as Default;

impl_provider_utils!();

pub mod default {
    use core::time::Duration;
    use s2n_quic_core::{connection, inet::SocketAddress};

    #[derive(Debug, Default)]
    pub struct Provider;

    impl super::Provider for Provider {
        type AddressValidationToken = (); // TODO
        type Error = core::convert::Infallible;

        fn generate_new_token(
            &mut self,
            peer_address: &SocketAddress,
            destination_connection_id: &connection::Id,
            source_connection_id: &connection::Id,
            output_buffer: &mut [u8],
        ) -> (usize, Duration) {
            (0, Duration::from_millis(0))
        }

        /// Called when a token is needed for a Retry Packet.
        fn generate_retry_token(
            &mut self,
            peer_address: &SocketAddress,
            destination_connection_id: &connection::Id,
            source_connection_id: &connection::Id,
            output_buffer: &mut [u8],
        ) -> (usize, Duration) {
            (0, Duration::from_millis(0))
        }

        /// Called to validate a token.
        fn start(self) -> Result<Self::AddressValidationToken, Self::Error> {
            // TODO
            Ok(())
        }
    }
}

impl_provider_utils!();

#[cfg(test)]
mod tests {
    use super::*;
    use s2n_codec::{DecoderBufferMut, EncoderBuffer};

    #[test]
    fn test_encoding() {
        let connection_id_bytes = [0u8; MAX_LEN];
        let dcid = connection::Id::try_from_bytes(&connection_id_bytes);
        let scid = connection::Id::try_from_bytes(&connection_id_bytes);
        let mut token_buffer = vec![0; MAX_ADDRESS_VALIDATION_TOKEN_LEN];

        let nonce: [u8; 32] = [1; 32];
        let mac: [u8; 32] = [2; 32];
        let token = AddressValidationToken {
            version: 0,
            master_key_id: 0,
            key_id: 0,
            token_type: TokenType::NewToken,
            nonce,
            mac,
        };

        let mut encoder = EncoderBuffer::new(&mut b);
        token.encode(&mut encoder);

        let decoder = DecoderBufferMut::new(&mut b);
        let (decoded_token, _) = decoder.decode::<AddressValidationToken>().unwrap();

        assert_eq!(token.token_type, decoded_token.token_type);
        assert_eq!(token.nonce, decoded_token.nonce);
        assert_eq!(token.hmac, decoded_token.mac);
    }
}

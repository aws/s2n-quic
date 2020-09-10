//! Defines the Address Validation token

use s2n_codec::{decoder_value, DecoderBuffer, DecoderBufferMut, DecoderError, Encoder, EncoderValue};

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

impl<'a> EncoderValue for TokenType {
    fn encode<E: Encoder>(&self, buffer: &mut E) {
        match self {
            TokenType::RetryToken => 0u8.encode(buffer),
            TokenType::NewToken => 1u8.encode(buffer),
        }
    }
}

decoder_value!(
    impl<'a> TokenType {
        fn decode(buffer: Buffer) -> Result<Self> {
            let (value, buffer) = buffer.decode::<u8>()?;
            match value {
                0x00 => Ok((TokenType::RetryToken, buffer)),
                0x01 => Ok((TokenType::NewToken, buffer)),
                _ => Err(DecoderError::InvariantViolation("Invalid token type")),
            }
        }
    }
);

/// Maximum size of an address validation token
const MAX_ADDRESS_VALIDATION_TOKEN_LEN: usize = 512;

//= https://tools.ietf.org/html/draft-ietf-quic-transport-29.txt#8.1.4
//#   There is no need for a single well-defined format for the token
//#   because the server that generates the token also consumes it.
pub struct AddressValidationToken {
    version: u8,
    master_key_id: u8,
    key_id: u8,
    token_type: TokenType,
    nonce: [u8; 32],
    hmac: [u8; 32],
}


impl AddressValidationToken {
    pub fn version(&self) -> u8 {
        self.version
    }

    pub fn master_key_id(&self) -> u8 {
        self.master_key_id 
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#21.2
    //#   An attacker might be able to receive an address validation token
    //#   (Section 8) from a server and then release the IP address it used to
    //#   acquire that token.
    //#   Servers SHOULD provide mitigations for this attack by limiting the
    //#   usage and lifetime of address validation tokens
    pub fn key_id(&self) -> u8 { 
        self.key_id
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#8.1.1
    //#   A token sent in a NEW_TOKEN frames or a Retry packet MUST be
    //#   constructed in a way that allows the server to identify how it was
    //#   provided to a client.  These tokens are carried in the same field,
    //#   but require different handling from servers.
    pub fn token_type(&self) -> TokenType {
        TokenType::NewToken
    }

    //= https://tools.ietf.org/html/draft-ietf-quic-transport-29.txt#8.1.3
    //#   An address validation token MUST be difficult to guess.  Including a
    //#   large enough random value in the token would be sufficient, but this
    //#   depends on the server remembering the value it sends to clients.
    pub fn nonce(&self) -> &[u8] { &self.nonce }

    //= https://tools.ietf.org/html/draft-ietf-quic-transport-29.txt#8.1.3
    //#   A token-based scheme allows the server to offload any state
    //#   associated with validation to the client.  For this design to work,
    //#   the token MUST be covered by integrity protection against
    //#   modification or falsification by clients.  Without integrity
    //#   protection, malicious clients could generate or guess values for
    //#   tokens that would be accepted by the server.  Only the server
    //#   requires access to the integrity protection key for tokens.
    pub fn hmac(&self) -> &[u8] { &self.hmac }

    //= https://tools.ietf.org/html/draft-ietf-quic-transport-29.txt#8.1.3
    //#   When a server receives an Initial packet with an address validation
    //#   token, it MUST attempt to validate the token, unless it has already
    //#   completed address validation.
    pub fn validate(&self) -> bool {
        true
    }
}

const VERSION_MASK: u8 = 0x03;
const MKID_MASK: u8 = 0x00;

const VERSION_SHIFT: u8 = 0;
const MKID_SHIFT: u8 = 2;
const KID_SHIFT: u8 = 4;
const TOKEN_TYPE_SHIFT: u8 = 5;

impl<'a> EncoderValue for AddressValidationToken {
    fn encode<E: Encoder>(&self, buffer: &mut E) {
        let mut token_buf = [0; 1];
        let decoder = DecoderBufferMut::new(&mut token_buf);
        let (decoded_token, _) = decoder.decode::<TokenType>().unwrap();

        let first_byte = 
            (self.version() << VERSION_SHIFT) & 
            (self.master_key_id() << MKID_SHIFT) & 
            (self.key_id() << KID_SHIFT) &
            ((self.token_type() as u8) << TOKEN_TYPE_SHIFT);
        buffer.encode(&first_byte);
        buffer.encode(&self.master_key_id());
        buffer.encode(&self.key_id());
        buffer.encode(&self.token_type());
        buffer.encode(&self.nonce());
        buffer.encode(&self.hmac());
    }
}

impl From<&[u8]> for AddressValidationToken {
    fn from(bytes: &[u8]) -> Self {
        let decoder = DecoderBuffer::new(bytes);
        let (decoded_token, _) = decoder.decode::<AddressValidationToken>().unwrap();
        decoded_token
    }
}

decoder_value!(
    impl<'a> AddressValidationToken {
        fn decode(buffer: Buffer) -> Result<Self> {
            let (token_type, buffer) = buffer.decode::<TokenType>()?;
            let (nonce_slice, buffer) = buffer.decode_slice(16)?;
            let nonce_slice: &[u8] = nonce_slice.into_less_safe_slice();
            let mut nonce: [u8; 32] = [0; 32];
            nonce[..32].copy_from_slice(nonce_slice);
            let (mac_slice, buffer) = buffer.decode_slice(32)?;
            let mac_slice: &[u8] = mac_slice.into_less_safe_slice();
            let mut hmac: [u8; 32] = [0; 32];
            hmac[..32].copy_from_slice(mac_slice);

            let master_key_id = 0;
            let key_id = 0;
            let version = 0;
            let token = Self {
                version,
                master_key_id,
                key_id,
                token_type,
                nonce,
                hmac,
            };

            Ok((token, buffer))
        }
    }
);

#[cfg(test)]
mod token_tests {
    use super::*;
    use s2n_codec::{DecoderBufferMut, EncoderBuffer};

    #[test]
    fn test_encoding() {
        let nonce: [u8; 32] = [1; 32];
        let hmac: [u8; 32] = [2; 32];
        let token = AddressValidationToken {
            version: 0,
            master_key_id: 0,
            key_id: 0,
            token_type: TokenType::NewToken,
            nonce,
            hmac,
        };

        let mut b = vec![0; 128];
        let mut encoder = EncoderBuffer::new(&mut b);
        token.encode(&mut encoder);

        let decoder = DecoderBufferMut::new(&mut b);
        let (decoded_token, _) = decoder.decode::<AddressValidationToken>().unwrap();

        assert_eq!(token.token_type, decoded_token.token_type);
        assert_eq!(token.nonce, decoded_token.nonce);
        assert_eq!(token.hmac, decoded_token.hmac);
    }
}

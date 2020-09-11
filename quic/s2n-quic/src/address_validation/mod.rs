//! Defines the Address Validation Token
//!
//! Address Validation Token layout
//!
//! ```text
//!
//! The address validation token is 512 bytes long. This gives enough space for a SHA256 HMAC,
//! a 247 bit nonce, and 9 bits of meta information about the token.
//!
//! The first 9 bits of the token represent the version, master key id, key id, and token type.
//!
//! +----------+----------+--------------------+-------------+
//! |  Version |   MKID   |      Key ID        |  Token Type |
//! +----------+----------+--------------------+-------------+
//!      2           2              4                  1
//!
//! The next 247 bits are the nonce. The last 256 bits are the HMAC.
//!
//! ```

use core::convert::TryFrom;
use s2n_codec::{decoder_value, DecoderBuffer, DecoderError, Encoder, EncoderValue};

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

impl TryFrom<u8> for TokenType {
    type Error = &'static str;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(TokenType::RetryToken),
            1 => Ok(TokenType::NewToken),
            _ => Err("Invalid token type"),
        }
    }
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
pub struct Token {
    pub version: u8,
    pub master_key_id: u8,
    pub key_id: u8,
    pub token_type: TokenType,
    pub nonce: [u8; 32],
    pub hmac: [u8; 32],
}

impl Token {
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
    pub fn token_type(&self) -> &TokenType {
        &self.token_type
    }

    //= https://tools.ietf.org/html/draft-ietf-quic-transport-29.txt#8.1.3
    //#   An address validation token MUST be difficult to guess.  Including a
    //#   large enough random value in the token would be sufficient, but this
    //#   depends on the server remembering the value it sends to clients.
    pub fn nonce(&self) -> &[u8] {
        &self.nonce
    }

    //= https://tools.ietf.org/html/draft-ietf-quic-transport-29.txt#8.1.3
    //#   A token-based scheme allows the server to offload any state
    //#   associated with validation to the client.  For this design to work,
    //#   the token MUST be covered by integrity protection against
    //#   modification or falsification by clients.  Without integrity
    //#   protection, malicious clients could generate or guess values for
    //#   tokens that would be accepted by the server.  Only the server
    //#   requires access to the integrity protection key for tokens.
    pub fn hmac(&self) -> &[u8] {
        &self.hmac
    }

    //= https://tools.ietf.org/html/draft-ietf-quic-transport-29.txt#8.1.3
    //#   When a server receives an Initial packet with an address validation
    //#   token, it MUST attempt to validate the token, unless it has already
    //#   completed address validation.
    pub fn validate(&self) -> bool {
        true
    }
}

pub const NONCE_LEN: usize = 32;

pub const TOKEN_VERSION: u8 = 0x01;

const VERSION_MASK: u8 = 0x03;
const MKID_MASK: u8 = 0x0c;
const KID_MASK: u8 = 0xf0;
const TOKEN_TYPE_MASK: u8 = 0x80;

const VERSION_SHIFT: u8 = 0;
const MKID_SHIFT: u8 = 2;
const KID_SHIFT: u8 = 4;
const TOKEN_TYPE_SHIFT: u8 = 7;

impl<'a> EncoderValue for Token {
    fn encode<E: Encoder>(&self, buffer: &mut E) {
        let t = self.token_type();
        let token_type = match t {
            TokenType::RetryToken => 0,
            TokenType::NewToken => 1,
        };

        let first_byte = (self.version() << VERSION_SHIFT)
            | (self.master_key_id() << MKID_SHIFT)
            | (self.key_id() << KID_SHIFT);
        let mut nonce = self.nonce()[0];
        nonce |= token_type << TOKEN_TYPE_SHIFT;
        let la = &self.nonce()[1..];
        buffer.encode(&first_byte);
        buffer.encode(&nonce);
        buffer.encode(&la);
        buffer.encode(&self.hmac());
    }
}

impl From<&[u8]> for Token {
    fn from(bytes: &[u8]) -> Self {
        let decoder = DecoderBuffer::new(bytes);
        let (decoded_token, _) = decoder.decode::<Token>().unwrap();
        decoded_token
    }
}

decoder_value!(
    impl<'a> Token {
        fn decode(buffer: Buffer) -> Result<Self> {
            let (first_byte, buffer) = buffer.decode::<u8>()?;
            let version = (first_byte & VERSION_MASK) >> VERSION_SHIFT;
            let master_key_id = (first_byte & MKID_MASK) >> MKID_SHIFT;
            let key_id = (first_byte & KID_MASK) >> KID_SHIFT;

            let (nonce_slice, buffer) = buffer.decode_slice(32)?;
            let nonce_slice: &[u8] = nonce_slice.into_less_safe_slice();
            let mut nonce: [u8; 32] = [0; 32];
            nonce[..32].copy_from_slice(nonce_slice);

            let token_type = (nonce[0] & TOKEN_TYPE_MASK) >> TOKEN_TYPE_SHIFT;
            let token_type = TokenType::try_from(token_type).unwrap();
            nonce[0] &= !TOKEN_TYPE_MASK;

            let (hmac_slice, buffer) = buffer.decode_slice(32)?;
            let hmac_slice: &[u8] = hmac_slice.into_less_safe_slice();
            let mut hmac: [u8; 32] = [0; 32];
            hmac[..32].copy_from_slice(hmac_slice);

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
mod tests {
    use super::*;
    use s2n_codec::{DecoderBufferMut, EncoderBuffer};

    #[test]
    fn test_encoding_decoding() {
        let nonce: [u8; 32] = [1; 32];
        let hmac: [u8; 32] = [2; 32];
        let token = Token {
            version: 0x01,
            master_key_id: 0x02,
            key_id: 0x01,
            token_type: TokenType::RetryToken,
            nonce,
            hmac,
        };

        let mut b = vec![0; MAX_ADDRESS_VALIDATION_TOKEN_LEN];
        let mut encoder = EncoderBuffer::new(&mut b);
        token.encode(&mut encoder);

        let decoder = DecoderBufferMut::new(&mut b);
        let (decoded_token, _) = decoder.decode::<Token>().unwrap();

        assert_eq!(token.version(), decoded_token.version());
        assert_eq!(token.master_key_id(), decoded_token.master_key_id());
        assert_eq!(token.key_id(), decoded_token.key_id());
        assert_eq!(token.token_type(), decoded_token.token_type());
        assert_eq!(token.nonce(), decoded_token.nonce());
        assert_eq!(token.hmac(), decoded_token.hmac());

        let nonce: [u8; 32] = [1; 32];
        let hmac: [u8; 32] = [2; 32];
        let token = Token {
            version: 0x02,
            master_key_id: 0x01,
            key_id: 0x05,
            token_type: TokenType::NewToken,
            nonce,
            hmac,
        };

        let mut b = vec![0; MAX_ADDRESS_VALIDATION_TOKEN_LEN];
        let mut encoder = EncoderBuffer::new(&mut b);
        token.encode(&mut encoder);

        let decoder = DecoderBufferMut::new(&mut b);
        let (decoded_token, _) = decoder.decode::<Token>().unwrap();

        assert_eq!(token.version(), decoded_token.version());
        assert_eq!(token.master_key_id(), decoded_token.master_key_id());
        assert_eq!(token.key_id(), decoded_token.key_id());
        assert_eq!(token.token_type(), decoded_token.token_type());
        assert_eq!(token.nonce(), decoded_token.nonce());
        assert_eq!(token.hmac(), decoded_token.hmac());
    }
}

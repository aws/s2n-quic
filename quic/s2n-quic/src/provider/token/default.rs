//! Defines the Address Validation Token
//!
//! Address Validation Token layout
//!
//! ```text
//!
//! The address validation token is 512 bytes long. This gives enough space for a SHA256 HMAC,
//! a 248 bit nonce, and 8 bits of meta information about the token.
//!
//! The first 8 bits of the token represent the version, token type, key id, time window ID.
//!
//! +----------+--------------+--------+----------------+
//! |  Version | Token Source | Key ID | Time Window ID |
//! +----------+--------------+--------+----------------+
//!      1           1            2           4
//!
//! The next 248 bits are the nonce. The last 256 bits are the HMAC.
//!
//! ```

use core::{mem::size_of, time::Duration};
use ring::rand::{SecureRandom, SystemRandom};
use s2n_codec::{DecoderBuffer, DecoderBufferMut};
use s2n_quic_core::{connection, inet::SocketAddress, token::Source};
use zerocopy::{AsBytes, FromBytes, Unaligned};

#[derive(Debug, Default)]
pub struct Provider {
    new_tokens: bool,
    new_token_validate_port: bool,
    retry_tokens: bool,
}

impl super::Provider for Provider {
    type Format = Format;
    type Error = core::convert::Infallible;

    fn start(self) -> Result<Self::Format, Self::Error> {
        // TODO: Start timer to update key
        Ok(Format {
            new_tokens: self.new_tokens,
            new_token_validate_port: self.new_token_validate_port,
            retry_tokens: self.retry_tokens,
        })
    }
}

pub struct DerivedKey();

#[derive(Debug, Default)]
pub struct Format {
    /// Support tokens in NEW_TOKEN frames
    new_tokens: bool,

    /// Validate source port from NEW_TOKEN frames
    new_token_validate_port: bool,

    /// Support tokens from Retry Requests
    retry_tokens: bool,
}

impl Format {
    fn generate_token(
        &mut self,
        source: Source,
        _peer_address: &SocketAddress,
        _destination_connection_id: &connection::Id,
        _source_connection_id: &connection::Id,
        output_buffer: &mut [u8],
    ) -> Option<Duration> {
        let buffer = DecoderBufferMut::new(output_buffer);
        let (token, _) = buffer
            .decode::<&mut Token>()
            .expect("Provided output buffer did not match TOKEN_LEN");

        // TODO
        let current_key_id = 0;
        let current_time_window_id = 0;
        let header = Header::new(current_key_id, current_time_window_id, source);

        token.header = header;

        SystemRandom::new().fill(&mut token.nonce[..]).ok()?;

        // Sign the token, then write to the buffer
        todo!("Sign the token")
    }

    #[allow(dead_code)]
    fn sign_token(&self, _token: &mut [u8], _key: &DerivedKey) {
        todo!();
    }

    fn validate_retry_token(
        &mut self,
        _peer_address: &SocketAddress,
        _destination_connection_id: &connection::Id,
        _source_connection_id: &connection::Id,
        _token: &Token,
    ) -> Option<()> {
        todo!()
    }

    fn validate_new_token(&mut self, _peer_address: &SocketAddress, _token: &Token) -> Option<()> {
        todo!()
    }
}

impl super::Format for Format {
    const TOKEN_LEN: usize = size_of::<Token>();

    fn generate_new_token(
        &mut self,
        peer_address: &SocketAddress,
        destination_connection_id: &connection::Id,
        source_connection_id: &connection::Id,
        output_buffer: &mut [u8],
    ) -> Option<Duration> {
        self.generate_token(
            Source::NewTokenFrame,
            peer_address,
            destination_connection_id,
            source_connection_id,
            output_buffer,
        )
    }

    /// Called when a token is needed for a Retry Packet.
    fn generate_retry_token(
        &mut self,
        peer_address: &SocketAddress,
        destination_connection_id: &connection::Id,
        source_connection_id: &connection::Id,
        output_buffer: &mut [u8],
    ) -> Option<Duration> {
        self.generate_token(
            Source::RetryPacket,
            peer_address,
            destination_connection_id,
            source_connection_id,
            output_buffer,
        )
    }

    fn validate_token(
        &mut self,
        peer_address: &SocketAddress,
        destination_connection_id: &connection::Id,
        source_connection_id: &connection::Id,
        token: &[u8],
    ) -> Option<Source> {
        let buffer = DecoderBuffer::new(token);
        let (token, _) = buffer.decode::<&Token>().ok()?;

        if token.header.version() != TOKEN_VERSION {
            return None;
        }

        let source = token.header.token_source();

        match source {
            Source::RetryPacket => {
                self.validate_retry_token(
                    peer_address,
                    destination_connection_id,
                    source_connection_id,
                    token,
                )?;
                Some(source)
            }
            Source::NewTokenFrame => {
                self.validate_new_token(peer_address, token)?;
                Some(source)
            }
        }
    }

    fn token_hash<'a>(&self, token: &'a [u8]) -> &'a [u8] {
        let buffer = DecoderBuffer::new(token);
        let (token, _) = buffer
            .decode::<&Token>()
            .expect("Provided output buffer did not match TOKEN_LEN");
        &token.hmac[..]
    }
}

#[derive(Clone, Copy, Debug, FromBytes, AsBytes, Unaligned)]
#[repr(C)]
pub struct Header(u8);

pub const TOKEN_VERSION: u8 = 0x00;

const VERSION_MASK: u8 = 0x80;
const TOKEN_SOURCE_MASK: u8 = 0x40;
const KEY_ID_MASK: u8 = 0x30;
const TIME_WINDOW_MASK: u8 = 0x0f;

const VERSION_SHIFT: u8 = 7;
const TOKEN_SOURCE_SHIFT: u8 = 6;
const KEY_ID_SHIFT: u8 = 4;
const TIME_WINDOW_SHIFT: u8 = 0;

impl Header {
    pub fn new(key_id: u8, time_window_id: u8, source: Source) -> Header {
        let mut header: u8 = 0;
        header |= TOKEN_VERSION << VERSION_SHIFT;
        header |= key_id << KEY_ID_SHIFT;
        header |= time_window_id << TIME_WINDOW_SHIFT;
        header |= match source {
            Source::NewTokenFrame => 0 << TOKEN_SOURCE_SHIFT,
            Source::RetryPacket => 1 << TOKEN_SOURCE_SHIFT,
        };

        Header(header)
    }

    pub fn version(&self) -> u8 {
        (self.0 & VERSION_MASK) >> VERSION_SHIFT
    }

    pub fn key_id(&self) -> u8 {
        (self.0 & KEY_ID_MASK) >> KEY_ID_SHIFT
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#21.2
    //# Servers SHOULD provide mitigations for this attack by limiting the
    //# usage and lifetime of address validation tokens
    pub fn time_window_id(&self) -> u8 {
        (self.0 & TIME_WINDOW_MASK) >> TIME_WINDOW_SHIFT
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#8.1.1
    //# A token sent in a NEW_TOKEN frames or a Retry packet MUST be
    //# constructed in a way that allows the server to identify how it was
    //# provided to a client.  These tokens are carried in the same field,
    //# but require different handling from servers.
    pub fn token_source(&self) -> Source {
        match (self.0 & TOKEN_SOURCE_MASK) >> TOKEN_SOURCE_SHIFT {
            0 => Source::NewTokenFrame,
            1 => Source::RetryPacket,
            _ => Source::NewTokenFrame,
        }
    }
}

//= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#8.1.4
//#   There is no need for a single well-defined format for the token
//#   because the server that generates the token also consumes it.
#[derive(Copy, Clone, Debug, FromBytes, AsBytes, Unaligned)]
#[repr(C)]
pub struct Token {
    header: Header,
    nonce: [u8; 32],
    hmac: [u8; 32],
}

s2n_codec::zerocopy_value_codec!(Token);

impl Token {
    pub fn header(&self) -> Header {
        self.header
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#8.1.4
    //# An address validation token MUST be difficult to guess.  Including a
    //# large enough random value in the token would be sufficient, but this
    //# depends on the server remembering the value it sends to clients.
    pub fn nonce(&self) -> &[u8] {
        &self.nonce
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#8.1.4
    //# A token-based scheme allows the server to offload any state
    //# associated with validation to the client.  For this design to work,
    //# the token MUST be covered by integrity protection against
    //# modification or falsification by clients.  Without integrity
    //# protection, malicious clients could generate or guess values for
    //# tokens that would be accepted by the server.  Only the server
    //# requires access to the integrity protection key for tokens.
    pub fn hmac(&self) -> &[u8] {
        &self.hmac
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#8.1.3
    //# When a server receives an Initial packet with an address validation
    //# token, it MUST attempt to validate the token, unless it has already
    //# completed address validation.
    #[allow(dead_code)]
    pub fn validate(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use s2n_quic_core::token::Source;

    #[test]
    fn test_header() {
        // Test all combinations of values to create a header and verify the header returns the
        // expected values.
        for key_id in 0..2 {
            for time_window_id in 0..4 {
                for source in &[Source::NewTokenFrame, Source::RetryPacket] {
                    let header = Header::new(key_id, time_window_id, *source);
                    // The version should always be the constant TOKEN_VERSION
                    assert_eq!(header.version(), TOKEN_VERSION);
                    assert_eq!(header.key_id(), key_id);
                    assert_eq!(header.time_window_id(), time_window_id);
                    assert_eq!(header.token_source(), *source);
                }
            }
        }
    }
}

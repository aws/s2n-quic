//! Default provider for Address Validation tokens
//!
//! The default provider will randomly generate a 256 bit key. This key will be used to sign and
//! verify tokens. The key can be rotated at a duration set by the user.
//!
//! The default provider does not support tokens delivered in a NEW_TOKEN frame.

use core::{mem::size_of, time::Duration};
use ring::{
    digest, hmac,
    rand::{SecureRandom, SystemRandom},
};
use s2n_codec::{DecoderBuffer, DecoderBufferMut};
use s2n_quic_core::{connection, inet::SocketAddress, token::Source};
use zerocopy::{AsBytes, FromBytes, Unaligned};

#[derive(Debug)]
struct BaseKey {
    // HMAC key for signing and verifying
    hmac_key: hmac::Key,
}

impl BaseKey {
    fn new(key_material: &[u8; digest::SHA256_OUTPUT_LEN]) -> Self {
        let hmac_key = hmac::Key::new(hmac::HMAC_SHA256, key_material);

        Self { hmac_key }
    }
}

#[derive(Debug, Default)]
pub struct Provider {
    /// Rotate the key periodically
    key_rotation_period: Duration,

    /// Send tokens in NEW_TOKEN frame. If this is true, the library will call `generate_new_token`
    /// to provide tokens to client.
    support_new_token: bool,
}

impl super::Provider for Provider {
    type Format = Format;
    type Error = core::convert::Infallible;

    fn start(&self) -> Result<Self::Format, Self::Error> {
        // Subtract the lifetime from the current clock to force a key generation on the first
        // request
        let force_key_update = s2n_quic_platform::time::now()
            .checked_sub(self.key_rotation_period)
            .unwrap();

        Ok(Format {
            last_update: force_key_update,
            key_rotation_period: self.key_rotation_period,
            key: BaseKey::new(&[0; digest::SHA256_OUTPUT_LEN]),
        })
    }
}

pub struct Format {
    /// Last time the key was updated
    last_update: s2n_quic_core::time::Timestamp,

    /// Support tokens from Retry Packets
    key_rotation_period: Duration,

    /// Key used to derive signing keys
    key: BaseKey,
}

impl Format {
    fn generate_base_key(&self) -> Option<BaseKey> {
        // Generate a random key to sign and verify tokens
        let mut key_material = [0; digest::SHA256_OUTPUT_LEN];
        SystemRandom::new().fill(&mut key_material[..]).ok()?;

        Some(BaseKey::new(&key_material))
    }

    fn update_key(&mut self) -> Option<()> {
        if self.key_rotation_period == Duration::from_millis(0) {
            return None;
        }

        if let Some(age) = s2n_quic_platform::time::now().checked_sub(self.key_rotation_period) {
            if age >= self.last_update {
                self.key = self.generate_base_key()?;
                self.last_update = s2n_quic_platform::time::now();
            }
        };

        Some(())
    }

    fn generate_token(
        &mut self,
        source: Source,
        peer_address: &SocketAddress,
        destination_connection_id: &connection::Id,
        _source_connection_id: &connection::Id,
        output_buffer: &mut [u8],
    ) -> Option<()> {
        let buffer = DecoderBufferMut::new(output_buffer);
        let (token, _) = buffer
            .decode::<&mut Token>()
            .expect("Provided output buffer did not match TOKEN_LEN");

        let header = Header::new(source);

        token.header = header;

        // Populate the nonce before signing
        SystemRandom::new().fill(&mut token.nonce[..]).ok()?;

        let tag = self.tag(&token, &peer_address, &destination_connection_id);

        token.hmac.copy_from_slice(tag.as_ref());

        Some(())
    }

    fn tag(
        &mut self,
        token: &Token,
        peer_address: &SocketAddress,
        conn_id: &connection::Id,
    ) -> hmac::Tag {
        self.update_key();
        let mut ctx = hmac::Context::with_key(&self.key.hmac_key);

        ctx.update(&token.nonce);
        ctx.update(&conn_id.as_bytes());
        match peer_address {
            SocketAddress::IPv4(addr) => ctx.update(addr.as_bytes()),
            SocketAddress::IPv6(addr) => ctx.update(addr.as_bytes()),
        };

        ctx.sign()
    }

    fn validate_retry_token(
        &mut self,
        peer_address: &SocketAddress,
        destination_connection_id: &connection::Id,
        _source_connection_id: &connection::Id,
        token: &Token,
    ) -> bool {
        let tag = self.tag(&token, &peer_address, &destination_connection_id);
        ring::constant_time::verify_slices_are_equal(&token.hmac, &tag.as_ref()).is_ok()
    }
}

impl super::Format for Format {
    const TOKEN_LEN: usize = size_of::<Token>();

    /// The default provider does not support NEW_FRAME tokens
    fn generate_new_token(
        &mut self,
        _peer_address: &SocketAddress,
        _destination_connection_id: &connection::Id,
        _source_connection_id: &connection::Id,
        _output_buffer: &mut [u8],
    ) -> Option<()> {
        None
    }

    /// Generate a signed token to be delivered in a Retry Packet
    fn generate_retry_token(
        &mut self,
        peer_address: &SocketAddress,
        destination_connection_id: &connection::Id,
        source_connection_id: &connection::Id,
        output_buffer: &mut [u8],
    ) -> Option<()> {
        self.generate_token(
            Source::RetryPacket,
            peer_address,
            destination_connection_id,
            source_connection_id,
            output_buffer,
        )?;

        Some(())
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
                if self.validate_retry_token(
                    peer_address,
                    destination_connection_id,
                    source_connection_id,
                    token,
                ) {
                    return Some(source);
                }
                None
            }
            Source::NewTokenFrame => None, // Not supported in the default provider
        }
    }

    /// Called to return the hash of a token for de-duplication purposes
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

const VERSION_SHIFT: u8 = 7;
const VERSION_MASK: u8 = 0x80;

const TOKEN_SOURCE_SHIFT: u8 = 6;
const TOKEN_SOURCE_MASK: u8 = 0x40;

impl Header {
    pub fn new(source: Source) -> Header {
        let mut header: u8 = 0;
        header |= TOKEN_VERSION << VERSION_SHIFT;
        header |= match source {
            Source::NewTokenFrame => 0 << TOKEN_SOURCE_SHIFT,
            Source::RetryPacket => 1 << TOKEN_SOURCE_SHIFT,
        };

        Header(header)
    }

    pub fn version(&self) -> u8 {
        (self.0 & VERSION_MASK) >> VERSION_SHIFT
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
    pub fn validate(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use s2n_quic_core::token::{Format as FormatTrait, Source};
    use s2n_quic_platform::time;
    use std::sync::Arc;

    #[test]
    fn test_header() {
        // Test all combinations of values to create a header and verify the header returns the
        // expected values.
        for source in &[Source::NewTokenFrame, Source::RetryPacket] {
            let header = Header::new(*source);
            // The version should always be the constant TOKEN_VERSION
            assert_eq!(header.version(), TOKEN_VERSION);
            assert_eq!(header.token_source(), *source);
        }
    }

    #[test]
    fn test_valid_retry_token() {
        let clock = Arc::new(time::testing::MockClock::new());
        time::testing::set_local_clock(clock.clone());

        let mut format = Format {
            key_rotation_period: Duration::from_millis(5000),
            key: BaseKey::new(&[0; 32]),
            last_update: time::now(),
        };

        clock.adjust_by(Duration::from_millis(10000));
        let conn_id = connection::Id::try_from_bytes(&[]).unwrap();
        let addr = SocketAddress::default();
        let mut buf = [0; size_of::<Token>()];

        format
            .generate_retry_token(&addr, &conn_id, &conn_id, &mut buf)
            .unwrap();

        clock.adjust_by(Duration::from_millis(1000));
        assert_eq!(
            format
                .validate_token(&addr, &conn_id, &conn_id, &buf)
                .is_some(),
            true
        );
    }

    #[test]
    fn test_expired_retry_token() {
        let clock = Arc::new(time::testing::MockClock::new());
        time::testing::set_local_clock(clock.clone());

        let mut format = Format {
            key_rotation_period: Duration::from_millis(1000),
            key: BaseKey::new(&[0; 32]),
            last_update: time::now(),
        };

        clock.adjust_by(Duration::from_millis(10000));
        let conn_id = connection::Id::try_from_bytes(&[]).unwrap();
        let addr = SocketAddress::default();
        let mut buf = [0; size_of::<Token>()];
        format
            .generate_retry_token(&addr, &conn_id, &conn_id, &mut buf)
            .unwrap();

        clock.adjust_by(Duration::from_millis(1000));
        assert_eq!(
            format
                .validate_token(&addr, &conn_id, &conn_id, &buf)
                .is_none(),
            true
        );
    }

    #[test]
    fn test_retry_validation_default_format() {
        let clock = Arc::new(time::testing::MockClock::new());
        time::testing::set_local_clock(clock.clone());
        clock.adjust_by(Duration::from_millis(10000));

        let mut format = Format {
            key_rotation_period: Duration::from_millis(0),
            key: BaseKey::new(&[0; 32]),
            last_update: time::now(),
        };

        let conn_id = connection::Id::try_from_bytes(&[]).unwrap();
        let addr = SocketAddress::default();
        let mut buf = [0; size_of::<Token>()];
        format
            .generate_retry_token(&addr, &conn_id, &conn_id, &mut buf)
            .unwrap();

        assert_eq!(
            format
                .validate_token(&addr, &conn_id, &conn_id, &buf)
                .unwrap(),
            Source::RetryPacket
        );

        let wrong_conn_id = connection::Id::try_from_bytes(&[0, 1, 2]).unwrap();
        assert_eq!(
            format
                .validate_token(&addr, &wrong_conn_id, &conn_id, &buf)
                .is_none(),
            true
        );
    }
}

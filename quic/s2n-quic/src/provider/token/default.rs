//! Default provider for Address Validation tokens
//!
//! The default provider will randomly generate a 256 bit key. This key will be used to sign and
//! verify tokens. The key can be rotated at a duration set by the user.
//!
//! The default provider does not support tokens delivered in a NEW_TOKEN frame.

use core::{mem::size_of, time::Duration};
use hash_hasher::HashHasher;
use ring::{
    digest, hmac,
    rand::{SecureRandom, SystemRandom},
};
use s2n_codec::{DecoderBuffer, DecoderBufferMut};
use s2n_quic_core::{connection, inet::SocketAddress, time::Timestamp, token::Source};
use std::hash::{Hash, Hasher};
use zerocopy::{AsBytes, FromBytes, Unaligned};

struct BaseKey {
    active_duration: Duration,

    // HMAC key for signing and verifying
    key: Option<(Timestamp, hmac::Key)>,

    // Each key tracks tokens it has verified, preventing duplicates
    duplicate_filter: cuckoofilter::CuckooFilter<HashHasher>,
}

impl BaseKey {
    pub fn new(active_duration: Duration) -> Self {
        Self {
            active_duration,
            key: None,
            duplicate_filter: cuckoofilter::CuckooFilter::with_capacity(
                cuckoofilter::DEFAULT_CAPACITY,
            ),
        }
    }

    pub fn hasher(&mut self) -> Option<hmac::Context> {
        let key = self.poll_key()?;
        Some(hmac::Context::with_key(&key))
    }

    fn poll_key(&mut self) -> Option<hmac::Key> {
        let now = s2n_quic_platform::time::now();

        if let Some((expires_at, key)) = self.key.as_ref() {
            if expires_at > &now {
                // key is still valid
                return Some(key.clone());
            }
        }

        let expires_at = now.checked_add(self.active_duration)?;

        // TODO in addition to generating new key material, clear out the filter used for detecting
        // duplicates.
        let mut key_material = [0; digest::SHA256_OUTPUT_LEN];
        SystemRandom::new().fill(&mut key_material[..]).ok()?;
        let key = hmac::Key::new(hmac::HMAC_SHA256, &key_material);

        // TODO clear the filter instead of recreating. This is pending a merge to crates.io
        // (https://github.com/axiomhq/rust-cuckoofilter/pull/52)
        self.duplicate_filter =
            cuckoofilter::CuckooFilter::with_capacity(cuckoofilter::DEFAULT_CAPACITY);

        self.key = Some((expires_at, key));

        self.key.as_ref().map(|key| key.1.clone())
    }
}

const DEFAULT_KEY_ROTATION_PERIOD: Duration = Duration::from_millis(1000);

#[derive(Debug)]
pub struct Provider {
    /// Rotate the key periodically
    key_rotation_period: Duration,
}

impl Default for Provider {
    fn default() -> Self {
        Self {
            key_rotation_period: DEFAULT_KEY_ROTATION_PERIOD,
        }
    }
}

impl super::Provider for Provider {
    type Format = Format;
    type Error = core::convert::Infallible;

    fn start(self) -> Result<Self::Format, Self::Error> {
        // The keys must remain valid for two rotation periods or they will regenerate their
        // material and validation will fail.
        let format = Format {
            key_rotation_period: self.key_rotation_period,
            current_key_rotates_at: s2n_quic_platform::time::now(),
            current_key: 0,
            keys: [
                BaseKey::new(self.key_rotation_period * 2),
                BaseKey::new(self.key_rotation_period * 2),
            ],
        };

        Ok(format)
    }
}

pub struct Format {
    /// Key validity period
    key_rotation_period: Duration,

    /// Timestamp to rotate current key
    current_key_rotates_at: s2n_quic_core::time::Timestamp,

    /// Which key is used to sign
    current_key: u8,

    /// Key used to sign keys
    keys: [BaseKey; 2],
}

impl Format {
    fn current_key(&mut self) -> u8 {
        let now = s2n_quic_platform::time::now();
        if now > self.current_key_rotates_at {
            // TODO either clear the duplicate filter here, or implement in the BaseKey logic
            self.current_key ^= 1;
            self.current_key_rotates_at = now + self.key_rotation_period;
        }
        self.current_key
    }

    // Retry Tokens need to include the original destination connection id from the transport
    // parameters. This OCID is included in the tag.
    fn tag_retry_token(
        &mut self,
        token: &Token,
        peer_address: &SocketAddress,
        destination_connection_id: &connection::Id,
        original_destination_connection_id: &connection::Id,
    ) -> Option<hmac::Tag> {
        let mut ctx = self.keys[token.header.key_id() as usize].hasher()?;

        ctx.update(&token.nonce);
        ctx.update(&destination_connection_id.as_bytes());
        ctx.update(&original_destination_connection_id.as_bytes());
        match peer_address {
            SocketAddress::IPv4(addr) => ctx.update(addr.as_bytes()),
            SocketAddress::IPv6(addr) => ctx.update(addr.as_bytes()),
        };

        Some(ctx.sign())
    }

    // Using the key id in the token, verify the token
    fn validate_retry_token(
        &mut self,
        peer_address: &SocketAddress,
        destination_connection_id: &connection::Id,
        original_destination_connection_id: &connection::Id,
        token: &Token,
    ) -> Option<()> {
        if self.keys[token.header.key_id() as usize]
            .duplicate_filter
            .contains(token)
        {
            return None;
        }

        let tag = self.tag_retry_token(
            token,
            peer_address,
            destination_connection_id,
            original_destination_connection_id,
        )?;

        if ring::constant_time::verify_slices_are_equal(&token.hmac, &tag.as_ref()).is_ok() {
            match self.keys[token.header.key_id() as usize]
                .duplicate_filter
                .add(token)
            {
                Ok(_) => return Some(()),
                // This error indicates our value was stored, but another value was evicted from
                // the filter. We want to continue to connection in this case.
                Err(cuckoofilter::CuckooError::NotEnoughSpace) => return Some(()),
            }
        }

        None
    }
}

impl super::Format for Format {
    const TOKEN_LEN: usize = size_of::<Token>();

    /// The default provider does not support NEW_TOKEN frame tokens
    fn generate_new_token(
        &mut self,
        _peer_address: &SocketAddress,
        _destination_connection_id: &connection::Id,
        _source_connection_id: &connection::Id,
        _output_buffer: &mut [u8],
    ) -> Option<()> {
        None
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-30.txt#8.1.2
    //# Requiring the server
    //# to provide a different connection ID, along with the
    //# original_destination_connection_id transport parameter defined in
    //# Section 18.2, forces the server to demonstrate that it, or an entity
    //# it cooperates with, received the original Initial packet from the
    //# client.
    fn generate_retry_token(
        &mut self,
        peer_address: &SocketAddress,
        destination_connection_id: &connection::Id,
        original_destination_connection_id: &connection::Id,
        output_buffer: &mut [u8],
    ) -> Option<()> {
        let buffer = DecoderBufferMut::new(output_buffer);
        let (token, _) = buffer
            .decode::<&mut Token>()
            .expect("Provided output buffer did not match TOKEN_LEN");

        let header = Header::new(Source::RetryPacket, self.current_key());

        token.header = header;

        // Populate the nonce before signing
        SystemRandom::new().fill(&mut token.nonce[..]).ok()?;

        let tag = self.tag_retry_token(
            token,
            peer_address,
            destination_connection_id,
            original_destination_connection_id,
        )?;

        token.hmac.copy_from_slice(tag.as_ref());

        Some(())
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#8.1.3
    //# When a server receives an Initial packet with an address validation
    //# token, it MUST attempt to validate the token, unless it has already
    //# completed address validation.
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
            Source::NewTokenFrame => None, // Not supported in the default provider
        }
    }
}

#[derive(Clone, Copy, Debug, FromBytes, AsBytes, Unaligned)]
#[repr(C)]
pub(crate) struct Header(u8);

const TOKEN_VERSION: u8 = 0x00;

const VERSION_SHIFT: u8 = 7;
const VERSION_MASK: u8 = 0x80;

const TOKEN_SOURCE_SHIFT: u8 = 6;
const TOKEN_SOURCE_MASK: u8 = 0x40;

const KEY_ID_SHIFT: u8 = 5;
const KEY_ID_MASK: u8 = 0x20;

impl Header {
    fn new(source: Source, key_id: u8) -> Header {
        let mut header: u8 = 0;
        header |= TOKEN_VERSION << VERSION_SHIFT;
        header |= match source {
            Source::NewTokenFrame => 0 << TOKEN_SOURCE_SHIFT,
            Source::RetryPacket => 1 << TOKEN_SOURCE_SHIFT,
        };

        // The key_id can only be 0 or 1
        debug_assert!(key_id <= 1);
        header |= (key_id & 0x01) << KEY_ID_SHIFT;

        Header(header)
    }

    fn version(&self) -> u8 {
        (self.0 & VERSION_MASK) >> VERSION_SHIFT
    }

    fn key_id(&self) -> u8 {
        (self.0 & KEY_ID_MASK) >> KEY_ID_SHIFT
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#8.1.1
    //# A token sent in a NEW_TOKEN frames or a Retry packet MUST be
    //# constructed in a way that allows the server to identify how it was
    //# provided to a client.  These tokens are carried in the same field,
    //# but require different handling from servers.
    fn token_source(&self) -> Source {
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
struct Token {
    header: Header,

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#8.1.4
    //# An address validation token MUST be difficult to guess.  Including a
    //# large enough random value in the token would be sufficient, but this
    //# depends on the server remembering the value it sends to clients.
    nonce: [u8; 32],

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#8.1.4
    //# A token-based scheme allows the server to offload any state
    //# associated with validation to the client.  For this design to work,
    //# the token MUST be covered by integrity protection against
    //# modification or falsification by clients.  Without integrity
    //# protection, malicious clients could generate or guess values for
    //# tokens that would be accepted by the server.  Only the server
    //# requires access to the integrity protection key for tokens.
    hmac: [u8; 32],
}

s2n_codec::zerocopy_value_codec!(Token);

impl Hash for Token {
    /// Token hashes are taken from the hmac
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write(&self.hmac);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use s2n_quic_core::token::{Format as FormatTrait, Source};
    use s2n_quic_platform::time;
    use std::sync::Arc;

    const TEST_KEY_ROTATION_PERIOD: Duration = Duration::from_millis(1000);

    #[test]
    fn test_header() {
        // Test all combinations of values to create a header and verify the header returns the
        // expected values.
        for source in &[Source::NewTokenFrame, Source::RetryPacket] {
            for key_id in [0, 1].iter().cloned() {
                let header = Header::new(*source, key_id);
                // The version should always be the constant TOKEN_VERSION
                assert_eq!(header.version(), TOKEN_VERSION);
                assert_eq!(header.token_source(), *source);
                assert_eq!(header.key_id(), key_id);
            }
        }
    }

    #[test]
    fn test_valid_retry_tokens() {
        let clock = Arc::new(time::testing::MockClock::new());
        time::testing::set_local_clock(clock.clone());

        let mut format = Format {
            key_rotation_period: TEST_KEY_ROTATION_PERIOD,
            keys: [
                BaseKey::new(TEST_KEY_ROTATION_PERIOD * 2),
                BaseKey::new(TEST_KEY_ROTATION_PERIOD * 2),
            ],
            current_key_rotates_at: time::now(),
            current_key: 0,
        };

        let dest_conn_id = connection::Id::EMPTY;
        let orig_conn_id = connection::Id::try_from_bytes(&[0, 1, 2, 3, 4, 5, 6, 7]).unwrap();
        let addr = SocketAddress::default();
        let mut first_token = [0; Format::TOKEN_LEN];
        let mut second_token = [0; Format::TOKEN_LEN];

        // Generate two tokens for different connections
        format
            .generate_retry_token(&addr, &dest_conn_id, &orig_conn_id, &mut first_token)
            .unwrap();

        format
            .generate_retry_token(&addr, &orig_conn_id, &dest_conn_id, &mut second_token)
            .unwrap();

        // Both tokens should pass validation
        clock.adjust_by(TEST_KEY_ROTATION_PERIOD);
        assert!(format
            .validate_token(&addr, &dest_conn_id, &orig_conn_id, &first_token)
            .is_some());
        assert!(format
            .validate_token(&addr, &orig_conn_id, &dest_conn_id, &second_token)
            .is_some());
    }

    #[test]
    fn test_key_rotation() {
        let clock = Arc::new(time::testing::MockClock::new());
        time::testing::set_local_clock(clock.clone());

        let mut format = Format {
            key_rotation_period: TEST_KEY_ROTATION_PERIOD,
            keys: [
                BaseKey::new(TEST_KEY_ROTATION_PERIOD * 2),
                BaseKey::new(TEST_KEY_ROTATION_PERIOD * 2),
            ],
            current_key_rotates_at: time::now(),
            current_key: 0,
        };

        let conn_id = connection::Id::EMPTY;
        let addr = SocketAddress::default();
        let mut buf = [0; Format::TOKEN_LEN];
        format
            .generate_retry_token(&addr, &conn_id, &conn_id, &mut buf)
            .unwrap();

        // Validation should succeed because the signing key is still valid, even
        // though it has been rotated from the current signing key
        clock.adjust_by(TEST_KEY_ROTATION_PERIOD);
        assert!(format
            .validate_token(&addr, &conn_id, &conn_id, &buf)
            .is_some());

        // Validation should fail because the key used for signing has been regenerated
        clock.adjust_by(TEST_KEY_ROTATION_PERIOD);
        assert!(format
            .validate_token(&addr, &conn_id, &conn_id, &buf)
            .is_none());
    }

    #[test]
    fn test_expired_retry_token() {
        let clock = Arc::new(time::testing::MockClock::new());
        time::testing::set_local_clock(clock.clone());

        let mut format = Format {
            key_rotation_period: TEST_KEY_ROTATION_PERIOD,
            keys: [
                BaseKey::new(TEST_KEY_ROTATION_PERIOD * 2),
                BaseKey::new(TEST_KEY_ROTATION_PERIOD * 2),
            ],
            current_key_rotates_at: time::now(),
            current_key: 0,
        };

        let conn_id = connection::Id::EMPTY;
        let addr = SocketAddress::default();
        let mut buf = [0; Format::TOKEN_LEN];
        format
            .generate_retry_token(&addr, &conn_id, &conn_id, &mut buf)
            .unwrap();

        // Validation should fail because multiple rotation periods have elapsed
        clock.adjust_by(TEST_KEY_ROTATION_PERIOD * 2);
        assert!(format
            .validate_token(&addr, &conn_id, &conn_id, &buf)
            .is_none());
    }

    #[test]
    fn test_retry_validation_default_format() {
        let clock = Arc::new(time::testing::MockClock::new());
        time::testing::set_local_clock(clock);

        let mut format = Format {
            key_rotation_period: TEST_KEY_ROTATION_PERIOD,
            keys: [
                BaseKey::new(TEST_KEY_ROTATION_PERIOD * 2),
                BaseKey::new(TEST_KEY_ROTATION_PERIOD * 2),
            ],
            current_key_rotates_at: time::now(),
            current_key: 0,
        };

        let conn_id = connection::Id::EMPTY;
        let addr = SocketAddress::default();
        let mut buf = [0; Format::TOKEN_LEN];
        format
            .generate_retry_token(&addr, &conn_id, &conn_id, &mut buf)
            .unwrap();

        assert_eq!(
            format.validate_token(&addr, &conn_id, &conn_id, &buf),
            Some(Source::RetryPacket)
        );

        let wrong_conn_id = connection::Id::try_from_bytes(&[0, 1, 2]).unwrap();
        assert!(format
            .validate_token(&addr, &wrong_conn_id, &conn_id, &buf)
            .is_none());
    }

    #[test]
    fn test_duplicate_token_detection() {
        let mut format = Format {
            key_rotation_period: TEST_KEY_ROTATION_PERIOD,
            keys: [
                BaseKey::new(TEST_KEY_ROTATION_PERIOD * 2),
                BaseKey::new(TEST_KEY_ROTATION_PERIOD * 2),
            ],
            current_key_rotates_at: time::now(),
            current_key: 0,
        };

        let conn_id = connection::Id::EMPTY;
        let addr = SocketAddress::default();
        let mut buf = [0; Format::TOKEN_LEN];
        format
            .generate_retry_token(&addr, &conn_id, &conn_id, &mut buf)
            .unwrap();

        assert_eq!(
            format.validate_token(&addr, &conn_id, &conn_id, &buf),
            Some(Source::RetryPacket)
        );

        // Second attempt with the same token should fail because the token is a duplicate
        assert!(format
            .validate_token(&addr, &conn_id, &conn_id, &buf)
            .is_none());
    }
}

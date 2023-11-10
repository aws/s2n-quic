// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Default provider for Address Validation tokens
//!
//! The default provider will randomly generate a 256 bit key. This key will be used to sign and
//! verify tokens. The key can be rotated at a duration set by the user.
//!
//! The default provider does not support tokens delivered in a NEW_TOKEN frame.

use core::{mem::size_of, time::Duration};
use hash_hasher::HashHasher;
use s2n_codec::{DecoderBuffer, DecoderBufferMut};
use s2n_quic_core::{
    connection, event::api::SocketAddress, random, time::Timestamp, token::Source,
};
use s2n_quic_crypto::{constant_time, digest, hmac};
use std::hash::{Hash, Hasher};
use zerocopy::{AsBytes, FromBytes, FromZeroes, Unaligned};
use zeroize::Zeroizing;

struct BaseKey {
    active_duration: Duration,

    // HMAC key for signing and verifying
    key: Option<(Timestamp, hmac::Key)>,

    //= https://www.rfc-editor.org/rfc/rfc9000#section-8.1.4
    //# To protect against such attacks, servers MUST ensure that
    //# replay of tokens is prevented or limited.
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

    pub fn hasher(&mut self, random: &mut dyn random::Generator) -> Option<hmac::Context> {
        let key = self.poll_key(random)?;
        Some(hmac::Context::with_key(&key))
    }

    fn poll_key(&mut self, random: &mut dyn random::Generator) -> Option<hmac::Key> {
        let now = s2n_quic_platform::time::now();

        //= https://www.rfc-editor.org/rfc/rfc9000#section-21.3
        //# Servers SHOULD provide mitigations for this attack by limiting the
        //# usage and lifetime of address validation tokens; see Section 8.1.3.
        if let Some((expires_at, key)) = self.key.as_ref() {
            if expires_at > &now {
                // key is still valid
                return Some(key.clone());
            }
        }

        let expires_at = now.checked_add(self.active_duration)?;

        // TODO in addition to generating new key material, clear out the filter used for detecting
        // duplicates.
        let mut key_material = Zeroizing::new([0; digest::SHA256_OUTPUT_LEN]);
        random.private_random_fill(&mut key_material[..]);
        let key = hmac::Key::new(hmac::HMAC_SHA256, key_material.as_ref());

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
    //= https://www.rfc-editor.org/rfc/rfc9000#section-8.1.3
    //# Thus, a token SHOULD have an
    //# expiration time, which could be either an explicit expiration time or
    //# an issued timestamp that can be used to dynamically calculate the
    //# expiration time.
    /// To fulfill this SHOULD, we rotate the key periodically. This allows
    /// customers to control the token lifetime without adding bytes to the token itself.
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
    //= https://www.rfc-editor.org/rfc/rfc9000#section-8.1.4
    //= type=exception
    //= reason=We use a duplicate filter to prevent tokens from being used more than once.
    //# Servers are encouraged to allow tokens to be used only
    //# once, if possible; tokens MAY include additional information about
    //# clients to further narrow applicability or reuse.
    /// Key validity period
    key_rotation_period: Duration,

    //= https://www.rfc-editor.org/rfc/rfc9000#section-8.1.4
    //# Servers SHOULD ensure that
    //# tokens sent in Retry packets are only accepted for a short time.
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
            self.current_key ^= 1;
            self.current_key_rotates_at = now + self.key_rotation_period;

            // TODO either clear the duplicate filter here, or implement in the BaseKey logic
            // https://github.com/aws/s2n-quic/issues/173
        }
        self.current_key
    }

    // Retry Tokens need to include the original destination connection id from the transport
    // parameters. This OCID is included in the tag.
    fn tag_retry_token(
        &mut self,
        token: &Token,
        context: &mut super::Context<'_>,
    ) -> Option<hmac::Tag> {
        let mut ctx = self.keys[token.header.key_id() as usize].hasher(context.random)?;

        //= https://www.rfc-editor.org/rfc/rfc9000#section-8.1.4
        //# Tokens
        //# sent in Retry packets SHOULD include information that allows the
        //# server to verify that the source IP address and port in client
        //# packets remain constant.
        ctx.update(&token.original_destination_connection_id);
        ctx.update(&token.nonce);
        ctx.update(context.peer_connection_id);
        match context.remote_address {
            SocketAddress::IpV4 { ip, port, .. } => {
                ctx.update(ip);
                ctx.update(&port.to_be_bytes());
            }
            SocketAddress::IpV6 { ip, port, .. } => {
                ctx.update(ip);
                ctx.update(&port.to_be_bytes());
            }
            _ => {
                // we are unable to hash the address so bail
                return None;
            }
        };

        Some(ctx.sign())
    }

    // Using the key id in the token, verify the token
    fn validate_retry_token(
        &mut self,
        context: &mut super::Context<'_>,
        token: &Token,
    ) -> Option<connection::InitialId> {
        if self.keys[token.header.key_id() as usize]
            .duplicate_filter
            .contains(token)
        {
            return None;
        }

        let tag = self.tag_retry_token(token, context)?;

        if constant_time::verify_slices_are_equal(&token.hmac, tag.as_ref()).is_ok() {
            // Only add the token once it has been validated. This will prevent the filter from
            // being filled with garbage tokens.

            // Ignore the outcome of adding a token to the filter because we always want to
            // continue the connection if the filter fails.
            let _ = self.keys[token.header.key_id() as usize]
                .duplicate_filter
                .add(token);

            return token.original_destination_connection_id();
        }

        None
    }
}

impl super::Format for Format {
    const TOKEN_LEN: usize = size_of::<Token>();

    /// The default provider does not support NEW_TOKEN frame tokens
    fn generate_new_token(
        &mut self,
        _context: &mut super::Context<'_>,
        _source_connection_id: &connection::LocalId,
        _output_buffer: &mut [u8],
    ) -> Option<()> {
        //= https://www.rfc-editor.org/rfc/rfc9000#section-8.1.3
        //= type=TODO
        //= tracking-issue=418
        //# A server MAY provide clients with an address validation token during
        //# one connection that can be used on a subsequent connection.

        //= https://www.rfc-editor.org/rfc/rfc9000#section-8.1.4
        //= type=TODO
        //= tracking-issue=346
        //# Tokens sent in NEW_TOKEN frames MUST include information that allows
        //# the server to verify that the client IP address has not changed from
        //# when the token was issued.

        //= https://www.rfc-editor.org/rfc/rfc9000#section-8.1.3
        //= type=TODO
        //= tracking-issue=345
        //# A token issued with NEW_TOKEN MUST NOT include information that would
        //# allow values to be linked by an observer to the connection on which
        //# it was issued.

        //= https://www.rfc-editor.org/rfc/rfc9000#section-8.1.3
        //= type=TODO
        //= tracking-issue=387
        //# A server MUST ensure that every NEW_TOKEN frame it sends
        //# is unique across all clients, with the exception of those sent to
        //# repair losses of previously sent NEW_TOKEN frames.

        //= https://www.rfc-editor.org/rfc/rfc9000#section-8.1.3
        //= type=TODO
        //= tracking-issue=394
        //# A server MAY provide clients with an address validation token during
        //# one connection that can be used on a subsequent connection.

        None
    }

    //= https://www.rfc-editor.org/rfc/rfc9000#section-8.1.2
    //# Requiring the server
    //# to provide a different connection ID, along with the
    //# original_destination_connection_id transport parameter defined in
    //# Section 18.2, forces the server to demonstrate that it, or an entity
    //# it cooperates with, received the original Initial packet from the
    //# client.
    fn generate_retry_token(
        &mut self,
        context: &mut super::Context<'_>,
        original_destination_connection_id: &connection::InitialId,
        output_buffer: &mut [u8],
    ) -> Option<()> {
        let buffer = DecoderBufferMut::new(output_buffer);
        let (token, _) = buffer
            .decode::<&mut Token>()
            .expect("Provided output buffer did not match TOKEN_LEN");

        let header = Header::new(Source::RetryPacket, self.current_key());

        token.header = header;
        token.original_destination_connection_id[..original_destination_connection_id.len()]
            .copy_from_slice(original_destination_connection_id.as_bytes());
        token.odcid_len = original_destination_connection_id.len() as u8;

        // ensure the other CID bytes are zeroed out
        for b in token
            .original_destination_connection_id
            .iter_mut()
            .skip(original_destination_connection_id.len())
        {
            *b = 0;
        }

        // Populate the nonce before signing
        context.random.public_random_fill(&mut token.nonce[..]);

        let tag = self.tag_retry_token(token, context)?;

        token.hmac.copy_from_slice(tag.as_ref());

        Some(())
    }

    //= https://www.rfc-editor.org/rfc/rfc9000#section-8.1.3
    //# When a server receives an Initial packet with an address validation
    //# token, it MUST attempt to validate the token, unless it has already
    //# completed address validation.
    fn validate_token(
        &mut self,
        context: &mut super::Context<'_>,
        token: &[u8],
    ) -> Option<connection::InitialId> {
        let buffer = DecoderBuffer::new(token);
        let (token, remaining) = buffer.decode::<&Token>().ok()?;

        // Verify the provided token doesn't have any additional data
        remaining.ensure_empty().ok()?;

        if token.header.version() != TOKEN_VERSION {
            return None;
        }

        let source = token.header.token_source();

        match source {
            Source::RetryPacket => self.validate_retry_token(context, token),
            Source::NewTokenFrame => None, // Not supported in the default provider
        }
        //= https://www.rfc-editor.org/rfc/rfc9000#section-8.1.4
        //= type=TODO
        //= tracking-issue=347
        //# Tokens that are provided
        //# in NEW_TOKEN frames (Section 19.7) need to be valid for longer but
        //# SHOULD NOT be accepted multiple times.

        //= https://www.rfc-editor.org/rfc/rfc9000#section-8.1.3
        //= type=TODO
        //= tracking-issue=388
        //# Clients that want to break continuity of identity with a server can
        //# discard tokens provided using the NEW_TOKEN frame.
    }
}

#[derive(Clone, Copy, Debug, FromBytes, FromZeroes, AsBytes, Unaligned)]
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
        //= https://www.rfc-editor.org/rfc/rfc9000#section-8.1.3
        //# Information that
        //# allows the server to distinguish between tokens from Retry and
        //# NEW_TOKEN MAY be accessible to entities other than the server.
        header |= match source {
            Source::NewTokenFrame => 0 << TOKEN_SOURCE_SHIFT,
            Source::RetryPacket => 1 << TOKEN_SOURCE_SHIFT,
        };

        // The key_id can only be 0 or 1
        debug_assert!(key_id <= 1);
        header |= (key_id & 0x01) << KEY_ID_SHIFT;

        Header(header)
    }

    fn version(self) -> u8 {
        (self.0 & VERSION_MASK) >> VERSION_SHIFT
    }

    fn key_id(self) -> u8 {
        (self.0 & KEY_ID_MASK) >> KEY_ID_SHIFT
    }

    //= https://www.rfc-editor.org/rfc/rfc9000#section-8.1.1
    //# A token sent in a NEW_TOKEN frame or a Retry packet MUST be
    //# constructed in a way that allows the server to identify how it was
    //# provided to a client.  These tokens are carried in the same field but
    //# require different handling from servers.
    fn token_source(self) -> Source {
        match (self.0 & TOKEN_SOURCE_MASK) >> TOKEN_SOURCE_SHIFT {
            0 => Source::NewTokenFrame,
            1 => Source::RetryPacket,
            _ => Source::NewTokenFrame,
        }
    }
}

//= https://www.rfc-editor.org/rfc/rfc9000#section-8.1.4
//#   There is no need for a single well-defined format for the token
//#   because the server that generates the token also consumes it.
#[derive(Copy, Clone, Debug, FromBytes, FromZeroes, AsBytes, Unaligned)]
#[repr(C)]
struct Token {
    header: Header,

    odcid_len: u8,
    original_destination_connection_id: [u8; 20],

    //= https://www.rfc-editor.org/rfc/rfc9000#section-8.1.4
    //# An address validation token MUST be difficult to guess.  Including a
    //# random value with at least 128 bits of entropy in the token would be
    //# sufficient, but this depends on the server remembering the value it
    //# sends to clients.
    nonce: [u8; 32],

    //= https://www.rfc-editor.org/rfc/rfc9000#section-8.1.4
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

impl Token {
    pub fn original_destination_connection_id(&self) -> Option<connection::InitialId> {
        let dcid = self
            .original_destination_connection_id
            .get(..self.odcid_len as usize)?;
        connection::InitialId::try_from_bytes(dcid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use s2n_quic_core::{
        inet::SocketAddress,
        random,
        token::{Context, Format as FormatTrait, Source},
    };
    use s2n_quic_platform::time;
    use std::{net::SocketAddr, sync::Arc};

    const TEST_KEY_ROTATION_PERIOD: Duration = Duration::from_millis(1000);

    fn get_test_format() -> Format {
        Format {
            key_rotation_period: TEST_KEY_ROTATION_PERIOD,
            keys: [
                BaseKey::new(TEST_KEY_ROTATION_PERIOD * 2),
                BaseKey::new(TEST_KEY_ROTATION_PERIOD * 2),
            ],
            current_key_rotates_at: time::now(),
            current_key: 0,
        }
    }

    #[test]
    fn test_header() {
        // Test all combinations of values to create a header and verify the header returns the
        // expected values.
        for source in &[Source::NewTokenFrame, Source::RetryPacket] {
            for key_id in [0, 1] {
                let header = Header::new(*source, key_id);
                // The version should always be the constant TOKEN_VERSION
                assert_eq!(header.version(), TOKEN_VERSION);
                //= https://www.rfc-editor.org/rfc/rfc9000#section-8.1.1
                //= type=test
                //# A token sent in a NEW_TOKEN frames or a Retry packet MUST be
                //# constructed in a way that allows the server to identify how it was
                //# provided to a client.

                assert_eq!(header.token_source(), *source);
                assert_eq!(header.key_id(), key_id);
            }
        }
    }

    #[test]
    fn test_valid_retry_tokens() {
        let clock = Arc::new(time::testing::MockClock::new());
        time::testing::set_local_clock(clock.clone());

        let mut format = get_test_format();
        let first_conn_id = connection::PeerId::try_from_bytes(&[2, 4, 6, 8, 10]).unwrap();
        let second_conn_id = connection::PeerId::try_from_bytes(&[1, 3, 5, 7, 9]).unwrap();
        let orig_conn_id =
            connection::InitialId::try_from_bytes(&[0, 1, 2, 3, 4, 5, 6, 7]).unwrap();
        let addr = SocketAddress::default();
        let mut first_token = [0; Format::TOKEN_LEN];
        let mut second_token = [0; Format::TOKEN_LEN];
        let mut random = random::testing::Generator(5);
        let mut context = Context::new(&addr, &first_conn_id, &mut random);

        // Generate two tokens for different connections
        format
            .generate_retry_token(&mut context, &orig_conn_id, &mut first_token)
            .unwrap();

        context = Context::new(&addr, &second_conn_id, &mut random);
        format
            .generate_retry_token(&mut context, &orig_conn_id, &mut second_token)
            .unwrap();

        clock.adjust_by(TEST_KEY_ROTATION_PERIOD);
        context = Context::new(&addr, &first_conn_id, &mut random);
        assert_eq!(
            format.validate_token(&mut context, &first_token),
            Some(orig_conn_id)
        );
        context = Context::new(&addr, &second_conn_id, &mut random);
        assert_eq!(
            format.validate_token(&mut context, &second_token),
            Some(orig_conn_id)
        );
        context = Context::new(&addr, &first_conn_id, &mut random);
        assert_eq!(format.validate_token(&mut context, &second_token), None);
    }

    #[test]
    fn test_retry_ip_port_validation() {
        //= https://www.rfc-editor.org/rfc/rfc9000#section-8.1.4
        //= type=test
        //# Tokens
        //# sent in Retry packets SHOULD include information that allows the
        //# server to verify that the source IP address and port in client
        //# packets remain constant.
        let mut format = get_test_format();
        let conn_id = connection::PeerId::try_from_bytes(&[2, 4, 6, 8, 10]).unwrap();
        let orig_conn_id =
            connection::InitialId::try_from_bytes(&[0, 1, 2, 3, 4, 5, 6, 7]).unwrap();

        let mut token = [0; Format::TOKEN_LEN];
        let ip_address = "127.0.0.1:443";
        let addr: SocketAddr = ip_address.parse().unwrap();
        let correct_address: SocketAddress = addr.into();
        let mut random = random::testing::Generator(5);
        let mut context = Context::new(&correct_address, &conn_id, &mut random);
        format
            .generate_retry_token(&mut context, &orig_conn_id, &mut token)
            .unwrap();

        let ip_address = "127.0.0.2:443";
        let addr: SocketAddr = ip_address.parse().unwrap();
        let incorrect_address: SocketAddress = addr.into();
        context = Context::new(&incorrect_address, &conn_id, &mut random);
        assert_eq!(format.validate_token(&mut context, &token), None);

        let ip_address = "127.0.0.1:444";
        let addr: SocketAddr = ip_address.parse().unwrap();
        let incorrect_port: SocketAddress = addr.into();
        context = Context::new(&incorrect_port, &conn_id, &mut random);
        assert_eq!(format.validate_token(&mut context, &token), None);

        // Verify the token is still valid after the failed attempts
        context = Context::new(&correct_address, &conn_id, &mut random);
        assert!(format.validate_token(&mut context, &token).is_some());
    }

    #[test]
    fn test_key_rotation() {
        //= https://www.rfc-editor.org/rfc/rfc9000#section-8.1.3
        //= type=test
        //# Thus, a token SHOULD have an
        //# expiration time, which could be either an explicit expiration time or
        //# an issued timestamp that can be used to dynamically calculate the
        //# expiration time.
        let clock = Arc::new(time::testing::MockClock::new());
        time::testing::set_local_clock(clock.clone());

        let mut format = get_test_format();
        let conn_id = connection::PeerId::TEST_ID;
        let orig_conn_id = connection::InitialId::TEST_ID;
        let addr = SocketAddress::default();
        let mut buf = [0; Format::TOKEN_LEN];
        let mut random = random::testing::Generator(5);
        let mut context = Context::new(&addr, &conn_id, &mut random);
        format
            .generate_retry_token(&mut context, &orig_conn_id, &mut buf)
            .unwrap();

        // Validation should succeed because the signing key is still valid, even
        // though it has been rotated from the current signing key
        clock.adjust_by(TEST_KEY_ROTATION_PERIOD);
        assert!(format.validate_token(&mut context, &buf).is_some());

        // Validation should fail because the key used for signing has been regenerated
        clock.adjust_by(TEST_KEY_ROTATION_PERIOD);
        assert!(format.validate_token(&mut context, &buf).is_none());
    }

    #[test]
    fn test_expired_retry_token() {
        //= https://www.rfc-editor.org/rfc/rfc9000#section-8.1.4
        //= type=test
        //# Servers SHOULD ensure that
        //# tokens sent in Retry packets are only accepted for a short time.
        let clock = Arc::new(time::testing::MockClock::new());
        time::testing::set_local_clock(clock.clone());

        let mut format = get_test_format();
        let conn_id = connection::PeerId::TEST_ID;
        let orig_conn_id = connection::InitialId::TEST_ID;
        let addr = SocketAddress::default();
        let mut buf = [0; Format::TOKEN_LEN];
        let mut random = random::testing::Generator(5);
        let mut context = Context::new(&addr, &conn_id, &mut random);
        format
            .generate_retry_token(&mut context, &orig_conn_id, &mut buf)
            .unwrap();

        //= https://www.rfc-editor.org/rfc/rfc9000#section-21.3
        //= type=test
        //# Servers SHOULD provide mitigations for this attack by limiting the
        //# usage and lifetime of address validation tokens; see Section 8.1.3.
        // Validation should fail because multiple rotation periods have elapsed
        clock.adjust_by(TEST_KEY_ROTATION_PERIOD * 2);
        assert!(format.validate_token(&mut context, &buf).is_none());
    }

    #[test]
    fn test_retry_validation_default_format() {
        let clock = Arc::new(time::testing::MockClock::new());
        time::testing::set_local_clock(clock);

        let mut format = get_test_format();
        let conn_id = connection::PeerId::TEST_ID;
        let odcid = connection::InitialId::try_from_bytes(&[0, 1, 2, 3, 4, 5, 6, 7]).unwrap();
        let addr = SocketAddress::default();
        let mut buf = [0; Format::TOKEN_LEN];
        let mut random = random::testing::Generator(5);
        let mut context = Context::new(&addr, &conn_id, &mut random);
        format
            .generate_retry_token(&mut context, &odcid, &mut buf)
            .unwrap();

        assert_eq!(format.validate_token(&mut context, &buf), Some(odcid));

        let wrong_conn_id = connection::PeerId::try_from_bytes(&[0, 1, 2]).unwrap();
        context = Context::new(&addr, &wrong_conn_id, &mut random);
        assert!(format.validate_token(&mut context, &buf).is_none());
    }

    #[test]
    fn test_duplicate_token_detection() {
        //= https://www.rfc-editor.org/rfc/rfc9000#section-8.1.4
        //= type=test
        //# To protect against such attacks, servers MUST ensure that
        //# replay of tokens is prevented or limited.
        let mut format = get_test_format();
        let conn_id = connection::PeerId::TEST_ID;
        let odcid = connection::InitialId::try_from_bytes(&[0, 1, 2, 3, 4, 5, 6, 7]).unwrap();
        let addr = SocketAddress::default();
        let mut buf = [0; Format::TOKEN_LEN];
        let mut random = random::testing::Generator(5);
        let mut context = Context::new(&addr, &conn_id, &mut random);
        format
            .generate_retry_token(&mut context, &odcid, &mut buf)
            .unwrap();

        assert_eq!(format.validate_token(&mut context, &buf), Some(odcid));

        // Second attempt with the same token should fail because the token is a duplicate
        assert!(format.validate_token(&mut context, &buf).is_none());
    }

    #[test]
    fn test_token_modification_detection() {
        //= https://www.rfc-editor.org/rfc/rfc9000#section-8.1.4
        //= type=test
        //# For this design to work,
        //# the token MUST be covered by integrity protection against
        //# modification or falsification by clients.
        let mut format = get_test_format();
        let conn_id = connection::PeerId::try_from_bytes(&[2, 4, 6, 8, 10]).unwrap();
        let orig_conn_id =
            connection::InitialId::try_from_bytes(&[0, 1, 2, 3, 4, 5, 6, 7]).unwrap();
        let addr = SocketAddress::default();
        let mut token = [0; Format::TOKEN_LEN];

        let mut random = random::testing::Generator(5);
        let mut context = Context::new(&addr, &conn_id, &mut random);
        // Generate two tokens for different connections
        format
            .generate_retry_token(&mut context, &orig_conn_id, &mut token)
            .unwrap();

        for i in 0..Format::TOKEN_LEN {
            random = random::testing::Generator(5);
            context = Context::new(&addr, &conn_id, &mut random);
            token[i] = !token[i];
            assert!(format.validate_token(&mut context, &token).is_none());
            token[i] = !token[i];
        }
    }

    #[test]
    fn test_token_length_check() {
        let mut format = get_test_format();
        let conn_id = connection::PeerId::try_from_bytes(&[2, 4, 6, 8, 10]).unwrap();
        let addr = SocketAddress::default();

        bolero::check!().for_each(move |token| {
            let mut random = random::testing::Generator(5);
            let mut context = Context::new(&addr, &conn_id, &mut random);
            assert!(format.validate_token(&mut context, token).is_none())
        });
    }

    #[test]
    fn test_token_falsification_detection() {
        let mut format = get_test_format();
        let conn_id = connection::PeerId::try_from_bytes(&[2, 4, 6, 8, 10]).unwrap();
        let addr = SocketAddress::default();

        //= https://www.rfc-editor.org/rfc/rfc9000#section-8.1.4
        //= type=test
        //# For this design to work,
        //# the token MUST be covered by integrity protection against
        //# modification or falsification by clients.
        let generator = bolero::generator::gen::<Vec<u8>>()
            .with()
            .len(Format::TOKEN_LEN);
        bolero::check!()
            .with_generator(generator)
            .for_each(move |token| {
                let mut random = random::testing::Generator(5);
                let mut context = Context::new(&addr, &conn_id, &mut random);
                assert!(format.validate_token(&mut context, token).is_none())
            });
    }
}

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
//! +----------+------------+--------+----------------+
//! |  Version | Token Source | Key ID | Time Window ID |
//! +----------+------------+--------+----------------+
//!      1           1          2           4
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
    // key_updater: Box<dyn KeyUpdater>,
}

pub trait KeyUpdater: 'static + Send {
    fn update(&mut self, key: &[u8]);
}

impl super::Provider for Provider {
    type Format = Format;
    type Error = core::convert::Infallible;

    fn start(self) -> Result<Self::Format, Self::Error> {
        Ok(Format {
            new_tokens: self.new_tokens,
            new_token_validate_port: self.new_token_validate_port,
            retry_tokens: self.retry_tokens,
        })
    }
}

#[derive(Debug, Default)]
pub struct Format {
    new_tokens: bool,
    new_token_validate_port: bool,
    retry_tokens: bool,
}

impl Format {
    fn generate_token(
        &mut self,
        source: Source,
        peer_address: &SocketAddress,
        destination_connection_id: &connection::Id,
        source_connection_id: &connection::Id,
        output_buffer: &mut [u8],
    ) -> Option<Duration> {
        let buffer = DecoderBufferMut::new(output_buffer);
        let (token, _) = buffer
            .decode::<&mut Token>()
            .expect("Provided output buffer did not match TOKEN_LEN");

        // TODO
        let current_key_id = 0;
        let current_time_window_id = 0;

        token.header.set_version(TOKEN_VERSION);
        token.header.set_key_id(current_key_id);
        token.header.set_time_window_id(current_time_window_id);
        token.header.set_token_source(source);

        SystemRandom::new().fill(&mut token.nonce[..]).ok()?;

        todo!()
    }

    fn validate_retry_token(
        &mut self,
        peer_address: &SocketAddress,
        destination_connection_id: &connection::Id,
        source_connection_id: &connection::Id,
        token: &Token,
    ) -> Option<()> {
        todo!()
    }

    fn validate_new_token(&mut self, peer_address: &SocketAddress, token: &Token) -> Option<()> {
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

    fn hash_token(&self, _token: &[u8]) -> &[u8] {
        &[0; 32]
    }
}

const KEY_SPACE: usize = 4;
const TIME_WINDOW_SPACE: usize = 16;

struct KeyStore {
    current_key_id: u8,
    current_time_window_id: u8,
    keys: [Key; KEY_SPACE],
    derived_keys: [DerivedKey; KEY_SPACE * TIME_WINDOW_SPACE],
}

#[derive(Clone, Copy, Debug, FromBytes, AsBytes, Unaligned)]
#[repr(C)]
pub struct Header(u8);

pub const TOKEN_VERSION: u8 = 0x01;

const VERSION_MASK: u8 = 0x03;
const MKID_MASK: u8 = 0x0c;
const KID_MASK: u8 = 0xf0;
const TOKEN_TYPE_MASK: u8 = 0x80;

const VERSION_SHIFT: u8 = 7;
const MKID_SHIFT: u8 = 2;
const KID_SHIFT: u8 = 4;
const TOKEN_TYPE_SHIFT: u8 = 7;

impl Header {
    pub fn version(&self) -> u8 {
        todo!()
    }

    pub fn set_version(&mut self, version: u8) {
        todo!()
    }

    pub fn key_id(&self) -> u8 {
        todo!()
    }

    pub fn set_key_id(&mut self, key_id: u8) {
        todo!()
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#21.2
    //#   An attacker might be able to receive an address validation token
    //#   (Section 8) from a server and then release the IP address it used to
    //#   acquire that token.
    //#   Servers SHOULD provide mitigations for this attack by limiting the
    //#   usage and lifetime of address validation tokens
    pub fn time_window_id(&self) -> u8 {
        todo!()
    }

    pub fn set_time_window_id(&mut self, id: u8) {
        todo!()
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#8.1.1
    //#   A token sent in a NEW_TOKEN frames or a Retry packet MUST be
    //#   constructed in a way that allows the server to identify how it was
    //#   provided to a client.  These tokens are carried in the same field,
    //#   but require different handling from servers.
    pub fn token_source(&self) -> Source {
        todo!()
    }

    pub fn set_token_source(&mut self, source: Source) {
        todo!()
    }
}

//= https://tools.ietf.org/html/draft-ietf-quic-transport-29.txt#8.1.4
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
    #[allow(dead_code)]
    pub fn validate(&self) -> bool {
        true
    }
}

use std::time::SystemTime;

pub type Secret = [u8; 32];

#[derive(Debug)]
struct Key {
    /// The epoch from which time windows are derived
    epoch: SystemTime,
    /// The time window in seconds for which the key id is valid
    time_window: u64,
    start_time: SystemTime,
    active_duration: Duration,
    valid_duration: Duration,
    secret: Secret,
}

struct DerivedKey {
    header: Header,
    key: ring::hmac::Key,
    is_valid: bool,
}

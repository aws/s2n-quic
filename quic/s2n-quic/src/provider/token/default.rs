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
    // key_updater: Box<dyn KeyUpdater>,
}

pub trait KeyUpdater: 'static + Send {
    fn update(&mut self, key: &[u8]);
}

impl super::Provider for Provider {
    type Format = Format;
    type Error = core::convert::Infallible;

    fn start(self) -> Result<Self::Format, Self::Error> {
        // Start timer to update key
        Ok(Format {
            new_tokens: self.new_tokens,
            new_token_validate_port: self.new_token_validate_port,
            retry_tokens: self.retry_tokens,
        })
    }
}

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

        token.header.set_version(TOKEN_VERSION);
        token.header.set_key_id(current_key_id);
        token.header.set_time_window_id(current_time_window_id);
        token.header.set_token_source(source);

        SystemRandom::new().fill(&mut token.nonce[..]).ok()?;

        // Sign the token, then write to the buffer
        todo!()
    }

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

impl super::FormatTrait for Format {
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
        todo!()
    }
}

#[allow(dead_code)]
const KEY_SPACE: usize = 4;

#[allow(dead_code)]
const TIME_WINDOW_SPACE: usize = 16;

// The KeyStore distributes derived keys to callers. The callers are responsible for verifying the
// validity of those keys.
#[allow(dead_code)]
struct KeyStore {
    // current_key_id: u8,
    current_time_window_id: u8,
    // keys: [Key; KEY_SPACE],
    key: Key,
    derived_keys: [DerivedKey; TIME_WINDOW_SPACE],
}

impl KeyStore {
    /// Generate the primary key used to derive time window keys
    fn generate_key(&mut self) {
        todo!();
    }

    /// Returns derived key for a time window
    fn key(&self, time_window_id: u8) -> &DerivedKey {
        &self.derived_keys[time_window_id as usize]
    }

    /// Derives a new set of keys given a Key and key id
    fn set_key(&mut self, _derived_key: &DerivedKey, _key_id: u8, _time_window_id: u8) {
        todo!();
    }
}

#[derive(Clone, Copy, Debug, FromBytes, AsBytes, Unaligned)]
#[repr(C)]
pub struct Header(u8);

pub const TOKEN_VERSION: u8 = 0x00;

const VERSION_MASK: u8 = 0x80;
const TOKEN_SOURCE_MASK: u8 = 0x40;
const KID_MASK: u8 = 0x30;
const TIME_WINDOW_MASK: u8 = 0x0f;

const VERSION_SHIFT: u8 = 7;
const TOKEN_SOURCE_SHIFT: u8 = 6;
const KID_SHIFT: u8 = 4;
const TIME_WINDOW_SHIFT: u8 = 0;

impl Header {
    pub fn version(&self) -> u8 {
        (self.0 & VERSION_MASK) >> VERSION_SHIFT
    }

    pub fn set_version(&mut self, version: u8) {
        self.0 |= version << VERSION_SHIFT
    }

    // Version is not settable. It may be required to handle multiple versions in-flight, but only
    // the latest version should be used when generating headers.

    pub fn key_id(&self) -> u8 {
        (self.0 & KID_MASK) >> KID_SHIFT
    }

    pub fn set_key_id(&mut self, key_id: u8) {
        self.0 |= key_id << KID_SHIFT
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#21.2
    //#   An attacker might be able to receive an address validation token
    //#   (Section 8) from a server and then release the IP address it used to
    //#   acquire that token.
    //#   Servers SHOULD provide mitigations for this attack by limiting the
    //#   usage and lifetime of address validation tokens
    pub fn time_window_id(&self) -> u8 {
        (self.0 & TIME_WINDOW_MASK) >> TIME_WINDOW_SHIFT
    }

    pub fn set_time_window_id(&mut self, id: u8) {
        self.0 |= id << TIME_WINDOW_SHIFT
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#8.1.1
    //#   A token sent in a NEW_TOKEN frames or a Retry packet MUST be
    //#   constructed in a way that allows the server to identify how it was
    //#   provided to a client.  These tokens are carried in the same field,
    //#   but require different handling from servers.
    pub fn token_source(&self) -> Source {
        match (self.0 & TOKEN_SOURCE_MASK) >> TOKEN_SOURCE_SHIFT {
            0 => Source::NewTokenFrame,
            1 => Source::RetryPacket,
            _ => Source::NewTokenFrame,
        }
    }

    pub fn set_token_source(&mut self, source: Source) {
        match source {
            Source::NewTokenFrame => self.0 |= 0 << TOKEN_SOURCE_SHIFT,
            Source::RetryPacket => self.0 |= 1 << TOKEN_SOURCE_SHIFT,
        }
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
    /// Start time
    start_time: SystemTime,
    /// Length of window in which this key is active
    active_duration: Duration,
    /// Total duration this key will be accepted
    valid_duration: Duration,
    /// Key material
    secret: Secret,
}

impl Key {
    /// Generate a new key
    fn generate(valid_duration: Duration) -> Self {
        let rng = SystemRandom::new();
        let secret = ring::rand::generate(&rng).unwrap().expose();
        let now = SystemTime::now();

        Key {
            epoch: now,
            start_time: now,
            active_duration: Duration::from_millis(0),
            valid_duration,
            secret,
        }
    }
}

struct DerivedKeyHeader {
    version: u8,
    key_id: u8,
    time_window_id: u8,
}

struct DerivedKey {
    header: DerivedKeyHeader,
    key: Secret,
    start_time: SystemTime,
    active_duration: Duration,
}

impl DerivedKey {
    /// Using the main key and a time window, derive the key that can be used for this period.
    fn generate(key: Key, time_window_id: u8) -> Self {
        todo!();
    }

    fn sign(&self, data: &[u8]) -> &[u8] {
        todo!();
    }

    fn verify(&self, signature: &[u8], data: &[u8]) -> bool {
        todo!();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use s2n_quic_core::{inet::SocketAddress, token::FormatTrait};

    #[test]
    fn test_header() {
        // Fuzz for an exhaustive test of the header
        let header = Header(0xac);
        assert_eq!(header.version(), 0x01);
        assert_eq!(header.token_source(), Source::NewTokenFrame);
        assert_eq!(header.key_id(), 0x02);
        assert_eq!(header.time_window_id(), 0x0c);

        let mut header = Header(0);
        header.set_key_id(0x03);
        header.set_time_window_id(0x0a);
        header.set_token_source(Source::RetryPacket);

        assert_eq!(header.version(), TOKEN_VERSION);
        assert_eq!(header.key_id(), 0x03);
        assert_eq!(header.time_window_id(), 0x0a);
        assert_eq!(header.token_source(), Source::RetryPacket);
    }

    #[test]
    fn test_token_sign() {
        let conn_id = &connection::Id::try_from_bytes(&[]).unwrap();
        let address = &SocketAddress::default();
        let mut token_buf = [0u8; 128];
        let format = Format::default();
        let token = format.generate_retry_token(address, conn_id, conn_id, &mut token_buf);
        let key = ring::hmac::Key::generate(ring::hmac::HMAC_SHA256, &SystemRandom::new()).unwrap();

        let derived_key = DerivedKey {
            header: Header(0),
            key,
            is_valid: true,
        };

        println!("{:?}", token);
        format.sign_token(&mut token_buf, key);
        println!("{:?}", token);
    }
}

//! Defines the Address Validation token

use crate::inet::{SocketAddressV4, SocketAddressV6, Unspecified};
use core::mem::size_of;
use s2n_codec::{decoder_value, Encoder, EncoderValue};
//use std::convert::TryFrom;

pub trait AddressValidation {
    //= https://tools.ietf.org/html/draft-ietf-quic-transport-29.txt#8.1.3
    //#   When a server receives an Initial packet with an address validation
    //#   token, it MUST attempt to validate the token, unless it has already
    //#   completed address validation.
    fn validate(&self) -> bool;
}

//= https://tools.ietf.org/html/draft-ietf-quic-transport-29.txt#8.1.1
//#   8.1.1.  Token Construction
//#
//#   A token sent in a NEW_TOKEN frames or a Retry packet MUST be
//#   constructed in a way that allows the server to identify how it was
//#   provided to a client.  These tokens are carried in the same field,
//#   but require different handling from servers.
pub struct AddressValidationToken {
    //= https://tools.ietf.org/html/draft-ietf-quic-transport-29.txt#8.1.4
    //#   There is no need for a single well-defined format for the token
    //#   because the server that generates the token also consumes it.

    //= https://tools.ietf.org/html/draft-ietf-quic-transport-29.txt#8.1.4
    //#   Tokens sent in Retry packets SHOULD include information that allows the
    //#   server to verify that the source IP address and port in client
    //#   packets remain constant.
    // TODO This is based on the logic used for the server PreferredAddress. Try to find a better
    // way to handle ipv4 OR ipv6 logic.
    ipv4_peer_address: Option<SocketAddressV4>,
    ipv6_peer_address: Option<SocketAddressV6>,

    //= https://tools.ietf.org/html/draft-ietf-quic-transport-29.txt#21.2
    //#   An attacker might be able to receive an address validation token
    //#   (Section 8) from a server and then release the IP address it used to
    //#   acquire that token.
    //#   Servers SHOULD provide mitigations for this attack by limiting the
    //#   usage and lifetime of address validation tokens
    lifetime: u64,

    //= https://tools.ietf.org/html/draft-ietf-quic-transport-29.txt#8.1.3
    //#   An address validation token MUST be difficult to guess.  Including a
    //#   large enough random value in the token would be sufficient, but this
    //#   depends on the server remembering the value it sends to clients.
    nonce: [u8; 16],

    //= https://tools.ietf.org/html/draft-ietf-quic-transport-29.txt#8.1.3
    //#   A token-based scheme allows the server to offload any state
    //#   associated with validation to the client.  For this design to work,
    //#   the token MUST be covered by integrity protection against
    //#   modification or falsification by clients.  Without integrity
    //#   protection, malicious clients could generate or guess values for
    //#   tokens that would be accepted by the server.  Only the server
    //#   requires access to the integrity protection key for tokens.
    mac: [u8; 32],
}

impl<'a> EncoderValue for AddressValidationToken {
    fn encode<E: Encoder>(&self, buffer: &mut E) {
        if let Some(ip) = self.ipv4_peer_address.as_ref() {
            buffer.encode(ip);
        } else {
            buffer.write_repeated(size_of::<SocketAddressV4>(), 0);
        }

        if let Some(ip) = self.ipv6_peer_address.as_ref() {
            buffer.encode(ip);
        } else {
            buffer.write_repeated(size_of::<SocketAddressV6>(), 0);
        }

        buffer.encode(&self.lifetime);
        buffer.encode(&self.nonce.as_ref());
        buffer.encode(&self.mac.as_ref());
    }
}

decoder_value!(
    impl<'a> AddressValidationToken {
        fn decode(buffer: Buffer) -> Result<Self> {
            let (ipv4_address, buffer) = buffer.decode::<SocketAddressV4>()?;
            let ipv4_address = ipv4_address.filter_unspecified();
            let (ipv6_address, buffer) = buffer.decode::<SocketAddressV6>()?;
            let ipv6_address = ipv6_address.filter_unspecified();
            let (lifetime, buffer) = buffer.decode::<u64>()?;
            let (nonce_slice, buffer) = buffer.decode_slice(16)?;
            let nonce_slice: &[u8] = nonce_slice.into_less_safe_slice();
            let mut nonce: [u8; 16] = [0; 16];
            nonce[..16].copy_from_slice(nonce_slice);
            let (mac_slice, buffer) = buffer.decode_slice(32)?;
            let mac_slice: &[u8] = mac_slice.into_less_safe_slice();
            let mut mac: [u8; 32] = [0; 32];
            mac[..32].copy_from_slice(mac_slice);

            let token = Self {
                ipv4_peer_address: ipv4_address,
                ipv6_peer_address: ipv6_address,
                lifetime,
                nonce,
                mac,
            };

            Ok((token, buffer))
        }
    }
);

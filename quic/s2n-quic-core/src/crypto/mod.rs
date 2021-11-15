// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#![forbid(unsafe_code)]

//! QUIC cryptography primitives and traits
//!
//! ## Decryption flow
//!
//! The lifecycle of a protected and encrypted payload follows the following flow:
//!
//! ```text
//!             +----------------+
//!             |ProtectedPayload|
//!             +-------+--------+
//!                     |
//!                     | unprotect()
//!                     |
//!          +----------+------------+
//!          |                       |
//!          |                       v
//!          |             +---------+-----------+
//!          |             |TruncatedPacketNumber|
//!          v             +---------+-----------+
//! +--------+--------+              |
//! |EncryptedPayload |              | expand(largest_acknowledged_packet_number)
//! +--------+--------+              v
//!          |                 +-----+------+
//!          |                 |PacketNumber|
//!          |                 +-----+------+
//!          |                       |
//!          +----------+------------+
//!                     |
//!                     |  decrypt()
//!                     v
//!              +------+---------+
//!              |CleartextPayload|
//!              +----------------+
//! ```
//!
//! The implementation of the decryption flow looks like the following:
//!
//! ```rust,ignore
//! let crypto = ..; // initialize crypto keys
//! let protected_payload = ..; // decode the payload from the incoming packet
//! let header_len = ..; // decode packet to derive header_len
//! let largest_acknowledged_packet_number = ..; // fetch the largest packet number from connection state
//!
//! let (truncated_packet_number, encrypted_payload) = crate::crypto::unprotect(
//!     &crypto,
//!     largest_acknowledged_packet_number.space(),
//!     header_len,
//!     protected_payload,
//! )?;
//!
//! let packet_number = truncated_packet_number.expand(largest_acknowledged_packet_number)?;
//!
//! let cleartext_payload = crate::crypto::decrypt(
//!     &crypto,
//!     packet_number,
//!     encrypted_payload,
//! )?;
//! ```
//!
//! ## Encryption flow
//!
//! Inversely, a cleartext payload follows the following flow:
//!
//! ```text
//! +----------------+                        +------------+
//! |CleartextPayload|                        |PacketNumber|
//! +-------+--------+                        +------+-----+
//!         |                                        |
//!         |                         +--------------+-----------------------------------+
//!         |                         |                                                  |
//!         |                         |   truncate(largest_acknowledged_packet_number)   |
//!         |                         |                                                  |
//!         |                         |                                                  |
//!         |   encode()   +----------+----------+                                       |
//!         +<-------------+TruncatedPacketNumber|                                       |
//!         |              +----------+----------+                                       |
//!         |                         |                                                  |
//!         |                         | len()                                            |
//!         |                         v                                                  |
//!         |   apply_mask()  +-------+-------+                                          |
//!         <-----------------+PacketNumberLen|                                          |
//!         |                 +--+----+-------+                                          |
//!         |                    |    |                                                  |
//!         +--------------------(----+---------------+----------------------------------+
//!                              |                    |
//!                              |                    |  encrypt()
//!                              |                    |
//!                              |           +--------+-------+
//!                              |           |EncryptedPayload|
//!                              |           +--------+-------+
//!                              |                    |
//!                              |                    |
//!                              +-----------+--------+
//!                                          |
//!                                          | protect()
//!                                          |
//!                                  +-------+--------+
//!                                  |ProtectedPayload|
//!                                  +----------------+
//! ```
//!
//! The implementation of the encryption flow looks like the following:
//!
//! ```rust,ignore
//! let crypto = ..; // initialize crypto keys
//! let cleartext_payload = ..; // encode an outgoing packet
//! let header_len = ..; // encode packet to derive header_len
//! let packet_number = ..; // use the packet number from the outgoing packet
//! let largest_acknowledged_packet_number = ..; // fetch the largest packet number from connection state
//!
//! let truncated_packet_number = packet_number.truncate(largest_acknowledged_packet_number).unwrap();
//! cleartext_payload[header_len..].encode(truncated_packet_number);
//! let packet_number_len = truncated_packet_number.len();
//! cleartext_payload[0] &= packet_number_len.into_packet_tag_mask();
//!
//! let (encrypted_payload, remaining_payload) = crate::crypto::encrypt(
//!     &crypto,
//!     packet_number,
//!     packet_number_len,
//!     header_len,
//!     cleartext_payload,
//! )?;
//!
//! let protected_payload =
//!     crate::crypto::protect(&crypto, encrypted_payload)?;
//! ```
//!

pub mod application;
pub mod error;
pub mod handshake;
pub mod header_crypto;
pub mod initial;
pub mod key;
pub mod label;
pub mod one_rtt;
pub mod packet_protection;
pub mod payload;
pub mod retry;
pub mod tls;
pub mod zero_rtt;

pub use application::*;
pub use error::*;
pub use handshake::*;
pub use header_crypto::*;
pub use initial::*;
pub use key::*;
pub use one_rtt::*;
pub use packet_protection::*;
pub use payload::*;
pub use retry::RetryKey;
pub use zero_rtt::*;

/// Trait which aggregates all Crypto types
pub trait CryptoSuite {
    type HandshakeKey: HandshakeKey;
    type HandshakeHeaderKey: HandshakeHeaderKey;
    type InitialKey: InitialKey<HeaderKey = Self::InitialHeaderKey>;
    type InitialHeaderKey: InitialHeaderKey;
    type OneRttKey: OneRttKey;
    type OneRttHeaderKey: OneRttHeaderKey;
    type ZeroRttKey: ZeroRttKey;
    type ZeroRttHeaderKey: ZeroRttHeaderKey;
    type RetryKey: RetryKey;
}

use crate::packet::number::{
    PacketNumber, PacketNumberLen, PacketNumberSpace, TruncatedPacketNumber,
};
use s2n_codec::{DecoderBuffer, DecoderBufferMut, DecoderError, Encoder, EncoderBuffer};

/// Protects an `EncryptedPayload` into a `ProtectedPayload`
#[inline]
pub fn protect<'a, K: HeaderKey>(
    crypto: &K,
    payload: EncryptedPayload<'a>,
) -> Result<ProtectedPayload<'a>, DecoderError> {
    let header_len = payload.header_len;
    let packet_number_len = payload.packet_number_len.bytesize();
    let sample_len = crypto.sealing_sample_len();

    let orig_payload = payload.buffer.into_less_safe_slice();
    {
        let (first, payload) = orig_payload.split_at_mut(1);
        let first = first.get_mut(0).unwrap();

        let (_, payload) = payload.split_at_mut(header_len - 1);
        let (pn_bytes, payload) = payload.split_at_mut(packet_number_len);

        let (_, payload) = payload.split_at_mut(PacketNumberLen::MAX_LEN - packet_number_len);
        let (sample, _) = payload.split_at_mut(sample_len);

        crypto.protect(sample, first, pn_bytes).unwrap();
    }

    Ok(ProtectedPayload::new(header_len, orig_payload))
}

/// Removes packet protection from a `ProtectedPayload` into a `EncryptedPayload`
/// and associated `TruncatedPacketNumber`
#[inline]
pub fn unprotect<'a, K: HeaderKey>(
    crypto: &K,
    space: PacketNumberSpace,
    payload: ProtectedPayload<'a>,
) -> Result<(TruncatedPacketNumber, EncryptedPayload<'a>), DecoderError> {
    let header_len = payload.header_len;
    let sample_len = crypto.sealing_sample_len();
    let orig_payload = payload.buffer.into_less_safe_slice();
    let (packet_number, packet_number_len) = {
        let (first, payload) = orig_payload.split_at_mut(1);
        let first = first.get_mut(0).unwrap();

        let (_, payload) = payload.split_at_mut(header_len - 1);
        let (pn_bytes, payload) = payload.split_at_mut(PacketNumberLen::MAX_LEN);

        let (sample, _) = payload.split_at_mut(sample_len);

        crypto.unprotect(sample, first, pn_bytes, space).unwrap();

        let packet_number_len = space.new_packet_number_len(*first);
        let (packet_number, _) =
            packet_number_len.decode_truncated_packet_number(DecoderBuffer::new(pn_bytes))?;

        (packet_number, packet_number_len)
    };

    Ok((
        packet_number,
        EncryptedPayload::new(header_len, packet_number_len, orig_payload),
    ))
}

/// Encrypts a cleartext payload with a crypto key into a `EncryptedPayload`
#[inline]
pub fn encrypt<'a, K: Key>(
    key: &K,
    packet_number: PacketNumber,
    packet_number_len: PacketNumberLen,
    header_len: usize,
    mut payload: EncoderBuffer<'a>,
) -> Result<(EncryptedPayload<'a>, EncoderBuffer<'a>), CryptoError> {
    let header_with_pn_len = packet_number_len.bytesize() + header_len;

    // Make space for the key tag
    payload.write_repeated(key.tag_len(), 0);

    let (payload, remaining) = payload.split_off();

    debug_assert!(
        header_with_pn_len < payload.len(),
        "header len ({}) should be less than payload ({})",
        header_with_pn_len,
        payload.len()
    );
    let (header, body) = payload.split_at_mut(header_with_pn_len);
    key.encrypt(packet_number.as_crypto_nonce(), header, body)?;

    let encrypted_payload = EncryptedPayload::new(header_len, packet_number_len, payload);
    let remaining = EncoderBuffer::new(remaining);

    Ok((encrypted_payload, remaining))
}

/// Decrypts a `EncryptedPayload` into clear text
#[inline]
pub fn decrypt<'a, K: Key>(
    key: &K,
    packet_number: PacketNumber,
    payload: EncryptedPayload<'a>,
) -> Result<(DecoderBufferMut<'a>, DecoderBufferMut<'a>), CryptoError> {
    let (header, payload) = payload.split_mut();
    key.decrypt(packet_number.as_crypto_nonce(), header, payload)?;

    // remove the key tag from payload
    let payload_len = payload.len() - key.tag_len();
    let payload = &mut payload[0..payload_len];

    Ok((header.into(), payload.into()))
}

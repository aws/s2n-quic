// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{credentials, crypto::seal, packet::WireVersion};
use s2n_codec::{
    decoder_invariant, decoder_value, DecoderBuffer, DecoderBufferMut,
    DecoderBufferMutResult as Rm, DecoderBufferResult as R, DecoderError, DecoderValue, Encoder,
    EncoderBuffer, EncoderValue,
};
use s2n_quic_core::varint::VarInt;
use zerocopy::{FromBytes, Unaligned};

#[macro_use]
mod decoder;
mod encoder;

const UNKNOWN_PATH_SECRET: u8 = 0b0110_0000;
const STALE_KEY: u8 = 0b0110_0001;
const REPLAY_DETECTED: u8 = 0b0110_0010;

pub const MAX_PACKET_SIZE: usize = 64;
pub const TAG_LEN: usize = 16;

macro_rules! impl_tag {
    ($tag:expr) => {
        #[derive(Clone, Copy, PartialEq, Eq, FromBytes, Unaligned)]
        #[repr(C)]
        pub struct Tag(u8);

        impl core::fmt::Debug for Tag {
            fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
                f.debug_struct(concat!(module_path!(), "::Tag")).finish()
            }
        }

        impl Tag {
            pub const VALUE: u8 = $tag;
        }

        impl From<Tag> for u8 {
            #[inline]
            fn from(v: Tag) -> Self {
                v.0
            }
        }

        decoder_value!(
            impl<'a> Tag {
                fn decode(buffer: Buffer) -> Result<Self> {
                    let (tag, buffer) = buffer.decode()?;
                    decoder_invariant!(tag == $tag, "invalid tag");
                    Ok((Self(tag), buffer))
                }
            }
        );

        impl EncoderValue for Tag {
            #[inline]
            fn encode<E: Encoder>(&self, e: &mut E) {
                self.0.encode(e)
            }
        }

        impl Default for Tag {
            #[inline]
            fn default() -> Self {
                Self($tag)
            }
        }
    };
}

#[cfg(test)]
macro_rules! impl_tests {
    ($ty:ident) => {
        #[test]
        fn round_trip_test() {
            use crate::crypto::awslc::{open, seal};
            use aws_lc_rs::hmac::HMAC_SHA256;

            let key = &[0u8; 16];
            let sealer = seal::control::Secret::new(key, &HMAC_SHA256);
            let opener = open::control::Secret::new(key, &HMAC_SHA256);

            bolero::check!()
                .with_type::<$ty>()
                .filter(|v| v.validate().is_some())
                .for_each(|value| {
                    // Also validates that all packets fit into MAX_PACKET_SIZE.
                    let mut buffer = [0u8; MAX_PACKET_SIZE];
                    let len = {
                        let encoder = s2n_codec::EncoderBuffer::new(&mut buffer);
                        value.encode(encoder, &sealer)
                    };

                    {
                        let buffer = s2n_codec::DecoderBufferMut::new(&mut buffer[..len]);
                        let (decoded, _) = Packet::decode(buffer).unwrap();
                        let decoded = decoded.authenticate(&opener).unwrap();
                        assert_eq!(value, decoded);
                    }

                    {
                        let buffer = s2n_codec::DecoderBufferMut::new(&mut buffer[..len]);
                        let (decoded, _) =
                            crate::packet::secret_control::Packet::decode(buffer).unwrap();
                        if let crate::packet::secret_control::Packet::$ty(decoded) = decoded {
                            let decoded = decoded.authenticate(&opener).unwrap();
                            assert_eq!(value, decoded);
                        } else {
                            panic!("decoded as the wrong packet type");
                        }
                    }
                });
        }
    };
}

pub mod replay_detected;
pub mod stale_key;
pub mod unknown_path_secret;

pub use replay_detected::ReplayDetected;
pub use stale_key::StaleKey;
pub use unknown_path_secret::UnknownPathSecret;

#[derive(Clone, Copy, Debug)]
pub enum Packet<'a> {
    UnknownPathSecret(unknown_path_secret::Packet<'a>),
    StaleKey(stale_key::Packet<'a>),
    ReplayDetected(replay_detected::Packet<'a>),
}

impl<'a> Packet<'a> {
    #[inline]
    pub fn decode(buffer: DecoderBufferMut<'a>) -> Rm<'a, Self> {
        let tag = buffer.peek_byte(0)?;

        Ok(match tag {
            UNKNOWN_PATH_SECRET => {
                let (packet, buffer) = unknown_path_secret::Packet::decode(buffer)?;
                (Self::UnknownPathSecret(packet), buffer)
            }
            STALE_KEY => {
                let (packet, buffer) = stale_key::Packet::decode(buffer)?;
                (Self::StaleKey(packet), buffer)
            }
            REPLAY_DETECTED => {
                let (packet, buffer) = replay_detected::Packet::decode(buffer)?;
                (Self::ReplayDetected(packet), buffer)
            }
            _ => return Err(DecoderError::InvariantViolation("invalid tag")),
        })
    }

    #[inline]
    pub fn credential_id(&self) -> &credentials::Id {
        match self {
            Self::UnknownPathSecret(p) => p.credential_id(),
            Self::StaleKey(p) => p.credential_id(),
            Self::ReplayDetected(p) => p.credential_id(),
        }
    }
}

macro_rules! impl_convert {
    ($name:ident, $mod:ident) => {
        impl<'a> From<$mod::Packet<'a>> for Packet<'a> {
            #[inline]
            fn from(packet: $mod::Packet<'a>) -> Self {
                Self::$name(packet)
            }
        }
    };
}

impl_convert!(UnknownPathSecret, unknown_path_secret);
impl_convert!(StaleKey, stale_key);
impl_convert!(ReplayDetected, replay_detected);

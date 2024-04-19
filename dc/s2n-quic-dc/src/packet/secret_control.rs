// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    credentials,
    crypto::{decrypt, encrypt},
};
use s2n_codec::{
    decoder_invariant, decoder_value, DecoderBuffer, DecoderBufferMut,
    DecoderBufferMutResult as Rm, DecoderBufferResult as R, DecoderError, DecoderValue, Encoder,
    EncoderBuffer, EncoderValue,
};
use s2n_quic_core::varint::VarInt;
use zerocopy::{AsBytes, FromBytes, FromZeroes, Unaligned};

#[macro_use]
mod decoder;
mod encoder;
mod nonce;

const UNKNOWN_PATH_SECRET: u8 = 0b0110_0000;
const STALE_KEY: u8 = 0b0110_0001;
const REPLAY_DETECTED: u8 = 0b0110_0010;
const REQUEST_SHARDS: u8 = 0b0110_0011;

macro_rules! impl_tag {
    ($tag:expr) => {
        #[derive(Clone, Copy, PartialEq, Eq, AsBytes, FromBytes, FromZeroes, Unaligned)]
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
            use crate::crypto::awslc::{DecryptKey, EncryptKey, AES_128_GCM};

            let creds = crate::credentials::Credentials {
                id: Default::default(),
                generation_id: Default::default(),
                sequence_id: Default::default(),
            };
            let key = &[0u8; 16];
            let iv = [0u8; 12];
            let encrypt = EncryptKey::new(creds, key, iv, &AES_128_GCM);
            let decrypt = DecryptKey::new(creds, key, iv, &AES_128_GCM);

            bolero::check!()
                .with_type::<$ty>()
                .filter(|v| v.validate().is_some())
                .for_each(|value| {
                    let mut buffer = [0u8; 64];
                    let len = {
                        let encoder = s2n_codec::EncoderBuffer::new(&mut buffer);
                        value.encode(encoder, (&mut &encrypt))
                    };

                    {
                        use decrypt::Key as _;
                        let buffer = s2n_codec::DecoderBufferMut::new(&mut buffer[..len]);
                        let (decoded, _) = Packet::decode(buffer, decrypt.tag_len()).unwrap();
                        let decoded = decoded.authenticate(&mut &decrypt).unwrap();
                        assert_eq!(value, decoded);
                    }

                    {
                        use decrypt::Key as _;
                        let buffer = s2n_codec::DecoderBufferMut::new(&mut buffer[..len]);
                        let (decoded, _) = crate::packet::secret_control::Packet::decode(
                            buffer,
                            decrypt.tag_len(),
                        )
                        .unwrap();
                        if let crate::packet::secret_control::Packet::$ty(decoded) = decoded {
                            let decoded = decoded.authenticate(&mut &decrypt).unwrap();
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
pub mod request_shards;
pub mod stale_key;
pub mod unknown_path_secret;

pub use nonce::Nonce;
pub use replay_detected::ReplayDetected;
pub use request_shards::RequestShards;
pub use stale_key::StaleKey;
pub use unknown_path_secret::UnknownPathSecret;

#[derive(Clone, Copy, Debug)]
pub enum Packet<'a> {
    UnknownPathSecret(unknown_path_secret::Packet<'a>),
    StaleKey(stale_key::Packet<'a>),
    ReplayDetected(replay_detected::Packet<'a>),
    RequestShards(request_shards::Packet<'a>),
}

impl<'a> Packet<'a> {
    #[inline]
    pub fn decode(buffer: DecoderBufferMut<'a>, crypto_tag_len: usize) -> Rm<Self> {
        let tag = buffer.peek_byte(0)?;

        Ok(match tag {
            UNKNOWN_PATH_SECRET => {
                let (packet, buffer) = unknown_path_secret::Packet::decode(buffer)?;
                (Self::UnknownPathSecret(packet), buffer)
            }
            STALE_KEY => {
                let (packet, buffer) = stale_key::Packet::decode(buffer, crypto_tag_len)?;
                (Self::StaleKey(packet), buffer)
            }
            REPLAY_DETECTED => {
                let (packet, buffer) = replay_detected::Packet::decode(buffer, crypto_tag_len)?;
                (Self::ReplayDetected(packet), buffer)
            }
            REQUEST_SHARDS => {
                let (packet, buffer) = request_shards::Packet::decode(buffer, crypto_tag_len)?;
                (Self::RequestShards(packet), buffer)
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
            Self::RequestShards(p) => p.credential_id(),
        }
    }
}

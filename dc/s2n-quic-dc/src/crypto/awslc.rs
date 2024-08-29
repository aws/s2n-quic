// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::IntoNonce;
use aws_lc_rs::aead::{Aad, Algorithm, LessSafeKey, Nonce, UnboundKey, NONCE_LEN};
use s2n_quic_core::{assume, packet::KeyPhase};

pub use aws_lc_rs::aead::{AES_128_GCM, AES_256_GCM};

const TAG_LEN: usize = 16;

pub mod seal {
    use super::*;
    use crate::crypto::seal;

    #[derive(Debug)]
    pub struct Application {
        key: LessSafeKey,
        iv: Iv,
    }

    impl Application {
        #[inline]
        pub fn new(key: &[u8], iv: [u8; NONCE_LEN], algorithm: &'static Algorithm) -> Self {
            let key = UnboundKey::new(algorithm, key).unwrap();
            let key = LessSafeKey::new(key);
            Self { key, iv: Iv(iv) }
        }

        #[inline]
        pub fn algorithm(&self) -> &'static Algorithm {
            self.key.algorithm()
        }
    }

    impl seal::Application for Application {
        #[inline]
        fn key_phase(&self) -> KeyPhase {
            KeyPhase::Zero
        }

        #[inline(always)]
        fn tag_len(&self) -> usize {
            debug_assert_eq!(TAG_LEN, self.key.algorithm().tag_len());
            TAG_LEN
        }

        #[inline]
        fn encrypt(
            &self,
            packet_number: u64,
            header: &[u8],
            extra_payload: Option<&[u8]>,
            payload_and_tag: &mut [u8],
        ) {
            let nonce = self.iv.nonce(packet_number);
            let aad = Aad::from(header);

            let extra_in = extra_payload.unwrap_or(&[][..]);

            unsafe {
                assume!(payload_and_tag.len() >= self.tag_len() + extra_in.len());
            }

            let inline_len = payload_and_tag.len() - self.tag_len() - extra_in.len();

            unsafe {
                assume!(payload_and_tag.len() >= inline_len);
            }
            let (in_out, extra_out_and_tag) = payload_and_tag.split_at_mut(inline_len);

            let result =
                self.key
                    .seal_in_place_scatter(nonce, aad, in_out, extra_in, extra_out_and_tag);

            unsafe {
                assume!(result.is_ok());
            }
        }
    }

    pub mod control {
        use super::{super::control::*, seal};

        macro_rules! impl_control {
            ($name:ident, $tag_len:expr) => {
                #[derive(Debug)]
                pub struct $name(Key);

                impl $name {
                    #[inline]
                    pub fn new(key: &[u8], algorithm: &'static Algorithm) -> Self {
                        let key = Key::new(*algorithm, key);
                        Self(key)
                    }
                }

                impl seal::Control for $name {
                    #[inline]
                    fn tag_len(&self) -> usize {
                        $tag_len
                    }

                    #[inline]
                    fn sign(&self, header: &[u8], tag: &mut [u8]) {
                        sign(&self.0, $tag_len, header, tag)
                    }
                }
            };
        }

        impl_control!(Stream, STREAM_TAG_LEN);

        impl seal::control::Stream for Stream {
            #[inline]
            fn retransmission_tag(
                &self,
                original_packet_number: u64,
                retransmission_packet_number: u64,
                tag_out: &mut [u8],
            ) {
                retransmission_tag(
                    &self.0,
                    original_packet_number,
                    retransmission_packet_number,
                    tag_out,
                )
            }
        }

        impl_control!(Secret, SECRET_TAG_LEN);

        impl seal::control::Secret for Secret {}
    }
}

pub mod open {
    use super::*;
    use crate::crypto::{
        open::{self, *},
        UninitSlice,
    };
    use s2n_quic_core::ensure;

    #[derive(Debug)]
    pub struct Application {
        key: LessSafeKey,
        iv: Iv,
    }

    impl Application {
        #[inline]
        pub fn new(key: &[u8], iv: [u8; NONCE_LEN], algorithm: &'static Algorithm) -> Self {
            let key = UnboundKey::new(algorithm, key).unwrap();
            let key = LessSafeKey::new(key);
            Self { key, iv: Iv(iv) }
        }
    }

    impl open::Application for Application {
        #[inline]
        fn tag_len(&self) -> usize {
            debug_assert_eq!(TAG_LEN, self.key.algorithm().tag_len());
            TAG_LEN
        }

        #[inline]
        fn decrypt(
            &self,
            key_phase: KeyPhase,
            packet_number: u64,
            header: &[u8],
            payload_in: &[u8],
            tag: &[u8],
            payload_out: &mut UninitSlice,
        ) -> Result {
            ensure!(
                key_phase == KeyPhase::Zero,
                Err(Error::RotationNotSupported)
            );
            debug_assert_eq!(payload_in.len(), payload_out.len());

            let nonce = self.iv.nonce(packet_number);
            let aad = Aad::from(header);

            let payload_out = unsafe {
                // SAFETY: the payload is not read by aws-lc, only written to
                let ptr = payload_out.as_mut_ptr();
                let len = payload_out.len();
                core::slice::from_raw_parts_mut(ptr, len)
            };

            self.key
                .open_separate_gather(nonce, aad, payload_in, tag, payload_out)
                .map_err(|_| Error::InvalidTag)
        }

        #[inline]
        fn decrypt_in_place(
            &self,
            key_phase: KeyPhase,
            packet_number: u64,
            header: &[u8],
            payload_and_tag: &mut [u8],
        ) -> Result {
            ensure!(
                key_phase == KeyPhase::Zero,
                Err(Error::RotationNotSupported)
            );
            let nonce = self.iv.nonce(packet_number);
            let aad = Aad::from(header);

            self.key
                .open_in_place(nonce, aad, payload_and_tag)
                .map_err(|_| Error::InvalidTag)?;

            Ok(())
        }
    }

    pub mod control {
        use super::{super::control::*, open};

        macro_rules! impl_control {
            ($name:ident, $tag_len:expr) => {
                #[derive(Debug)]
                pub struct $name(Key);

                impl $name {
                    #[inline]
                    pub fn new(key: &[u8], algorithm: &'static Algorithm) -> Self {
                        let key = Key::new(*algorithm, key);
                        Self(key)
                    }
                }

                impl open::Control for $name {
                    #[inline]
                    fn tag_len(&self) -> usize {
                        $tag_len
                    }

                    #[inline]
                    fn verify(&self, header: &[u8], tag: &[u8]) -> open::Result {
                        verify(&self.0, $tag_len, header, tag)
                    }
                }
            };
        }

        impl_control!(Stream, STREAM_TAG_LEN);

        impl open::control::Stream for Stream {
            #[inline]
            fn retransmission_tag(
                &self,
                original_packet_number: u64,
                retransmission_packet_number: u64,
                tag_out: &mut [u8],
            ) -> open::Result {
                retransmission_tag(
                    &self.0,
                    original_packet_number,
                    retransmission_packet_number,
                    tag_out,
                );
                Ok(())
            }
        }

        impl_control!(Secret, SECRET_TAG_LEN);

        impl open::control::Secret for Secret {}
    }
}

mod control {
    use crate::crypto::open;
    use aws_lc_rs::hmac;

    pub use hmac::{Algorithm, Key};

    //= https://datatracker.ietf.org/doc/html/rfc2104#section-5
    //# A well-known practice with message authentication codes is to
    //# truncate the output of the MAC and output only part of the bits
    //# (e.g., [MM, ANSI]).  Preneel and van Oorschot [PV] show some
    //# analytical advantages of truncating the output of hash-based MAC
    //# functions. The results in this area are not absolute as for the
    //# overall security advantages of truncation. It has advantages (less
    //# information on the hash result available to an attacker) and
    //# disadvantages (less bits to predict for the attacker).  Applications
    //# of HMAC can choose to truncate the output of HMAC by outputting the t
    //# leftmost bits of the HMAC computation for some parameter t (namely,
    //# the computation is carried in the normal way as defined in section 2
    //# above but the end result is truncated to t bits). We recommend that
    //# the output length t be not less than half the length of the hash
    //# output (to match the birthday attack bound) and not less than 80 bits
    //# (a suitable lower bound on the number of bits that need to be
    //# predicted by an attacker).
    pub const STREAM_TAG_LEN: usize = 16;
    pub const SECRET_TAG_LEN: usize = crate::packet::secret_control::TAG_LEN;

    #[inline]
    pub fn sign(key: &Key, expected_tag_len: usize, header: &[u8], tag: &mut [u8]) {
        debug_assert_eq!(tag.len(), expected_tag_len);
        let out = hmac::sign(key, header);
        let out = out.as_ref();
        let len = tag.len().min(out.len());
        tag[..len].copy_from_slice(&out[..len]);
    }

    #[inline]
    pub fn verify(
        key: &Key,
        expected_tag_len: usize,
        header: &[u8],
        tag: &[u8],
    ) -> open::Result<()> {
        if tag.len() != expected_tag_len {
            return Err(open::Error::InvalidTag);
        }

        // instead of using the `hmac::verify` function, we implement our own that controls the
        // amount of truncation that happens to the tag.
        let out = hmac::sign(key, header);
        let out = out.as_ref();
        let len = tag.len().min(out.len());

        aws_lc_rs::constant_time::verify_slices_are_equal(&tag[..len], &out[..len])
            .map_err(|_| open::Error::InvalidTag)
    }

    #[inline]
    pub fn retransmission_tag(
        key: &Key,
        original_packet_number: u64,
        retransmission_packet_number: u64,
        tag_out: &mut [u8],
    ) {
        let mut v = [0; 16];
        v[..8].copy_from_slice(&original_packet_number.to_be_bytes());
        v[8..].copy_from_slice(&retransmission_packet_number.to_be_bytes());
        let tag = hmac::sign(key, &v);
        for (a, b) in tag_out.iter_mut().zip(tag.as_ref()) {
            *a ^= b;
        }
    }
}

#[derive(Debug)]
struct Iv([u8; NONCE_LEN]);

impl Iv {
    #[inline]
    fn nonce<N: IntoNonce>(&self, nonce: N) -> Nonce {
        let mut nonce = nonce.into_nonce();
        for (dst, src) in nonce.iter_mut().zip(&self.0) {
            *dst ^= src;
        }
        Nonce::assume_unique_for_key(nonce)
    }
}

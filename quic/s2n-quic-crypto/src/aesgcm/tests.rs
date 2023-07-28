// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    aes::BLOCK_LEN,
    aesgcm::{testing, NONCE_LEN, TAG_LEN},
    testing::{Outcome, MAX_PAYLOAD},
};
use bolero::{check, generator::*};
use core::{convert::TryInto, fmt};
use pretty_hex::{pretty_hex, simple_hex};
use s2n_codec::{encoder::scatter, EncoderBuffer};

#[derive(TypeGenerator)]
struct Input<const KEY_LEN: usize> {
    key: [u8; KEY_LEN],
    nonce: [u8; NONCE_LEN],
    #[generator(0..BLOCK_LEN * 4)]
    aad_len: usize,
    #[generator(gen::<Vec<u8>>().with().len(0..MAX_PAYLOAD))]
    aad_and_payload: Vec<u8>,
}

impl<const KEY_LEN: usize> fmt::Debug for Input<KEY_LEN> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "    key = {}", simple_hex(&self.key))?;
        writeln!(f, "  nonce = {}", simple_hex(&self.nonce))?;

        let (aad, payload) = self.aad_and_payload();

        writeln!(f)?;
        writeln!(f, "aad:\n{}", pretty_hex(&aad))?;
        writeln!(f)?;
        writeln!(f, "payload:\n{}", pretty_hex(&payload))?;
        Ok(())
    }
}

impl<const KEY_LEN: usize> Input<KEY_LEN> {
    fn aad_and_payload(&self) -> (&[u8], &[u8]) {
        let aad_and_payload = &self.aad_and_payload[..];
        let idx = self.aad_len.min(aad_and_payload.len());
        aad_and_payload.split_at(idx)
    }
}

macro_rules! impl_tests {
    ($name:ident, $test_vectors:expr) => {
        mod $name {
            use super::*;
            use crate::aes::$name::KEY_LEN;
            use hex_literal::hex;
            use testing::$name::Implementation;

            fn ensure_match(
                impls: &[Implementation],
                input: &Input<KEY_LEN>,
                flip_idx: Option<usize>,
            ) -> Vec<u8> {
                let mut outcomes = vec![];
                let mut failed_decrypts = vec![];

                for imp in impls {
                    let key = imp.new(input.key);
                    let name = imp.name();
                    let (aad, payload) = input.aad_and_payload();
                    let mut output = payload.to_vec();
                    output.extend_from_slice(&[0; TAG_LEN]);

                    // encrypt the payload
                    {
                        let mut buffer = EncoderBuffer::new(&mut output);
                        buffer.advance_position(payload.len());
                        let mut buffer = scatter::Buffer::new(buffer);
                        key.encrypt(&input.nonce, aad, &mut buffer).unwrap();
                    }

                    let outcome = Outcome {
                        name,
                        output: output.clone(),
                    };
                    outcomes.push(outcome);

                    // flip some arbitrary index and make sure it doesn't authenticate
                    if let Some(flip_idx) = flip_idx {
                        let mut output = output.clone();
                        let idx = flip_idx % output.len();
                        output[idx] = !output[idx];

                        let (encrypted, tag) = output.split_at_mut(payload.len());
                        let tag = (&*tag).try_into().unwrap();

                        assert!(key.decrypt(&input.nonce, aad, encrypted, tag).is_err());
                    }

                    // decrypt the encrypted payload and make sure it matches
                    {
                        let (encrypted, tag) = output.split_at_mut(payload.len());
                        let tag = (&*tag).try_into().unwrap();

                        if key.decrypt(&input.nonce, aad, encrypted, tag).is_err() {
                            failed_decrypts.push(name);
                        } else if payload != encrypted {
                            failed_decrypts.push(name);
                        }
                    }
                }

                let first = &outcomes[0].output;

                let all_match = outcomes[1..].iter().all(|res| first == &res.output);

                assert!(
                    all_match && failed_decrypts.is_empty(),
                    "{:#?}\n{:#?}",
                    outcomes,
                    failed_decrypts
                );

                core::mem::take(&mut outcomes[0].output)
            }

            #[test]
            fn differential_test() {
                let impls = testing::$name::implementations();

                assert!(!impls.is_empty());

                check!().with_type().for_each(|(input, flip_idx)| {
                    ensure_match(impls, input, Some(*flip_idx));
                })
            }

            /// ensures that we can't pull a valid payload out of thin air
            #[test]
            fn decrypt_failure_test() {
                let impls = testing::$name::implementations();

                assert!(!impls.is_empty());

                check!()
                    .with_type()
                    .for_each(|(input, tag): &(Input<KEY_LEN>, [u8; TAG_LEN])| {
                        for imp in impls {
                            let key = imp.new(input.key);
                            let name = imp.name();
                            let (aad, ciphertext) = input.aad_and_payload();
                            let mut ciphertext = ciphertext.to_vec();

                            // ensure we have at least a tag in the payload
                            ciphertext.extend(tag);

                            let payload_len = ciphertext.len() - TAG_LEN;

                            let (ciphertext, tag) = ciphertext.split_at_mut(payload_len);
                            let tag = (&*tag).try_into().unwrap();

                            assert!(
                                key.decrypt(&input.nonce, aad, ciphertext, tag).is_err(),
                                "{} authenticated a fabricated packet",
                                name
                            );
                        }
                    })
            }

            #[test]
            fn test_vectors() {
                let impls = testing::$name::implementations();

                assert!(!impls.is_empty());

                let tests = $test_vectors;

                for (input, ciphertext_and_tag) in tests.iter() {
                    let actual = ensure_match(impls, input, None);
                    assert_eq!(&actual, ciphertext_and_tag);
                }
            }

            #[test]
            fn batch_test() {
                let impls = testing::$name::implementations();

                assert!(!impls.is_empty());
                for payload_len in [
                    0,
                    1,
                    BLOCK_LEN - 1,
                    BLOCK_LEN,
                    BLOCK_LEN + 1,
                    BLOCK_LEN * 5,
                    BLOCK_LEN * 6,
                    BLOCK_LEN * 7,
                ] {
                    eprintln!("payload len = {}", payload_len);
                    ensure_match(
                        impls,
                        &Input {
                            key: [1u8; KEY_LEN],
                            nonce: [1u8; NONCE_LEN],
                            aad_len: 0,
                            aad_and_payload: vec![1; payload_len],
                        },
                        None,
                    );
                }
            }
        }
    };
}

impl_tests!(aes128, {
    vec![
        // https://github.com/awslabs/aws-lc/blob/aed75eb04d322d101941e1377f274484f5e4f5b8/crypto/fipsmodule/modes/gcm_tests.txt
        // Key = 00000000000000000000000000000000
        // Plaintext =
        // AdditionalData =
        // Nonce = 000000000000000000000000
        // Ciphertext =
        // Tag = 58e2fccefa7e3061367f1d57a4e7455a
        (
            Input {
                key: hex!("00000000000000000000000000000000"),
                nonce: hex!("000000000000000000000000"),
                aad_len: 0,
                aad_and_payload: vec![],
            },
            hex!(
                "
                58e2fccefa7e3061367f1d57a4e7455a
                "
            )
            .to_vec(),
        ),
        // Key = 00000000000000000000000000000000
        // Plaintext = 00000000000000000000000000000000
        // AdditionalData =
        // Nonce = 000000000000000000000000
        // Ciphertext = 0388dace60b6a392f328c2b971b2fe78
        // Tag = ab6e47d42cec13bdf53a67b21257bddf
        (
            Input {
                key: hex!("00000000000000000000000000000000"),
                nonce: hex!("000000000000000000000000"),
                aad_len: 0,
                aad_and_payload: hex!(
                    "
                    00000000000000000000000000000000
                    "
                )
                .to_vec(),
            },
            hex!(
                "
                0388dace60b6a392f328c2b971b2fe78
                ab6e47d42cec13bdf53a67b21257bddf
                "
            )
            .to_vec(),
        ),
        // Key = feffe9928665731c6d6a8f9467308308
        // Plaintext = d9313225f88406e5a55909c5aff5269a86a7a9531534f7da2e4c303d8a318a721c3c0c95956809532fcf0e2449a6b525b16aedf5aa0de657ba637b391aafd255
        // AdditionalData =
        // Nonce = cafebabefacedbaddecaf888
        // Ciphertext = 42831ec2217774244b7221b784d0d49ce3aa212f2c02a4e035c17e2329aca12e21d514b25466931c7d8f6a5aac84aa051ba30b396a0aac973d58e091473f5985
        // Tag = 4d5c2af327cd64a62cf35abd2ba6fab4
        (
            Input {
                key: hex!("feffe9928665731c6d6a8f9467308308"),
                nonce: hex!("cafebabefacedbaddecaf888"),
                aad_len: 0,
                aad_and_payload: hex!(
                    "
                    d9313225f88406e5a55909c5aff5269a86a7a9531534f7da2e4c303d8a318a721c3c0c95956809532fcf0e2449a6b525b16aedf5aa0de657ba637b391aafd255
                    "
                )
                .to_vec(),
            },
            hex!(
                "
                42831ec2217774244b7221b784d0d49ce3aa212f2c02a4e035c17e2329aca12e21d514b25466931c7d8f6a5aac84aa051ba30b396a0aac973d58e091473f5985
                4d5c2af327cd64a62cf35abd2ba6fab4
                "
            )
            .to_vec(),
        ),
        // Key = feffe9928665731c6d6a8f9467308308
        // Plaintext = d9313225f88406e5a55909c5aff5269a86a7a9531534f7da2e4c303d8a318a721c3c0c95956809532fcf0e2449a6b525b16aedf5aa0de657ba637b39
        // AdditionalData = feedfacedeadbeeffeedfacedeadbeefabaddad2
        // Nonce = cafebabefacedbaddecaf888
        // Ciphertext = 42831ec2217774244b7221b784d0d49ce3aa212f2c02a4e035c17e2329aca12e21d514b25466931c7d8f6a5aac84aa051ba30b396a0aac973d58e091
        // Tag = 5bc94fbc3221a5db94fae95ae7121a47
        (
            Input {
                key: hex!("feffe9928665731c6d6a8f9467308308"),
                nonce: hex!("cafebabefacedbaddecaf888"),
                aad_len: 20,
                aad_and_payload: hex!(
                    "
                    feedfacedeadbeeffeedfacedeadbeefabaddad2
                    d9313225f88406e5a55909c5aff5269a86a7a9531534f7da2e4c303d8a318a721c3c0c95956809532fcf0e2449a6b525b16aedf5aa0de657ba637b39
                    "
                )
                .to_vec(),
            },
            hex!(
                "
                42831ec2217774244b7221b784d0d49ce3aa212f2c02a4e035c17e2329aca12e21d514b25466931c7d8f6a5aac84aa051ba30b396a0aac973d58e091
                5bc94fbc3221a5db94fae95ae7121a47
                "
            )
            .to_vec(),
        ),
    ]
});
impl_tests!(aes256, {
    vec![
        // https://github.com/awslabs/aws-lc/blob/aed75eb04d322d101941e1377f274484f5e4f5b8/crypto/fipsmodule/modes/gcm_tests.txt
        // Key = 0000000000000000000000000000000000000000000000000000000000000000
        // Plaintext =
        // AdditionalData =
        // Nonce = 000000000000000000000000
        // Ciphertext =
        // Tag = 530f8afbc74536b9a963b4f1c4cb738b
        (
            Input {
                key: hex!("0000000000000000000000000000000000000000000000000000000000000000"),
                nonce: hex!("000000000000000000000000"),
                aad_len: 0,
                aad_and_payload: vec![],
            },
            hex!(
                "
                530f8afbc74536b9a963b4f1c4cb738b
                "
            )
            .to_vec(),
        ),
        // Key = 0000000000000000000000000000000000000000000000000000000000000000
        // Plaintext = 00000000000000000000000000000000
        // AdditionalData =
        // Nonce = 000000000000000000000000
        // Ciphertext = cea7403d4d606b6e074ec5d3baf39d18
        // Tag = d0d1c8a799996bf0265b98b5d48ab919
        (
            Input {
                key: hex!("0000000000000000000000000000000000000000000000000000000000000000"),
                nonce: hex!("000000000000000000000000"),
                aad_len: 0,
                aad_and_payload: hex!(
                    "
                    00000000000000000000000000000000
                    "
                )
                .to_vec(),
            },
            hex!(
                "
                cea7403d4d606b6e074ec5d3baf39d18
                d0d1c8a799996bf0265b98b5d48ab919
                "
            )
            .to_vec(),
        ),
        // Key = feffe9928665731c6d6a8f9467308308feffe9928665731c6d6a8f9467308308
        // Plaintext = d9313225f88406e5a55909c5aff5269a86a7a9531534f7da2e4c303d8a318a721c3c0c95956809532fcf0e2449a6b525b16aedf5aa0de657ba637b391aafd255
        // AdditionalData =
        // Nonce = cafebabefacedbaddecaf888
        // Ciphertext = 522dc1f099567d07f47f37a32a84427d643a8cdcbfe5c0c97598a2bd2555d1aa8cb08e48590dbb3da7b08b1056828838c5f61e6393ba7a0abcc9f662898015ad
        // Tag = b094dac5d93471bdec1a502270e3cc6c
        (
            Input {
                key: hex!("feffe9928665731c6d6a8f9467308308feffe9928665731c6d6a8f9467308308"),
                nonce: hex!("cafebabefacedbaddecaf888"),
                aad_len: 0,
                aad_and_payload: hex!(
                    "
                    d9313225f88406e5a55909c5aff5269a86a7a9531534f7da2e4c303d8a318a721c3c0c95956809532fcf0e2449a6b525b16aedf5aa0de657ba637b391aafd255
                    "
                )
                .to_vec(),
            },
            hex!(
                "
                522dc1f099567d07f47f37a32a84427d643a8cdcbfe5c0c97598a2bd2555d1aa8cb08e48590dbb3da7b08b1056828838c5f61e6393ba7a0abcc9f662898015ad
                b094dac5d93471bdec1a502270e3cc6c
                "
            )
            .to_vec(),
        ),
        // Key = feffe9928665731c6d6a8f9467308308feffe9928665731c6d6a8f9467308308
        // Plaintext = d9313225f88406e5a55909c5aff5269a86a7a9531534f7da2e4c303d8a318a721c3c0c95956809532fcf0e2449a6b525b16aedf5aa0de657ba637b39
        // AdditionalData = feedfacedeadbeeffeedfacedeadbeefabaddad2
        // Nonce = cafebabefacedbaddecaf888
        // Ciphertext = 522dc1f099567d07f47f37a32a84427d643a8cdcbfe5c0c97598a2bd2555d1aa8cb08e48590dbb3da7b08b1056828838c5f61e6393ba7a0abcc9f662
        // Tag = 76fc6ece0f4e1768cddf8853bb2d551b
        (
            Input {
                key: hex!("feffe9928665731c6d6a8f9467308308feffe9928665731c6d6a8f9467308308"),
                nonce: hex!("cafebabefacedbaddecaf888"),
                aad_len: 20,
                aad_and_payload: hex!(
                    "
                    feedfacedeadbeeffeedfacedeadbeefabaddad2
                    d9313225f88406e5a55909c5aff5269a86a7a9531534f7da2e4c303d8a318a721c3c0c95956809532fcf0e2449a6b525b16aedf5aa0de657ba637b39
                    "
                )
                .to_vec(),
            },
            hex!(
                "
                522dc1f099567d07f47f37a32a84427d643a8cdcbfe5c0c97598a2bd2555d1aa8cb08e48590dbb3da7b08b1056828838c5f61e6393ba7a0abcc9f662
                76fc6ece0f4e1768cddf8853bb2d551b
                "
            )
            .to_vec(),
        ),
    ]
});

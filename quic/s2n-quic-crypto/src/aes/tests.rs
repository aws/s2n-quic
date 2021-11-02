// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{aes::testing, testing::Outcome};
use bolero::check;
use core::convert::TryInto;

macro_rules! impl_tests {
    ($name:ident, $test_vectors:expr) => {
        mod $name {
            use super::*;
            use crate::aes::$name::KEY_LEN;
            use hex_literal::hex;
            use testing::$name::Implementation;

            fn ensure_match(impls: &[Implementation], key: [u8; KEY_LEN], input: &[u8]) -> Vec<u8> {
                let mut outcomes = vec![];

                for imp in impls {
                    let key = imp.new(key);
                    let name = imp.name();
                    let mut output = input.to_vec();
                    key.encrypt(&mut output);
                    let outcome = Outcome {
                        name,
                        output: output.clone(),
                    };
                    outcomes.push(outcome);

                    // make sure it decrypts to the original value
                    key.decrypt(&mut output);
                    assert_eq!(&output[..], input);
                }

                let first = &outcomes[0].output;

                let all_match = outcomes[1..].iter().all(|res| first == &res.output);

                assert!(all_match, "{:#?}", outcomes);

                core::mem::take(&mut outcomes[0].output)
            }

            #[test]
            fn differential_test() {
                let impls = testing::$name::implementations();

                assert!(!impls.is_empty());

                check!().for_each(|bytes| {
                    if bytes.len() < KEY_LEN {
                        return;
                    }

                    let (key, input) = bytes.split_at(KEY_LEN);
                    let key = key.try_into().unwrap();

                    ensure_match(impls, key, input);
                })
            }

            #[test]
            fn test_vectors() {
                let impls = testing::$name::implementations();

                assert!(!impls.is_empty());

                let tests = $test_vectors;

                for (key, plaintext, ciphertext) in tests.iter() {
                    let actual = ensure_match(impls, *key, plaintext);
                    assert_eq!(actual, ciphertext);
                }
            }
        }
    };
}

impl_tests!(aes128, {
    vec![
        // https://github.com/awslabs/aws-lc/blob/aed75eb04d322d101941e1377f274484f5e4f5b8/crypto/fipsmodule/aes/aes_tests.txt
        // # Test vectors from FIPS-197, Appendix C.
        // Mode = Raw
        // Key = 000102030405060708090a0b0c0d0e0f
        // Plaintext = 00112233445566778899aabbccddeeff
        // Ciphertext = 69c4e0d86a7b0430d8cdb78070b4c55a
        (
            hex!("000102030405060708090a0b0c0d0e0f"),
            hex!("00112233445566778899aabbccddeeff"),
            hex!("69c4e0d86a7b0430d8cdb78070b4c55a"),
        ),
    ]
});

impl_tests!(aes256, {
    vec![
        // https://github.com/awslabs/aws-lc/blob/aed75eb04d322d101941e1377f274484f5e4f5b8/crypto/fipsmodule/aes/aes_tests.txt
        // # Test vectors from FIPS-197, Appendix C.
        // Mode = Raw
        // Key = 000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f
        // Plaintext = 00112233445566778899aabbccddeeff
        // Ciphertext = 8ea2b7ca516745bfeafc49904b496089
        (
            hex!("000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f"),
            hex!("00112233445566778899aabbccddeeff"),
            hex!("8ea2b7ca516745bfeafc49904b496089"),
        ),
    ]
});

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{aes::testing, testing::Outcome};
use bolero::check;
use core::convert::TryInto;

macro_rules! impl_tests {
    ($name:ident) => {
        mod $name {
            use super::*;
            use crate::aes::$name::KEY_LEN;
            use testing::$name::Implementation;

            fn ensure_match(impls: &[Implementation], key: [u8; KEY_LEN], input: &[u8]) {
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
        }
    };
}

impl_tests!(aes128);
impl_tests!(aes256);

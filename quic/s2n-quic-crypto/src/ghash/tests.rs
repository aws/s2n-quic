// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    ghash::{testing, KEY_LEN},
    testing::Outcome,
};
use bolero::check;
use core::convert::TryInto;

fn ensure_match(impls: &[testing::Implementation], key: [u8; 16], input: &[u8]) {
    // make sure we don't go beyond the max payload len
    let input_len = input.len().min(crate::testing::MAX_PAYLOAD);
    let input = &input[..input_len];

    let mut outcomes = vec![];

    for imp in impls {
        let key = imp.new(key);
        let name = imp.name();
        let tag = key.hash(input);
        let outcome = Outcome { name, output: tag };
        outcomes.push(outcome);
    }

    let first = &outcomes[0].output;

    let all_match = outcomes[1..].iter().all(|res| first == &res.output);

    assert!(all_match, "{:#?}", outcomes);
}

#[test]
fn differential_test() {
    let impls = testing::implementations();

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

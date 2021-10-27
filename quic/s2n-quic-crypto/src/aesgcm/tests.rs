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
        let (aad, payload) = aad_and_payload.split_at(idx);

        // round down to the nearest block size
        let payload_len = payload.len() / BLOCK_LEN * BLOCK_LEN;
        let payload = &payload[..payload_len];

        (aad, payload)
    }
}

macro_rules! impl_tests {
    ($name:ident) => {
        mod $name {
            use super::*;
            use crate::aes::$name::KEY_LEN;
            use testing::$name::Implementation;

            fn ensure_match(impls: &[Implementation], input: &Input<KEY_LEN>) {
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
                        let (payload, tag) = output.split_at_mut(payload.len());
                        let tag = tag.try_into().unwrap();
                        key.encrypt(&input.nonce, aad, payload, tag);
                    }

                    let outcome = Outcome {
                        name,
                        output: output.clone(),
                    };
                    outcomes.push(outcome);

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
            }

            #[test]
            fn differential_test() {
                let impls = testing::$name::implementations();

                assert!(!impls.is_empty());

                check!()
                    .with_type()
                    .for_each(|input| ensure_match(impls, input))
            }

            #[test]
            fn batch_test() {
                let impls = testing::$name::implementations();

                assert!(!impls.is_empty());
                for payload_len in [0, 1, BLOCK_LEN, BLOCK_LEN * 5, BLOCK_LEN * 6, BLOCK_LEN * 7]
                    .iter()
                    .copied()
                {
                    eprintln!("payload len = {}", payload_len);
                    ensure_match(
                        impls,
                        &Input {
                            key: [1u8; KEY_LEN],
                            nonce: [1u8; NONCE_LEN],
                            aad_len: 0,
                            aad_and_payload: vec![1; payload_len],
                        },
                    );
                }
            }
        }
    };
}

impl_tests!(aes128);
impl_tests!(aes256);

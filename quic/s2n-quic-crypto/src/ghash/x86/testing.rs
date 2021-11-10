// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    aes::BLOCK_LEN,
    arch::*,
    block::Block,
    ghash::{
        self,
        testing::{GHash, Implementation},
        x86::{self, hkey, precomputed::Array},
    },
    testing::MAX_BLOCKS,
};
use core::{convert::TryInto, marker::PhantomData};
pub struct Impl<G, A>(G, PhantomData<A>)
where
    G: ghash::GHash<Block = __m128i>,
    A: Arch;

impl<G, A> Impl<G, A>
where
    G: ghash::GHash<Block = __m128i>,
    A: Arch,
{
    fn new(key: G) -> Self {
        Self(key, PhantomData)
    }
}

impl<G, A> GHash for Impl<G, A>
where
    G: ghash::GHash<Block = __m128i>,
    A: Arch,
{
    fn hash(&self, input: &[u8]) -> [u8; BLOCK_LEN] {
        unsafe {
            A::call(
                #[inline(always)]
                || {
                    let blocks = input.len() / BLOCK_LEN;

                    let mut state = self.0.start(blocks);

                    for block in input.chunks_exact(BLOCK_LEN) {
                        let block: [u8; BLOCK_LEN] = block.try_into().unwrap();
                        self.0.update(&mut state, &__m128i::from_array(block));
                    }
                    self.0.finish(state).into_array()
                },
            )
        }
    }
}

pub fn implementations(impls: &mut Vec<Implementation>) {
    Avx2::call_supported(|| {
        impls.push(Implementation {
            name: "s2n_quic/std/avx2",
            new: |key| {
                let ghash = x86::GHash::new(key);
                Box::new(<Impl<_, Avx2>>::new(ghash))
            },
        });
        impls.push(Implementation {
            name: "s2n_quic/pre_h/avx2",
            new: |key| {
                let ghash = <Array<hkey::H, MAX_BLOCKS>>::new(key);
                Box::new(<Impl<_, Avx2>>::new(ghash))
            },
        });
        impls.push(Implementation {
            name: "s2n_quic/pre_hr/avx2",
            new: |key| {
                let ghash = <Array<hkey::Hr, MAX_BLOCKS>>::new(key);
                Box::new(<Impl<_, Avx2>>::new(ghash))
            },
        });
    });
}

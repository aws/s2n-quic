// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

macro_rules! impl_aes {
    ($name:ident) => {
        pub mod $name {
            use crate::{
                aes::{
                    testing::{for_each_block, $name::Implementation, Aes},
                    x86::$name::Key,
                    $name::{Key as Wrapper, KEY_LEN},
                    Decrypt, Encrypt,
                },
                arch::*,
                block::Block,
            };
            use core::marker::PhantomData;

            struct Impl<A: Arch>(Wrapper<Key>, PhantomData<A>);

            impl<A: Arch> Impl<A> {
                #[inline(always)]
                fn new(key: [u8; KEY_LEN]) -> Self {
                    unsafe {
                        A::call(
                            #[inline(always)]
                            || {
                                let key = Wrapper(Key::new(key));
                                Self(key, PhantomData)
                            },
                        )
                    }
                }
            }

            impl<A: Arch> Aes for Impl<A>
            where
                Wrapper<Key>: Encrypt<Block = __m128i> + Decrypt<Block = __m128i>,
            {
                fn encrypt(&self, input: &mut [u8]) {
                    unsafe {
                        A::call(
                            #[inline(always)]
                            || {
                                for_each_block(input, |chunk| {
                                    let mut block = __m128i::from_array(*chunk);
                                    self.0.encrypt(&mut block);
                                    chunk.copy_from_slice(&block.into_array());
                                })
                            },
                        )
                    }
                }

                fn decrypt(&self, input: &mut [u8]) {
                    unsafe {
                        A::call(
                            #[inline(always)]
                            || {
                                for_each_block(input, |chunk| {
                                    let mut block = __m128i::from_array(*chunk);
                                    self.0.decrypt(&mut block);
                                    chunk.copy_from_slice(&block.into_array());
                                })
                            },
                        )
                    }
                }
            }

            pub fn implementations(impls: &mut Vec<Implementation>) {
                Avx2::call_supported(|| {
                    impls.push(Implementation {
                        name: "s2n_quic/avx2",
                        new: |key| Box::new(<Impl<Avx2>>::new(key)),
                    });
                });
            }
        }
    };
}

impl_aes!(aes128);
impl_aes!(aes256);

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use rand_chacha::rand_core::TryRng;

pub use s2n_quic_core::random::*;

struct AwsLc;

impl TryRng for AwsLc {
    type Error = core::convert::Infallible;

    #[inline]
    fn try_next_u32(&mut self) -> Result<u32, Self::Error> {
        let mut v = [0; 4];
        aws_lc_rs::rand::fill(&mut v).unwrap();
        Ok(u32::from_ne_bytes(v))
    }

    #[inline]
    fn try_next_u64(&mut self) -> Result<u64, Self::Error> {
        let mut v = [0; 8];
        aws_lc_rs::rand::fill(&mut v).unwrap();
        Ok(u64::from_ne_bytes(v))
    }

    #[inline]
    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), Self::Error> {
        aws_lc_rs::rand::fill(dest).unwrap();
        Ok(())
    }
}

pub struct Random(s2n_quic::provider::random::Random<AwsLc>);

impl Default for Random {
    #[inline]
    fn default() -> Self {
        Self(s2n_quic::provider::random::Random::new(AwsLc, AwsLc))
    }
}

impl Generator for Random {
    #[inline]
    fn public_random_fill(&mut self, dest: &mut [u8]) {
        self.0.public_random_fill(dest);
    }

    #[inline]
    fn private_random_fill(&mut self, dest: &mut [u8]) {
        self.0.private_random_fill(dest);
    }
}

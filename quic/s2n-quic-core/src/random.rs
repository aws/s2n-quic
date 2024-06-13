// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::ops::RangeInclusive;

/// A generator of random data. The two methods provide the same functionality for
/// different use cases. One for "public" randomly generated data that may appear
/// in the clear, and one for "private" data that should remain secret. This approach
/// lessens the risk of potential predictability weaknesses in random number generation
/// algorithms from leaking information across contexts.
pub trait Generator: 'static + Send {
    /// Fills `dest` with unpredictable bits that may be
    /// sent over the wire and viewable in the clear.
    fn public_random_fill(&mut self, dest: &mut [u8]);

    /// Fills `dest` with unpredictable bits that will only be
    /// used internally within the endpoint, remaining secret.
    fn private_random_fill(&mut self, dest: &mut [u8]);
}

/// Generates a random usize within the given inclusive range
///
/// NOTE: This will have slight bias towards the lower end of the range. Usages that
/// require uniform sampling should implement rejection sampling or other methodologies
/// and not copy this implementation.
pub(crate) fn gen_range_biased<R: Generator + ?Sized>(
    random_generator: &mut R,
    range: RangeInclusive<usize>,
) -> usize {
    if range.start() == range.end() {
        return *range.start();
    }

    let mut dest = [0; core::mem::size_of::<usize>()];
    random_generator.public_random_fill(&mut dest);
    let result = usize::from_le_bytes(dest);

    let max_variance = (range.end() - range.start()).saturating_add(1);
    range.start() + result % max_variance
}

#[cfg(any(test, feature = "testing"))]
pub mod testing {
    use crate::random;

    #[derive(Debug, Default)]
    pub struct Generator(pub u8);

    impl random::Generator for Generator {
        fn public_random_fill(&mut self, dest: &mut [u8]) {
            let seed = self.0;

            for (i, elem) in dest.iter_mut().enumerate() {
                *elem = seed ^ i as u8;
            }

            self.0 = self.0.wrapping_add(1)
        }

        fn private_random_fill(&mut self, dest: &mut [u8]) {
            let seed = u8::MAX - self.0;

            for (i, elem) in dest.iter_mut().enumerate() {
                *elem = seed ^ i as u8;
            }

            self.0 = self.0.wrapping_add(1)
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::random;

    #[test]
    #[cfg_attr(miri, ignore)] // This test is too expensive for miri to complete in a reasonable amount of time
    #[cfg_attr(kani, kani::proof, kani::unwind(10), kani::solver(kissat))]
    fn gen_range_biased_test() {
        bolero::check!()
            .with_type()
            .cloned()
            .for_each(|(seed, mut min, mut max)| {
                if min > max {
                    core::mem::swap(&mut min, &mut max);
                }
                let mut generator = random::testing::Generator(seed);
                let result = random::gen_range_biased(&mut generator, min..=max);
                assert!(result >= min);
                assert!(result <= max);
            });
    }
}

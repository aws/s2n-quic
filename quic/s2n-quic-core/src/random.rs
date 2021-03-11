// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

/// A generator of random data. The two methods provide the same functionality for
/// different use cases. One for "public" randomly generated data that may appear
/// in the clear, and one for "private" data that should remain secret. This approach
/// lessens the risk of potential predictability weaknesses in random number generation
/// algorithms from leaking information across contexts.
pub trait Generator: 'static {
    /// Fills `dest` with unpredictable bits that may be
    /// sent over the wire and viewable in the clear.
    fn public_random_fill(&mut self, dest: &mut [u8]);

    /// Fills `dest` with unpredictable bits that will only be
    /// used internally within the endpoint, remaining secret.
    fn private_random_fill(&mut self, dest: &mut [u8]);
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
            let seed = u8::max_value() - self.0;

            for (i, elem) in dest.iter_mut().enumerate() {
                *elem = seed ^ i as u8;
            }

            self.0 = self.0.wrapping_add(1)
        }
    }
}

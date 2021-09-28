// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

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

    /// Return a bool with a probability p of being true.
    fn gen_bool(&mut self, p: f64) -> bool;
}

#[cfg(any(test, feature = "testing"))]
pub mod testing {
    use crate::random;

    #[derive(Debug)]
    pub struct Generator {
        pub seed: u8,
        pub gen_bool_result: bool,
    }

    impl Default for Generator {
        fn default() -> Self {
            Self {
                seed: 123,
                gen_bool_result: false,
            }
        }
    }

    impl random::Generator for Generator {
        fn public_random_fill(&mut self, dest: &mut [u8]) {
            let seed = self.seed;

            for (i, elem) in dest.iter_mut().enumerate() {
                *elem = seed ^ i as u8;
            }

            self.seed = self.seed.wrapping_add(1)
        }

        fn private_random_fill(&mut self, dest: &mut [u8]) {
            let seed = u8::MAX - self.seed;

            for (i, elem) in dest.iter_mut().enumerate() {
                *elem = seed ^ i as u8;
            }

            self.seed = self.seed.wrapping_add(1)
        }

        fn gen_bool(&mut self, _p: f64) -> bool {
            self.gen_bool_result
        }
    }
}

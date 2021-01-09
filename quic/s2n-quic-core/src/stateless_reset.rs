//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#10.3
//# Stateless Reset {
//#   Fixed Bits (2) = 1,
//#   Unpredictable Bits (38..),
//#   Stateless Reset Token (128),
//# }

pub mod token;

pub use token::Token;

/// A generator of unpredictable bits
pub trait UnpredictableBits {
    /// Fills `dest` with unpredictable bits
    fn fill(&mut self, dest: &mut [u8]);
}

#[cfg(any(test, feature = "testing"))]
pub mod testing {
    use crate::stateless_reset;

    #[derive(Debug, Default)]
    pub struct Generator(pub u8);

    impl stateless_reset::UnpredictableBits for Generator {
        fn fill(&mut self, dest: &mut [u8]) {
            let seed = self.0;

            for (i, elem) in dest.iter_mut().enumerate() {
                *elem = seed ^ i as u8;
            }

            self.0 += 1
        }
    }
}

//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#10.3
//# Stateless Reset {
//#   Fixed Bits (2) = 1,
//#   Unpredictable Bits (38..),
//#   Stateless Reset Token (128),
//# }

pub use token::Token;

pub mod token;

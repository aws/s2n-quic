use crate::{
    packet::{long::DESTINATION_CONNECTION_ID_MAX_LEN, number::PacketNumberLen, Tag},
    path::MINIMUM_MTU,
    random, stateless_reset,
};
use core::ops::RangeInclusive;
use s2n_codec::{Encoder, EncoderValue};

//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#10.3
//# Stateless Reset {
//#   Fixed Bits (2) = 1,
//#   Unpredictable Bits (38..),
//#   Stateless Reset Token (128),
//# }

//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#10.3
//# Endpoints MUST send stateless reset packets formatted as a packet
//# with a short header.
const TAG: u8 = 0b0100_0000;
const TAG_OFFSET: u8 = 2;

// Since the length of the destination connection ID is determined by the peer, we use the maximum
// destination connection ID length when determining the minimum stateless reset packet size so
// that stateless resets are indistinguishable from a valid short header packet no matter what
// length connection ID the peer decides to use.
const SHORT_HEADER_LEN: usize =
    core::mem::size_of::<Tag>() + PacketNumberLen::MAX_LEN + DESTINATION_CONNECTION_ID_MAX_LEN;

#[derive(Debug)]
pub struct StatelessReset {
    pub unpredictable_bits: Vec<u8>,
    pub token: stateless_reset::Token,
}

impl StatelessReset {
    pub fn new<R: random::Generator>(
        token: stateless_reset::Token,
        max_tag_len: usize,
        triggering_packet_len: usize,
        random_generator: &mut R,
    ) -> Option<StatelessReset> {
        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#10.3
        //# These values assume that the Stateless Reset Token is the same length
        //# as the minimum expansion of the packet protection AEAD. Additional
        //# unpredictable bytes are necessary if the endpoint could have negotiated
        //# a packet protection scheme with a larger minimum expansion.
        // The tag length for all cipher suites defined in TLS 1.3 is 16 bytes, but
        // we will calculate based on a given max tag length to allow for future cipher
        // suites with larger tags. One additional byte is added to represent the minimum
        // valid payload size.
        let min_len = SHORT_HEADER_LEN + max_tag_len + 1;

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#10.3
        //# An endpoint MUST NOT send a stateless reset that is three times or
        //# more larger than the packet it receives to avoid being used for
        //# amplification.

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#10.3.3
        //# An endpoint MUST ensure that every Stateless Reset that it sends is
        //# smaller than the packet that triggered it, unless it maintains state
        //# sufficient to prevent looping.
        let max_len = (triggering_packet_len - 1).min(MINIMUM_MTU as usize);

        // The packet that triggered this stateless reset was too small to send a stateless reset
        // that would be indistinguishable from a valid short header packet, so we'll just drop the
        // packet instead of sending a stateless reset.
        if max_len < min_len {
            return None;
        }

        let unpredictable_bits = generate_unpredictable_bits(random_generator, min_len..=max_len);

        if cfg!(debug_assertions) {
            let len = unpredictable_bits.len() + token.encoding_size();
            assert!(len >= min_len);
            assert!(len <= max_len);
            assert!(len < triggering_packet_len);
        }

        Some(Self {
            unpredictable_bits,
            token,
        })
    }
}

/// Generates a random amount of random data within the given inclusive
/// range of packet lengths, leaving room for the stateless reset token.
fn generate_unpredictable_bits<R: random::Generator>(
    random_generator: &mut R,
    total_len_range: RangeInclusive<usize>,
) -> Vec<u8> {
    let min_len = total_len_range.start() - stateless_reset::token::LEN;
    let max_len = total_len_range.end() - stateless_reset::token::LEN;

    // Generate a random amount of unpredictable bits within the valid range
    // to further decrease the likelihood a stateless reset could be distinguished
    // from a valid packet.
    let len = gen_range(random_generator, min_len..=max_len);

    let mut unpredictatable_bits = vec![0u8; len];

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#10.3
    //# The remainder of the first byte
    //# and an arbitrary number of bytes following it are set to values that
    //# SHOULD be indistinguishable from random.
    random_generator.public_random_fill(&mut unpredictatable_bits);

    unpredictatable_bits
}

/// Generates a random usize within the given inclusive range. Note that this
/// will have slight bias towards the lower end of the range, but this bias
/// does not result in any reduction in security for this usage.
fn gen_range<R: random::Generator>(
    random_generator: &mut R,
    range: RangeInclusive<usize>,
) -> usize {
    if range.start() == range.end() {
        return *range.start();
    }

    let mut dest = [0; core::mem::size_of::<usize>()];
    random_generator.public_random_fill(&mut dest);
    let result = usize::from_be_bytes(dest);

    let max_variance = range.end() - range.start() + 1;
    range.start() + result % max_variance
}

impl EncoderValue for StatelessReset {
    fn encode<E: Encoder>(&self, encoder: &mut E) {
        // Write the short header tag over the first two bits
        (self.unpredictable_bits[0] >> TAG_OFFSET | TAG).encode(encoder);
        (&self.unpredictable_bits[1..]).encode(encoder);
        self.token.encode(encoder);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{path::MINIMUM_MTU, stateless_reset::token::testing::TEST_TOKEN_1};
    use s2n_codec::EncoderBuffer;

    #[test]
    fn gen_range_test() {
        let min = 100;
        let max = 1000;

        let mut generator = random::testing::Generator(123);

        for _ in 0..1000 {
            let result = gen_range(&mut generator, min..=max);
            assert!(result >= min);
            assert!(result <= max);
        }
    }

    #[test]
    fn generate_unpredictable_bits_test() {
        let min = 100;
        let max = 1000;

        let mut generator = random::testing::Generator(123);

        for _ in 0..1000 {
            let result = generate_unpredictable_bits(&mut generator, min..=max);
            assert!(result.len() + stateless_reset::token::LEN >= min);
            assert!(result.len() + stateless_reset::token::LEN <= max);
        }

        let bits_1 = generate_unpredictable_bits(&mut generator, min..=max);
        let bits_2 = generate_unpredictable_bits(&mut generator, min..=max);

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#10.3
        //= type=test
        //# The remainder of the first byte
        //# and an arbitrary number of bytes following it are set to values that
        //# SHOULD be indistinguishable from random.
        assert_ne!(bits_1, bits_2);
    }

    #[test]
    fn encode_test() {
        let max_tag_len = 16;
        let triggering_packet_len = 600;
        let mut generator = random::testing::Generator(123);

        let stateless_reset = StatelessReset::new(
            TEST_TOKEN_1,
            max_tag_len,
            triggering_packet_len,
            &mut generator,
        )
        .unwrap();

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#10.3
        //= type=test
        //# An endpoint MUST NOT send a stateless reset that is three times or
        //# more larger than the packet it receives to avoid being used for
        //# amplification.

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#10.3.3
        //= type=test
        //# An endpoint MUST ensure that every Stateless Reset that it sends is
        //# smaller than the packet that triggered it, unless it maintains state
        //# sufficient to prevent looping.
        assert!(stateless_reset.encoding_size() < triggering_packet_len);

        let mut buf = [0; MINIMUM_MTU as usize];
        let mut encoder = EncoderBuffer::new(&mut buf);

        stateless_reset.encode(&mut encoder);

        let packet_len = encoder.len();

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#10.3
        //= type=test
        //# Endpoints MUST send stateless reset packets formatted as a packet
        //# with a short header.
        assert!(matches!(&buf[0] >> 4, short_tag!()));

        assert_eq!(
            TEST_TOKEN_1.into_inner(),
            buf[packet_len - stateless_reset::token::LEN..packet_len]
        );
    }

    #[test]
    fn min_packet_test() {
        let max_tag_len = 16;
        let mut triggering_packet_len = SHORT_HEADER_LEN + max_tag_len + 1 + 1;
        let mut generator = random::testing::Generator(123);

        let stateless_reset = StatelessReset::new(
            TEST_TOKEN_1,
            max_tag_len,
            triggering_packet_len,
            &mut generator,
        );

        assert!(stateless_reset.is_some());

        triggering_packet_len -= 1;

        let stateless_reset = StatelessReset::new(
            TEST_TOKEN_1,
            max_tag_len,
            triggering_packet_len,
            &mut generator,
        );

        assert!(stateless_reset.is_none());
    }

    #[test]
    fn max_packet_test() {
        let max_tag_len = 16;
        let triggering_packet_len = (MINIMUM_MTU * 2) as usize;
        let mut generator = random::testing::Generator(123);

        let stateless_reset = StatelessReset::new(
            TEST_TOKEN_1,
            max_tag_len,
            triggering_packet_len,
            &mut generator,
        );

        assert!(stateless_reset.is_some());

        assert!(stateless_reset.unwrap().encoding_size() <= MINIMUM_MTU as usize);
    }
}

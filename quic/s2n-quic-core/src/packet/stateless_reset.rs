// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    connection,
    packet::{number::PacketNumberLen, Tag},
    random, stateless_reset,
};

//= https://www.rfc-editor.org/rfc/rfc9000#section-10.3
//# Stateless Reset {
//#   Fixed Bits (2) = 1,
//#   Unpredictable Bits (38..),
//#   Stateless Reset Token (128),
//# }

//= https://www.rfc-editor.org/rfc/rfc9000#section-10.3
//# Endpoints MUST send Stateless Resets formatted as a packet
//# with a short header.
const TAG: u8 = 0b0100_0000;
const TAG_OFFSET: u8 = 2;

// This value represents the minimum packet size of a packet that will be indistinguishable from
// valid QUIC version 1 packets, including one byte for the minimal payload and excluding the
// authentication tag (which may be variable and should be added to this constant). Since the
// connection ID length is either determined by a provider or by the peer, connection::id::MAX_LEN
// is used to ensure this value is valid no matter what length connection ID is used.
const MIN_INDISTINGUISHABLE_PACKET_LEN_WITHOUT_TAG: usize =
    core::mem::size_of::<Tag>() + PacketNumberLen::MAX_LEN + connection::id::MAX_LEN + 1;

/// Calculates the minimum packet length required such that a packet is indistinguishable from
/// other valid QUIC version 1 packets.
pub fn min_indistinguishable_packet_len(max_tag_len: usize) -> usize {
    MIN_INDISTINGUISHABLE_PACKET_LEN_WITHOUT_TAG + max_tag_len
}

/// Encodes a stateless reset packet into the given packet buffer.
pub fn encode_packet(
    token: stateless_reset::Token,
    max_tag_len: usize,
    triggering_packet_len: usize,
    random_generator: &mut dyn random::Generator,
    packet_buf: &mut [u8],
) -> Option<usize> {
    //= https://www.rfc-editor.org/rfc/rfc9000#section-10.3
    //# These values assume that the stateless reset token is the same length
    //# as the minimum expansion of the packet protection AEAD.  Additional
    //# unpredictable bytes are necessary if the endpoint could have
    //# negotiated a packet protection scheme with a larger minimum
    //# expansion.
    // The tag length for all cipher suites defined in TLS 1.3 is 16 bytes, but
    // we will calculate based on a given max tag length to allow for future cipher
    // suites with larger tags.
    let min_len = min_indistinguishable_packet_len(max_tag_len);

    //= https://www.rfc-editor.org/rfc/rfc9000#section-10.3
    //# An endpoint MUST NOT send a Stateless Reset that is three times or
    //# more larger than the packet it receives to avoid being used for
    //# amplification.

    //= https://www.rfc-editor.org/rfc/rfc9000#section-10.3.3
    //# An endpoint MUST ensure that every Stateless Reset that it sends is
    //# smaller than the packet that triggered it, unless it maintains state
    //# sufficient to prevent looping.
    let max_len = triggering_packet_len
        .saturating_sub(1)
        .min(packet_buf.len());

    // The packet that triggered this stateless reset was too small to send a stateless reset
    // that would be indistinguishable from a valid short header packet, so we'll just drop the
    // packet instead of sending a stateless reset.
    if max_len < min_len {
        return None;
    }

    // Generate unpredictable bits, leaving room for the stateless reset token
    let unpredictable_bits_min_len = min_len - stateless_reset::token::LEN;
    let unpredictable_bits_max_len = max_len - stateless_reset::token::LEN;

    let unpredictable_bits_len = generate_unpredictable_bits(
        random_generator,
        unpredictable_bits_min_len,
        &mut packet_buf[..unpredictable_bits_max_len],
    );
    // Write the short header tag over the first two bits
    packet_buf[0] = packet_buf[0] >> TAG_OFFSET | TAG;

    let packet_len = unpredictable_bits_len + stateless_reset::token::LEN;

    packet_buf[unpredictable_bits_len..packet_len].copy_from_slice(token.as_ref());

    if cfg!(debug_assertions) {
        assert!(packet_len >= min_len);
        assert!(packet_len <= max_len);
        assert!(packet_len < triggering_packet_len);
    }

    Some(packet_len)
}

/// Fills the given buffer with a random amount of random data at least of the
/// given `min_len`. Returns the length of the unpredictable bits that were generated.
fn generate_unpredictable_bits(
    random_generator: &mut dyn random::Generator,
    min_len: usize,
    buffer: &mut [u8],
) -> usize {
    // Generate a random amount of unpredictable bits within the valid range
    // to further decrease the likelihood a stateless reset could be distinguished
    // from a valid packet. This will have slight bias towards the lower end of the range,
    // but this bias does not result in any reduction in security for this usage and is actually
    // welcome as it results in reaching the minimal stateless reset size and thus
    // existing stateless reset loops sooner.
    let len = random::gen_range_biased(random_generator, min_len..=buffer.len());

    //= https://www.rfc-editor.org/rfc/rfc9000#section-10.3
    //# The remainder of the first byte
    //# and an arbitrary number of bytes following it are set to values that
    //# SHOULD be indistinguishable from random.
    random_generator.public_random_fill(&mut buffer[..len]);

    len
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{path::MINIMUM_MAX_DATAGRAM_SIZE, stateless_reset::token::testing::TEST_TOKEN_1};

    #[test]
    #[cfg_attr(miri, ignore)] // This test is too expensive for miri to complete in a reasonable amount of time
    fn generate_unpredictable_bits_test() {
        bolero::check!()
            .with_type::<(u8, u16, u16)>()
            .cloned()
            .for_each(|(seed, mut min, mut max)| {
                if min > max {
                    core::mem::swap(&mut min, &mut max);
                }
                let mut generator = random::testing::Generator(seed);
                let mut buffer = vec![0; max.into()];
                let len = generate_unpredictable_bits(&mut generator, min.into(), &mut buffer);
                assert!(len >= min.into());
                assert!(len <= max.into());
            });
    }

    //= https://www.rfc-editor.org/rfc/rfc9000#section-10.3
    //= type=test
    //# The remainder of the first byte
    //# and an arbitrary number of bytes following it are set to values that
    //# SHOULD be indistinguishable from random.
    #[test]
    fn unpredictable_bits_are_indistinguishable_from_random() {
        const MIN_LEN: usize = 100;
        const MAX_LEN: usize = 1000;

        let mut generator = random::testing::Generator(123);
        let mut buffer = [0; MAX_LEN];
        let mut buffer_2 = [0; MAX_LEN];
        generate_unpredictable_bits(&mut generator, MIN_LEN, &mut buffer);
        generate_unpredictable_bits(&mut generator, MIN_LEN, &mut buffer_2);

        assert_ne!(buffer[0..32], buffer_2[0..32]);
    }

    #[test]
    fn encode_packet_test() {
        let max_tag_len = 16;
        let triggering_packet_len = 600;
        let mut generator = random::testing::Generator(123);

        let mut buffer = [0; MINIMUM_MAX_DATAGRAM_SIZE as usize];

        let packet_len = encode_packet(
            TEST_TOKEN_1,
            max_tag_len,
            triggering_packet_len,
            &mut generator,
            &mut buffer,
        )
        .unwrap();

        //= https://www.rfc-editor.org/rfc/rfc9000#section-10.3
        //= type=test
        //# An endpoint MUST NOT send a Stateless Reset that is three times or
        //# more larger than the packet it receives to avoid being used for
        //# amplification.

        //= https://www.rfc-editor.org/rfc/rfc9000#section-10.3.3
        //= type=test
        //# An endpoint MUST ensure that every Stateless Reset that it sends is
        //# smaller than the packet that triggered it, unless it maintains state
        //# sufficient to prevent looping.
        assert!(packet_len < triggering_packet_len);

        //= https://www.rfc-editor.org/rfc/rfc9000#section-10.3
        //= type=test
        //# Endpoints MUST send Stateless Resets formatted as a packet
        //# with a short header.
        assert!(matches!(&buffer[0] >> 4, short_tag!()));

        assert_eq!(
            TEST_TOKEN_1.into_inner(),
            buffer[packet_len - stateless_reset::token::LEN..packet_len]
        );
    }

    #[test]
    fn min_packet_test() {
        let max_tag_len = 16;
        let mut triggering_packet_len = min_indistinguishable_packet_len(max_tag_len) + 1;
        let mut generator = random::testing::Generator(123);
        let mut buffer = [0; MINIMUM_MAX_DATAGRAM_SIZE as usize];

        let packet_len = encode_packet(
            TEST_TOKEN_1,
            max_tag_len,
            triggering_packet_len,
            &mut generator,
            &mut buffer,
        );

        assert_eq!(packet_len, Some(triggering_packet_len - 1));

        triggering_packet_len -= 1;

        let packet_len = encode_packet(
            TEST_TOKEN_1,
            max_tag_len,
            triggering_packet_len,
            &mut generator,
            &mut buffer,
        );

        assert!(packet_len.is_none());

        triggering_packet_len = 0;

        let packet_len = encode_packet(
            TEST_TOKEN_1,
            max_tag_len,
            triggering_packet_len,
            &mut generator,
            &mut buffer,
        );

        assert!(packet_len.is_none());
    }

    #[test]
    fn max_packet_test() {
        let max_tag_len = 16;
        let triggering_packet_len = (MINIMUM_MAX_DATAGRAM_SIZE * 2) as usize;
        let mut generator = random::testing::Generator(123);
        let mut buffer = [0; MINIMUM_MAX_DATAGRAM_SIZE as usize];

        let packet_len = encode_packet(
            TEST_TOKEN_1,
            max_tag_len,
            triggering_packet_len,
            &mut generator,
            &mut buffer,
        );

        assert!(packet_len.is_some());

        assert!(packet_len.unwrap() <= MINIMUM_MAX_DATAGRAM_SIZE as usize);
    }

    #[test]
    #[cfg_attr(miri, ignore)] // This test breaks in CI but can't be reproduced locally - https://github.com/aws/s2n-quic/issues/867
    fn packet_encoding_test() {
        let mut buffer = [0; MINIMUM_MAX_DATAGRAM_SIZE as usize];

        bolero::check!()
            .with_type::<(u8, usize, u16)>()
            .cloned()
            .for_each(|(seed, triggering_packet_len, max_tag_len)| {
                let mut generator = random::testing::Generator(seed);
                let packet_len = encode_packet(
                    TEST_TOKEN_1,
                    max_tag_len.into(),
                    triggering_packet_len,
                    &mut generator,
                    &mut buffer,
                );

                let min_len = MIN_INDISTINGUISHABLE_PACKET_LEN_WITHOUT_TAG + max_tag_len as usize;
                let max_len = triggering_packet_len.saturating_sub(1).min(buffer.len());

                if min_len <= max_len {
                    assert!(packet_len.is_some());
                    let packet_len = packet_len.unwrap();
                    assert!(packet_len <= max_len);
                    assert!(packet_len >= min_len);

                    //= https://www.rfc-editor.org/rfc/rfc9000#section-10.3
                    //= type=test
                    //# Endpoints MUST send Stateless Resets formatted as a packet
                    //# with a short header.
                    assert!(matches!(&buffer[0] >> 4, short_tag!()));

                    assert_eq!(
                        TEST_TOKEN_1.into_inner(),
                        buffer[packet_len - stateless_reset::token::LEN..packet_len]
                    );
                } else {
                    assert!(packet_len.is_none());
                }
            })
    }
}

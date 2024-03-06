// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    transport::parameters::{AckDelayExponent, MaxAckDelay},
    varint::VarInt,
};
use core::time::Duration;

// After running simulations, this seemed to be a good baseline
// TODO experiment more with this
/// The recommended value for the ack_elicitation_interval setting
const RECOMMENDED_ELICITATION_INTERVAL: u8 = 4;

// TODO experiment more with this
/// The recommended number of packet number ranges that an endpoint should store
const RECOMMENDED_RANGES_LIMIT: u8 = 10;

/// Settings for ACK frames
#[derive(Clone, Copy, Debug)]
pub struct Settings {
    /// The maximum ACK delay indicates the maximum amount of time by which the
    /// endpoint will delay sending acknowledgments.
    pub max_ack_delay: Duration,
    /// The ACK delay exponent is an integer value indicating an exponent used
    /// to decode the ACK Delay field in the ACK frame
    pub ack_delay_exponent: u8,

    //= https://www.rfc-editor.org/rfc/rfc9000#section-13.2.4
    //# A receiver that sends only non-ack-eliciting packets, such as ACK
    //# frames, might not receive an acknowledgement for a long period of
    //# time.  This could cause the receiver to maintain state for a large
    //# number of ACK frames for a long period of time, and ACK frames it
    //# sends could be unnecessarily large.  In such a case, a receiver could
    //# send a PING or other small ack-eliciting frame occasionally, such as
    //# once per round trip, to elicit an ACK from the peer.
    /// The number of packets received before sending an ACK-eliciting packet
    pub ack_elicitation_interval: u8,

    /// The number of packet number intervals an endpoint is willing to store
    pub ack_ranges_limit: u8,
}

impl Default for Settings {
    fn default() -> Self {
        Self::RECOMMENDED
    }
}

//= https://www.rfc-editor.org/rfc/rfc9000#section-19.3
//# ACK Delay:  A variable-length integer encoding the acknowledgement
//#    delay in microseconds; see Section 13.2.5.  It is decoded by
//#    multiplying the value in the field by 2 to the power of the
//#    ack_delay_exponent transport parameter sent by the sender of the
//#    ACK frame; see Section 18.2.  Compared to simply expressing the
//#    delay as an integer, this encoding allows for a larger range of
//#    values within the same number of bytes, at the cost of lower
//#    resolution.

impl Settings {
    //= https://www.rfc-editor.org/rfc/rfc9000#section-13.2.1
    //# An endpoint MUST acknowledge all ack-eliciting Initial and Handshake
    //# packets immediately
    pub const EARLY: Self = Self {
        max_ack_delay: Duration::from_secs(0),
        ack_delay_exponent: 0,
        ..Self::RECOMMENDED
    };

    pub const RECOMMENDED: Self = Self {
        max_ack_delay: MaxAckDelay::RECOMMENDED.as_duration(),
        ack_delay_exponent: AckDelayExponent::RECOMMENDED.as_u8(),
        ack_elicitation_interval: RECOMMENDED_ELICITATION_INTERVAL,
        ack_ranges_limit: RECOMMENDED_RANGES_LIMIT,
    };

    /// Decodes the peer's `Ack Delay` field
    pub fn decode_ack_delay(&self, delay: VarInt) -> Duration {
        Duration::from_micros(*delay) * self.scale()
    }

    /// Encodes the local `Ack Delay` field
    pub fn encode_ack_delay(&self, delay: Duration) -> VarInt {
        let micros = delay.as_micros();
        let scale = self.scale() as u128;
        (micros / scale).try_into().unwrap_or(VarInt::MAX)
    }

    /// Computes the scale from the exponent
    fn scale(&self) -> u32 {
        2u32.pow(self.ack_delay_exponent as u32)
    }
}

#[cfg(test)]
mod ack_settings_tests {
    use super::*;

    #[test]
    #[cfg_attr(miri, ignore)] // this test is too expensive for miri
    fn ack_settings_test() {
        for ack_delay_exponent in 0..=20 {
            let settings = Settings {
                ack_delay_exponent,
                ..Default::default()
            };
            // use an epsilon instead of comparing the values directly,
            // as there will be some precision loss
            let epsilon = settings.scale() as u128;

            for delay in (0..1000).map(|v| v * 100).map(Duration::from_micros) {
                let delay_varint = settings.encode_ack_delay(delay);
                let expected_us = delay.as_micros();
                let actual_us = settings.decode_ack_delay(delay_varint).as_micros();
                let actual_difference = expected_us - actual_us;
                assert!(actual_difference < epsilon);
            }

            // ensure MAX values are handled correctly and don't overflow
            let delay = settings.decode_ack_delay(VarInt::MAX);
            let delay_varint = settings.encode_ack_delay(delay);
            assert_eq!(VarInt::MAX, delay_varint);
        }
    }
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{contexts::WriteContext, transmission};
use s2n_quic_core::{
    ct::ConstantTimeEq,
    frame,
    time::{Timer, Timestamp},
};

pub type Data = [u8; frame::path_challenge::DATA_LEN];

#[derive(Clone, Debug)]
pub struct Challenge {
    state: State,
    abandon_timer: Timer,
    data: Data,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    /// A Challenge frame must be sent. The `u8` represents the remaining number of retries
    RequiresTransmission(u8),

    /// Challenged have been sent and we await a response until the abandon timer expires
    Idle,

    /// The Challenge has been abandoned due to the abandon_timer
    Abandoned,
}

impl Challenge {
    pub fn new(abandon: Timestamp, data: Data) -> Self {
        let mut abandon_timer = Timer::default();
        abandon_timer.set(abandon);

        Self {
            //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#8.2.1
            //# An endpoint SHOULD NOT probe a new path with packets containing a
            //# PATH_CHALLENGE frame more frequently than it would send an Initial
            //# packet.

            //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#8.2.1
            //# An endpoint MAY send multiple PATH_CHALLENGE frames to guard against
            //# packet loss.

            // Re-transmitting twice guards against packet loss, while remaining
            // below the amplification limit of 3.
            state: State::RequiresTransmission(2),
            abandon_timer,
            data,
        }
    }

    pub fn timers(&self) -> impl Iterator<Item = Timestamp> {
        self.abandon_timer.iter()
    }

    /// When a PATH_CHALLENGE is transmitted this handles any internal state operations.
    pub fn on_transmit<W: WriteContext>(&mut self, context: &mut W) {
        match self.state {
            State::RequiresTransmission(0) => self.state = State::Idle,
            State::RequiresTransmission(remaining) => {
                let frame = frame::PathChallenge { data: &self.data };

                //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#8.2.1
                //# However, an endpoint SHOULD NOT send multiple
                //# PATH_CHALLENGE frames in a single packet.
                if context.write_frame(&frame).is_some() {
                    let remaining = remaining - 1;
                    self.state = State::RequiresTransmission(remaining);
                }
            }
            _ => {}
        }
    }

    pub fn on_timeout(&mut self, timestamp: Timestamp) {
        if self.abandon_timer.poll_expiration(timestamp).is_ready() {
            self.state = State::Abandoned;
        }
    }

    pub fn is_pending(&self, timestamp: Timestamp) -> bool {
        !self.abandon_timer.is_expired(timestamp)
    }

    pub fn is_abandoned(&self) -> bool {
        self.state == State::Abandoned
    }

    pub fn is_valid(&self, data: &[u8]) -> bool {
        // 1 represents true. https://docs.rs/subtle/2.4.0/subtle/struct.Choice.html
        ConstantTimeEq::ct_eq(&self.data[..], &data).unwrap_u8() == 1
    }
}

impl transmission::interest::Provider for Challenge {
    fn transmission_interest(&self) -> transmission::Interest {
        if matches!(self.state, State::RequiresTransmission(_)) {
            transmission::Interest::NewData
        } else {
            transmission::Interest::None
        }
    }
}

#[cfg(any(test, feature = "testing"))]
pub mod testing {
    use super::*;
    use s2n_quic_core::time::{Clock, Duration, NoopClock};

    pub fn helper_challenge() -> Helper {
        let now = NoopClock {}.get_time();
        // let initial_transmit_time = now + Duration::from_millis(10);
        // let retransmit_period = Duration::from_millis(500);
        let abandon_duration = Duration::from_millis(10_000);
        let expected_data: [u8; 8] = [0; 8];

        let challenge = Challenge::new(now + abandon_duration, expected_data);

        Helper {
            now,
            abandon_duration,
            expected_data,
            challenge,
        }
    }

    #[allow(dead_code)]
    pub struct Helper {
        pub now: Timestamp,
        pub abandon_duration: Duration,
        pub expected_data: Data,
        pub challenge: Challenge,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contexts::testing::{MockWriteContext, OutgoingFrameBuffer};
    use s2n_quic_core::{endpoint, time::Duration};
    use testing::*;

    #[test]
    fn test_path_challenge_retransmited_2_times() {
        let helper = helper_challenge();

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#8.2.1
        //= type=test
        //# An endpoint SHOULD NOT probe a new path with packets containing a
        //# PATH_CHALLENGE frame more frequently than it would send an Initial
        //# packet.

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#8.2.1
        //= type=test
        //# An endpoint MAY send multiple PATH_CHALLENGE frames to guard against
        //# packet loss.
        assert_eq!(helper.challenge.state, State::RequiresTransmission(2));
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#8.2.1
    //= type=test
    //# An endpoint SHOULD NOT probe a new path with packets containing a
    //# PATH_CHALLENGE frame more frequently than it would send an Initial
    //# packet.

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#8.2.1
    //= type=test
    //# An endpoint MAY send multiple PATH_CHALLENGE frames to guard against
    //# packet loss.
    #[test]
    fn transmit_challenge_only_twice() {
        // Setup:
        let mut helper = helper_challenge();
        let mut frame_buffer = OutgoingFrameBuffer::new();
        let mut context = MockWriteContext::new(
            s2n_quic_platform::time::now(),
            &mut frame_buffer,
            transmission::Constraint::None,
            endpoint::Type::Client,
        );
        assert_eq!(helper.challenge.state, State::RequiresTransmission(2));

        // Trigger:
        helper.challenge.on_transmit(&mut context);

        // Expectation:
        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#8.2.1
        //= type=test
        //# However, an endpoint SHOULD NOT send multiple
        //# PATH_CHALLENGE frames in a single packet.
        assert_eq!(context.frame_buffer.len(), 1);

        assert_eq!(helper.challenge.state, State::RequiresTransmission(1));
        let written_data = match context.frame_buffer.pop_front().unwrap().as_frame() {
            frame::Frame::PathChallenge(frame) => Some(*frame.data),
            _ => None,
        }
        .unwrap();
        assert_eq!(written_data, helper.expected_data);

        // Trigger:
        helper.challenge.on_transmit(&mut context);

        // Expectation:
        assert_eq!(helper.challenge.state, State::RequiresTransmission(0));
        let written_data = match context.frame_buffer.pop_front().unwrap().as_frame() {
            frame::Frame::PathChallenge(frame) => Some(*frame.data),
            _ => None,
        }
        .unwrap();
        assert_eq!(written_data, helper.expected_data);

        // Trigger:
        helper.challenge.on_transmit(&mut context);

        // Expectation:
        assert_eq!(helper.challenge.state, State::Idle);
        assert_eq!(context.frame_buffer.len(), 0);
    }

    #[test]
    fn maintain_idle_and_dont_transmit_when_idle_state() {
        // Setup:
        let mut helper = helper_challenge();
        let mut frame_buffer = OutgoingFrameBuffer::new();
        let mut context = MockWriteContext::new(
            s2n_quic_platform::time::now(),
            &mut frame_buffer,
            transmission::Constraint::None,
            endpoint::Type::Client,
        );
        helper.challenge.state = State::Idle;
        assert_eq!(helper.challenge.state, State::Idle);

        // Trigger:
        helper.challenge.on_transmit(&mut context);

        // Expectation:
        assert_eq!(helper.challenge.state, State::Idle);
        assert_eq!(context.frame_buffer.len(), 0);
    }

    #[test]
    fn test_on_timeout() {
        let mut helper = helper_challenge();
        let expiration_time = helper.now + helper.abandon_duration;

        helper
            .challenge
            .on_timeout(expiration_time - Duration::from_millis(10));
        assert_eq!(helper.challenge.is_abandoned(), false);

        helper
            .challenge
            .on_timeout(expiration_time + Duration::from_millis(10));
        assert_eq!(helper.challenge.is_abandoned(), true);
    }

    #[test]
    fn challenge_must_remains_abandoned_once_abandoned() {
        let mut helper = helper_challenge();
        let expiration_time = helper.now + helper.abandon_duration;

        helper
            .challenge
            .on_timeout(expiration_time + Duration::from_millis(10));
        assert_eq!(helper.challenge.is_abandoned(), true);

        helper
            .challenge
            .on_timeout(expiration_time - Duration::from_millis(10));
        assert_eq!(helper.challenge.is_abandoned(), true);
    }

    #[test]
    fn test_is_pending() {
        let helper = helper_challenge();
        let expiration_time = helper.now + helper.abandon_duration;

        assert_eq!(
            helper
                .challenge
                .is_pending(expiration_time - Duration::from_millis(10)),
            true
        );
        assert_eq!(helper.challenge.is_pending(expiration_time), false);
        assert_eq!(
            helper
                .challenge
                .is_pending(expiration_time + Duration::from_millis(10)),
            false
        );
    }

    #[test]
    fn test_is_abandoned() {
        let mut helper = helper_challenge();
        let expiration_time = helper.now + helper.abandon_duration;

        assert_eq!(helper.challenge.is_abandoned(), false);

        helper
            .challenge
            .on_timeout(expiration_time + Duration::from_millis(10));
        assert_eq!(helper.challenge.is_abandoned(), true);
    }

    #[test]
    fn test_is_valid() {
        let helper = helper_challenge();

        assert!(helper.challenge.is_valid(&helper.expected_data));

        let wrong_data: [u8; 8] = [5; 8];
        assert_eq!(helper.challenge.is_valid(&wrong_data), false);
    }
}

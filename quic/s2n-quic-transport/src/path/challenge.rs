// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{contexts::WriteContext, transmission};
use s2n_quic_core::{
    ct::ConstantTimeEq,
    frame,
    time::{Duration, Timer, Timestamp},
};

pub type Data = [u8; frame::path_challenge::DATA_LEN];
const DISABLED_DATA: Data = [0; frame::path_challenge::DATA_LEN];

#[derive(Clone, Debug)]
pub struct Challenge {
    state: State,
    abandon_duration: Duration,
    abandon_timer: Timer,
    data: Data,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    /// PATH_CHALLENGE is not used for path validation. This is the case when initiating
    /// a new connection.
    Disabled,

    /// A Challenge frame must be sent. The `u8` represents the remaining number of retries
    RequiresTransmission(u8),

    /// Challenge has been sent and we are awaiting a response until the abandon timer expires
    Idle,

    /// The Challenge has been abandoned due to the abandon_timer
    Abandoned,

    /// When the PATH_CHALLENGE was validated by a PATH_RESPONSE
    Validated,
}

impl transmission::interest::Provider for State {
    fn transmission_interest(&self) -> transmission::Interest {
        match self {
            State::RequiresTransmission(_) => transmission::Interest::NewData,
            _ => transmission::Interest::None,
        }
    }
}

impl Challenge {
    pub fn new(abandon_duration: Duration, data: Data) -> Self {
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
            abandon_duration,
            abandon_timer: Timer::default(),
            data,
        }
    }

    pub fn disabled() -> Self {
        Self {
            state: State::Disabled,
            abandon_duration: Duration::default(),
            abandon_timer: Timer::default(),
            data: DISABLED_DATA,
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
                //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#8.2.1
                //# However, an endpoint SHOULD NOT send multiple
                //# PATH_CHALLENGE frames in a single packet.
                let frame = frame::PathChallenge { data: &self.data };

                if context.write_frame(&frame).is_some() {
                    let remaining = remaining - 1;
                    self.state = State::RequiresTransmission(remaining);

                    if !self.abandon_timer.is_armed() {
                        self.abandon_timer
                            .set(context.current_time() + self.abandon_duration);
                    }
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

    pub fn abandon(&mut self) {
        self.state = State::Abandoned
    }

    pub fn is_pending(&self) -> bool {
        match self.state {
            State::Idle | State::RequiresTransmission(_) => true,
            _ => false,
        }
    }

    pub fn on_validate(&mut self, data: &[u8]) -> bool {
        if ConstantTimeEq::ct_eq(&self.data[..], &data).into() {
            self.state = State::Validated;
            true
        } else {
            false
        }
    }
}

impl transmission::interest::Provider for Challenge {
    fn transmission_interest(&self) -> transmission::Interest {
        self.state.transmission_interest()
    }
}

#[cfg(any(test, feature = "testing"))]
pub mod testing {
    use super::*;
    use s2n_quic_core::time::{Clock, Duration, NoopClock};

    pub fn helper_challenge() -> Helper {
        let now = NoopClock {}.get_time();
        let abandon_duration = Duration::from_millis(10_000);
        let expected_data: [u8; 8] = [0; 8];

        let challenge = Challenge::new(abandon_duration, expected_data);

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

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#8.2.1
    //= type=test
    //# An endpoint MAY send multiple PATH_CHALLENGE frames to guard against
    //# packet loss.
    #[test]
    fn create_challenge_that_requires_two_transmissions() {
        let helper = helper_challenge();
        assert_eq!(helper.challenge.state, State::RequiresTransmission(2));
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#8.2.1
    //= type=test
    //# An endpoint SHOULD NOT probe a new path with packets containing a
    //# PATH_CHALLENGE frame more frequently than it would send an Initial
    //# packet.
    #[test]
    fn transmit_challenge_only_twice() {
        // Setup:
        let mut helper = helper_challenge();
        let mut frame_buffer = OutgoingFrameBuffer::new();
        let mut context = MockWriteContext::new(
            helper.now,
            &mut frame_buffer,
            transmission::Constraint::None,
            transmission::Mode::Normal,
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
    fn successful_on_transmit_arms_the_timer() {
        // Setup:
        let mut helper = helper_challenge();
        let mut frame_buffer = OutgoingFrameBuffer::new();
        let mut context = MockWriteContext::new(
            helper.now,
            &mut frame_buffer,
            transmission::Constraint::None,
            transmission::Mode::Normal,
            endpoint::Type::Client,
        );
        assert_eq!(helper.challenge.state, State::RequiresTransmission(2));
        assert!(!helper.challenge.abandon_timer.is_armed());

        // Trigger:
        helper.challenge.on_transmit(&mut context);

        // Expectation:
        assert!(helper.challenge.abandon_timer.is_armed());
    }

    #[test]
    fn maintain_idle_and_dont_transmit_when_idle_state() {
        // Setup:
        let mut helper = helper_challenge();
        let mut frame_buffer = OutgoingFrameBuffer::new();
        let mut context = MockWriteContext::new(
            helper.now,
            &mut frame_buffer,
            transmission::Constraint::None,
            transmission::Mode::Normal,
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

        let mut frame_buffer = OutgoingFrameBuffer::new();
        let mut context = MockWriteContext::new(
            helper.now,
            &mut frame_buffer,
            transmission::Constraint::None,
            transmission::Mode::Normal,
            endpoint::Type::Client,
        );
        helper.challenge.on_transmit(&mut context);

        helper
            .challenge
            .on_timeout(expiration_time - Duration::from_millis(10));
        assert!(!helper.challenge.is_abandoned());

        helper
            .challenge
            .on_timeout(expiration_time + Duration::from_millis(10));
        assert!(helper.challenge.is_abandoned());
    }

    #[test]
    fn challenge_must_remains_abandoned_once_abandoned() {
        let mut helper = helper_challenge();
        let expiration_time = helper.now + helper.abandon_duration;

        let mut frame_buffer = OutgoingFrameBuffer::new();
        let mut context = MockWriteContext::new(
            helper.now,
            &mut frame_buffer,
            transmission::Constraint::None,
            transmission::Mode::Normal,
            endpoint::Type::Client,
        );
        helper.challenge.on_transmit(&mut context);

        // Trigger:
        helper
            .challenge
            .on_timeout(expiration_time + Duration::from_millis(10));

        // Expectation:
        assert!(helper.challenge.is_abandoned());

        // Trigger:
        helper
            .challenge
            .on_timeout(expiration_time - Duration::from_millis(10));

        // Expectation:
        assert!(helper.challenge.is_abandoned());
    }

    #[test]
    fn test_is_abandoned() {
        let mut helper = helper_challenge();
        let expiration_time = helper.now + helper.abandon_duration;

        let mut frame_buffer = OutgoingFrameBuffer::new();
        let mut context = MockWriteContext::new(
            helper.now,
            &mut frame_buffer,
            transmission::Constraint::None,
            transmission::Mode::Normal,
            endpoint::Type::Client,
        );
        helper.challenge.on_transmit(&mut context);

        assert!(!helper.challenge.is_abandoned());

        helper
            .challenge
            .on_timeout(expiration_time + Duration::from_millis(10));
        assert!(helper.challenge.is_abandoned());
    }

    #[test]
    fn test_is_valid() {
        let helper = helper_challenge();

        assert!(helper.challenge.is_valid(&helper.expected_data));

        let wrong_data: [u8; 8] = [5; 8];
        assert!(!helper.challenge.is_valid(&wrong_data));
    }
}

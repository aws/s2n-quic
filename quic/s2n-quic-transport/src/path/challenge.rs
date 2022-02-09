// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{contexts::WriteContext, transmission};
use s2n_quic_core::{
    ct::ConstantTimeEq,
    event, frame,
    time::{timer, Duration, Timer, Timestamp},
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
    /// PATH_CHALLENGE is not used for path validation when initiating a new connection
    InitialPathDisabled,

    /// A Challenge frame must be sent. The `u8` represents the remaining number of retries
    RequiresTransmission(u8),

    /// Challenge has been sent and we are awaiting a response until the abandon timer expires
    PendingResponse,

    /// The Challenge has been abandoned due to the abandon_timer
    Abandoned,

    /// When the PATH_CHALLENGE was validated by a PATH_RESPONSE
    Validated,
}

impl transmission::interest::Provider for State {
    #[inline]
    fn transmission_interest<Q: transmission::interest::Query>(
        &self,
        query: &mut Q,
    ) -> transmission::interest::Result {
        match self {
            State::RequiresTransmission(_) => query.on_interest(transmission::Interest::NewData),
            _ => Ok(()),
        }
    }
}

impl Challenge {
    pub fn new(abandon_duration: Duration, data: Data) -> Self {
        Self {
            //= https://www.rfc-editor.org/rfc/rfc9000#8.2.1
            //# An endpoint SHOULD NOT probe a new path with packets containing a
            //# PATH_CHALLENGE frame more frequently than it would send an Initial
            //# packet.

            //= https://www.rfc-editor.org/rfc/rfc9000#8.2.1
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
            state: State::InitialPathDisabled,
            abandon_duration: Duration::ZERO,
            abandon_timer: Timer::default(),
            data: DISABLED_DATA,
        }
    }

    /// When a PATH_CHALLENGE is transmitted this handles any internal state operations.
    pub fn on_transmit<W: WriteContext>(&mut self, context: &mut W) {
        match self.state {
            State::RequiresTransmission(0) => self.state = State::PendingResponse,
            State::RequiresTransmission(remaining) => {
                //= https://www.rfc-editor.org/rfc/rfc9000#8.2.1
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

    pub fn on_timeout<Pub: event::ConnectionPublisher>(
        &mut self,
        timestamp: Timestamp,
        publisher: &mut Pub,
        path: event::builder::Path,
    ) {
        if self.abandon_timer.poll_expiration(timestamp).is_ready() {
            self.abandon(publisher, path);
        }
    }

    pub fn abandon<Pub: event::ConnectionPublisher>(
        &mut self,
        publisher: &mut Pub,
        path: event::builder::Path,
    ) {
        if self.is_pending() {
            self.state = State::Abandoned;
            self.abandon_timer.cancel();
            publisher.on_path_challenge_updated(event::builder::PathChallengeUpdated {
                path_challenge_status: event::builder::PathChallengeStatus::Abandoned,
                path,
                challenge_data: self.challenge_data(),
            });
        }
    }

    pub fn is_disabled(&self) -> bool {
        matches!(self.state, State::InitialPathDisabled)
    }

    pub fn is_pending(&self) -> bool {
        matches!(
            self.state,
            State::PendingResponse | State::RequiresTransmission(_)
        )
    }

    pub fn on_validated(&mut self, data: &[u8]) -> bool {
        if self.is_pending() && ConstantTimeEq::ct_eq(&self.data[..], data).into() {
            self.state = State::Validated;
            true
        } else {
            false
        }
    }

    pub fn challenge_data(&self) -> &[u8] {
        &self.data
    }
}

impl timer::Provider for Challenge {
    #[inline]
    fn timers<Q: timer::Query>(&self, query: &mut Q) -> timer::Result {
        self.abandon_timer.timers(query)?;

        Ok(())
    }
}

impl transmission::interest::Provider for Challenge {
    #[inline]
    fn transmission_interest<Q: transmission::interest::Query>(
        &self,
        query: &mut Q,
    ) -> transmission::interest::Result {
        self.state.transmission_interest(query)
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
    use s2n_quic_core::{
        endpoint,
        time::{Clock, Duration, NoopClock},
    };
    use testing::*;

    //= https://www.rfc-editor.org/rfc/rfc9000#8.2.1
    //= type=test
    //# An endpoint MAY send multiple PATH_CHALLENGE frames to guard against
    //# packet loss.
    #[test]
    fn create_challenge_that_requires_two_transmissions() {
        let helper = helper_challenge();
        assert_eq!(helper.challenge.state, State::RequiresTransmission(2));
    }

    #[test]
    fn create_disabled_challenge() {
        let challenge = Challenge::disabled();
        assert_eq!(challenge.state, State::InitialPathDisabled);
        assert!(!challenge.abandon_timer.is_armed());
    }

    //= https://www.rfc-editor.org/rfc/rfc9000#8.2.1
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
        //= https://www.rfc-editor.org/rfc/rfc9000#8.2.1
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
        assert_eq!(helper.challenge.state, State::PendingResponse);
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
    fn maintain_idle_and_dont_transmit_when_pending_response_state() {
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
        helper.challenge.state = State::PendingResponse;
        assert_eq!(helper.challenge.state, State::PendingResponse);

        // Trigger:
        helper.challenge.on_transmit(&mut context);

        // Expectation:
        assert_eq!(helper.challenge.state, State::PendingResponse);
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

        let mut publisher = event::testing::Publisher::snapshot();
        let path = event::builder::Path::test();

        helper.challenge.on_timeout(
            expiration_time - Duration::from_millis(10),
            &mut publisher,
            path,
        );
        assert!(helper.challenge.is_pending());

        let path = event::builder::Path::test();
        helper.challenge.on_timeout(
            expiration_time + Duration::from_millis(10),
            &mut publisher,
            path,
        );
        assert!(!helper.challenge.is_pending());
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

        let mut publisher = event::testing::Publisher::snapshot();
        let path = event::builder::Path::test();

        // Trigger:
        helper.challenge.on_timeout(
            expiration_time + Duration::from_millis(10),
            &mut publisher,
            path,
        );

        // Expectation:
        assert!(!helper.challenge.is_pending());

        let path = event::builder::Path::test();

        // Trigger:
        helper.challenge.on_timeout(
            expiration_time - Duration::from_millis(10),
            &mut publisher,
            path,
        );

        // Expectation:
        assert!(!helper.challenge.is_pending());
    }

    #[test]
    fn dont_abandon_disabled_state() {
        let mut challenge = Challenge::disabled();
        let now = NoopClock {}.get_time();

        let mut frame_buffer = OutgoingFrameBuffer::new();
        let mut context = MockWriteContext::new(
            now,
            &mut frame_buffer,
            transmission::Constraint::None,
            transmission::Mode::Normal,
            endpoint::Type::Client,
        );
        challenge.on_transmit(&mut context);

        assert_eq!(challenge.state, State::InitialPathDisabled);

        let mut publisher = event::testing::Publisher::snapshot();
        let path = event::builder::Path::test();

        let large_expiration_time = now + Duration::from_secs(1_000_000);
        challenge.on_timeout(large_expiration_time, &mut publisher, path);
        assert_eq!(challenge.state, State::InitialPathDisabled);
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

        let mut publisher = event::testing::Publisher::snapshot();
        let path = event::builder::Path::test();

        assert!(helper.challenge.is_pending());

        helper.challenge.on_timeout(
            expiration_time + Duration::from_millis(10),
            &mut publisher,
            path,
        );
        assert!(!helper.challenge.is_pending());
    }

    #[test]
    fn test_on_validate() {
        let mut helper = helper_challenge();

        let wrong_data: [u8; 8] = [5; 8];
        assert!(!helper.challenge.on_validated(&wrong_data));
        assert!(helper.challenge.is_pending());

        assert!(helper.challenge.on_validated(&helper.expected_data));
        assert_eq!(helper.challenge.state, State::Validated);
    }

    #[test]
    fn is_disabled() {
        let challenge = Challenge::disabled();

        assert_eq!(challenge.state, State::InitialPathDisabled);
        assert!(challenge.is_disabled());
    }

    #[test]
    fn dont_validate_disabled_state() {
        let mut helper = helper_challenge();
        helper.challenge.state = State::InitialPathDisabled;

        assert!(!helper.challenge.on_validated(&helper.expected_data));
        assert_eq!(helper.challenge.state, State::InitialPathDisabled);
    }

    #[test]
    fn dont_abandon_a_validated_challenge() {
        let mut helper = helper_challenge();
        helper.challenge.state = State::Validated;
        let mut publisher = event::testing::Publisher::snapshot();
        let path = event::builder::Path::test();

        helper.challenge.abandon(&mut publisher, path);

        assert_eq!(helper.challenge.state, State::Validated);
    }

    #[test]
    fn cancel_abandon_timer_on_abandon() {
        let mut helper = helper_challenge();
        let mut publisher = event::testing::Publisher::snapshot();
        let path = event::builder::Path::test();

        helper.challenge.abandon(&mut publisher, path);

        assert!(!helper.challenge.abandon_timer.is_armed());
    }
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::contexts::WriteContext;
use s2n_quic_core::{
    ct::ConstantTimeEq,
    frame,
    time::{Duration, Timer, Timestamp},
};

pub type Data = [u8; frame::path_challenge::DATA_LEN];

#[derive(Clone, Debug)]
pub struct Challenge {
    state: State,
    // retransmit_timer: Timer,
    // retransmit_period: Duration,
    abandon_timer: Timer,
    data: Data,
}

#[derive(Clone, Debug)]
pub enum State {
    /// A Challenge has been sent to the peer and the response is pending
    RequiresTransmission(u8),

    /// A timeout caused this Challenge to be abandoned, an new Challenge will have to be used
    Abandoned,
}

impl Challenge {
    pub fn new(
        abandon: Timestamp,
        data: Data,
    ) -> Self {
        let mut abandon_timer = Timer::default();
        abandon_timer.set(abandon);

        Self {
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
        // TODO check abandon, retransmit if left
    }

    pub fn on_timeout(&mut self, timestamp: Timestamp) {
        if self.abandon_timer.poll_expiration(timestamp).is_ready() {
            self.state = State::Abandoned;
        }
    }

    pub fn is_expired(&self, timestamp: Timestamp) -> bool {
        self.abandon_timer.is_expired(timestamp)
    }

    pub fn is_valid(&self, data: &[u8]) -> bool {
        ConstantTimeEq::ct_eq(&self.data[..], &data).unwrap_u8() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use s2n_quic_core::time::{Clock, Duration, NoopClock};

    #[test]
    fn test_is_valid() {
        let helper = helper_challenge();

        assert!(helper.challenge.is_valid(&helper.expected_data));

        let wrong_data: [u8; 8] = [5; 8];
        assert_ne!(helper.challenge.is_valid(&wrong_data));
    }

    // #[test]
    // fn is_pending_should_check_expiration_time() {
    //     let helper = helper_challenge();

    //     assert_eq!(helper.challenge.is_pending(helper.now), true);
    //     assert_eq!(
    //         helper
    //             .challenge
    //             .is_pending(helper.initial_transmit_time + Duration::from_millis(10)),
    //         false
    //     );
    // }

    // #[test]
    // fn cancelled_timer_should_not_be_pending() {
    //     let helper = helper_challenge();

    //     assert_eq!(helper.challenge.is_pending(helper.now), true);

    //     if let Challenge::Pending(mut state) = helper.challenge {
    //         state.retransmit_timer.cancel();
    //         assert_eq!(Challenge::Pending(state).is_pending(helper.now), false);
    //     } else {
    //         panic!("expected Pending");
    //     }
    // }

    fn helper_challenge() -> Helper {
        let now = NoopClock {}.get_time();
        // let initial_transmit_time = now + Duration::from_millis(10);
        // let retransmit_period = Duration::from_millis(500);
        let abandon_duration = Duration::from_millis(10_000);
        let expected_data: [u8; 8] = [0; 8];

        let challenge = Challenge::new(
            now + abandon_duration,
            expected_data,
        );

        Helper {
            now,
            abandon_duration,
            expected_data,
            challenge,
        }
    }

    #[allow(dead_code)]
    struct Helper {
        now: Timestamp,
        abandon_duration: Duration,
        expected_data: Data,
        challenge: Challenge,
    }
}

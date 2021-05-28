// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    ct::ConstantTimeEq,
    frame,
    time::{Duration, Timer, Timestamp},
};

pub type Data = [u8; frame::path_challenge::DATA_LEN];

#[derive(Clone, Debug)]
pub struct State {
    retransmit_timer: Timer,
    retransmit_period: Duration,
    abandon_timer: Timer,
    data: Data,
}

#[derive(Clone, Debug)]
pub enum Challenge {
    /// There is no Challenge associated with this Path
    None,

    /// A Challenge has been sent to the peer and the response is pending
    Pending(State),

    /// A timeout caused this Challenge to be abandoned, an new Challenge will have to be used
    Abandoned,
}

impl Challenge {
    pub fn new(
        expiration: Timestamp,
        retransmit_period: Duration,
        abandon_duration: Duration,
        data: Data,
    ) -> Self {
        let mut retransmit_timer = Timer::default();
        retransmit_timer.set(expiration);
        let mut abandon_timer = Timer::default();
        abandon_timer.set(expiration + abandon_duration);

        let state = State {
            retransmit_timer,
            retransmit_period,
            abandon_timer,
            data,
        };

        Self::Pending(state)
    }

    pub fn timers(&self) -> impl Iterator<Item = Timestamp> {
        if let Challenge::Pending(state) = self {
            Some(
                core::iter::empty()
                    .chain(state.abandon_timer.iter())
                    .chain(state.retransmit_timer.iter()),
            )
        } else {
            None
        }
        .into_iter()
        .flatten()
    }

    /// When a PATH_CHALLENGE is transmitted this handles any internal state operations.
    pub fn on_transmit(&mut self, timestamp: Timestamp) {
        if let Challenge::Pending(state) = self {
            state
                .retransmit_timer
                .set(timestamp + state.retransmit_period);
        }
    }

    pub fn on_timeout(&mut self, timestamp: Timestamp) {
        if let Challenge::Pending(state) = self {
            if state.abandon_timer.is_expired(timestamp) {
                *self = Challenge::Abandoned;
            }
        }
    }

    pub fn is_pending(&self, timestamp: Timestamp) -> bool {
        if let Challenge::Pending(state) = self {
            return state.retransmit_timer.is_armed()
                && !state.retransmit_timer.is_expired(timestamp);
        }

        false
    }

    pub fn data(&self) -> Option<&Data> {
        if let Challenge::Pending(state) = self {
            return Some(&state.data);
        }
        None
    }

    pub fn is_valid(&self, timestamp: Timestamp, data: &[u8]) -> bool {
        if let Challenge::Pending(state) = self {
            let mut valid = true;
            if state.abandon_timer.is_expired(timestamp) {
                valid = false;
            }

            if ConstantTimeEq::ct_eq(&state.data[..], &data).unwrap_u8() == 0 {
                valid = false;
            }

            return valid;
        }

        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::{Clock, Duration, NoopClock};

    #[test]
    fn test_challenge_validation() {
        let now = NoopClock {}.get_time();
        let expiration = now + Duration::from_millis(500);

        let expected_data: [u8; 8] = [0; 8];

        let challenge = Challenge::new(
            expiration,
            Duration::from_millis(0),
            Duration::from_millis(10_000),
            expected_data,
        );

        assert!(challenge.is_valid(now, &expected_data));

        let empty_challenge = Challenge::None;
        assert!(!empty_challenge.is_valid(now, &expected_data));

        let invalid_data = [1; 8];
        assert!(!challenge.is_valid(now, &invalid_data));

        let abandoned = Duration::from_millis(11_000);
        assert!(!challenge.is_valid(now + abandoned, &expected_data));
    }

    #[test]
    fn is_pending_should_check_expiration_time() {
        let now = NoopClock {}.get_time();
        let expiration = now + Duration::from_millis(500);
        let expected_data: [u8; 8] = [0; 8];

        let challenge = Challenge::new(
            expiration,
            Duration::from_millis(0),
            Duration::from_millis(10_000),
            expected_data,
        );

        assert_eq!(challenge.is_pending(now), true);
        assert_eq!(challenge.is_pending(now + Duration::from_millis(400)), true);
        assert_eq!(
            challenge.is_pending(now + Duration::from_millis(600)),
            false
        );
    }

    #[test]
    fn cancelled_timer_should_not_be_pending() {
        let now = NoopClock {}.get_time();
        let expiration = now + Duration::from_millis(500);
        let expected_data: [u8; 8] = [0; 8];

        let challenge = Challenge::new(
            expiration,
            Duration::from_millis(0),
            Duration::from_millis(10_000),
            expected_data,
        );

        assert_eq!(challenge.is_pending(now), true);

        if let Challenge::Pending(mut state) = challenge {
            state.retransmit_timer.cancel();
            assert_eq!(Challenge::Pending(state).is_pending(now), false);
        } else {
            panic!("expected Pending");
        }
    }
}

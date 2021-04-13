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
    None,
    Pending(State),
    Abandoned,
}

impl Default for Challenge {
    fn default() -> Self {
        Self::None
    }
}

impl PartialEq for Challenge {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Challenge::None, Challenge::None) => true,
            (Challenge::Abandoned, Challenge::Abandoned) => true,
            (Challenge::Pending(state), Challenge::Pending(other)) => {
                ConstantTimeEq::ct_eq(&state.data[..], &other.data[..]).unwrap_u8() == 1
            }
            _ => false,
        }
    }
}

impl Challenge {
    pub fn new(
        timestamp: Timestamp,
        retransmit_period: Duration,
        expiration: Duration,
        data: Data,
    ) -> Self {
        let mut retransmit_timer = Timer::default();
        retransmit_timer.set(timestamp);
        let mut abandon_timer = Timer::default();
        abandon_timer.set(timestamp + expiration);

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

    pub fn reset_timer(&mut self, timestamp: Timestamp) {
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
            return state.retransmit_timer.is_expired(timestamp);
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
        let clock = NoopClock {};
        let expiration = Duration::from_millis(100);
        let expected_data: [u8; 8] = [0; 8];

        let challenge = Challenge::new(
            clock.get_time(),
            Duration::from_millis(0),
            expiration,
            expected_data,
        );

        assert!(challenge.is_valid(clock.get_time(), &expected_data));

        let empty_challenge = Challenge::None;
        assert!(!empty_challenge.is_valid(clock.get_time(), &expected_data));

        let invalid_data = [1; 8];
        assert!(!challenge.is_valid(clock.get_time(), &invalid_data));

        let expired = Duration::from_millis(150);
        assert!(!challenge.is_valid(clock.get_time() + expired, &expected_data));
    }
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    ct::ConstantTimeEq,
    frame,
    inet::SocketAddress,
    time::{Duration, Timer, Timestamp},
};

pub type Data = [u8; frame::path_challenge::DATA_LEN];

#[derive(Clone, Debug)]
pub struct State {
    retransmit_timer: Timer,
    retransmit_period: Duration,
    abandon_timer: Timer,
    peer_address: SocketAddress,
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
                (ConstantTimeEq::ct_eq(&state.data[..], &other.data[..]).unwrap_u8() == 1)
                    && (state.peer_address == other.peer_address)
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
        peer_address: SocketAddress,
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
            peer_address,
            data,
        };

        Self::Pending(state)
    }

    pub fn next_timer(&self) -> Option<&Timestamp> {
        if let Challenge::Pending(state) = self {
            return core::iter::empty()
                .chain(state.abandon_timer.iter())
                .chain(state.retransmit_timer.iter())
                .min();
        }

        None
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#9.4
    //= type=TODO
    //# This timer SHOULD be set as described in Section 6.2.1 of
    //# [QUIC-RECOVERY] and MUST NOT be more aggressive.
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

    pub fn is_valid(&self, timestamp: Timestamp, addr: &SocketAddress, data: &[u8]) -> bool {
        if let Challenge::Pending(state) = self {
            let mut valid = true;
            if state.abandon_timer.is_expired(timestamp) {
                valid = false;
            }

            //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#8.2.2
            //# A PATH_RESPONSE frame MUST be sent on the network path where the
            //# PATH_CHALLENGE was received.
            if &state.peer_address != addr {
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
    use crate::{
        inet::SocketAddressV4,
        time::{Clock, Duration, NoopClock},
    };

    #[test]
    fn test_challenge_validation() {
        let clock = NoopClock {};
        let expiration = Duration::from_millis(100);
        let expected_data: [u8; 8] = [0; 8];

        let challenge = Challenge::new(
            clock.get_time(),
            Duration::from_millis(0),
            expiration,
            SocketAddress::default(),
            expected_data,
        );

        assert!(challenge.is_valid(clock.get_time(), &SocketAddress::default(), &expected_data));

        let empty_challenge = Challenge::None;
        assert!(!empty_challenge.is_valid(
            clock.get_time(),
            &SocketAddress::default(),
            &expected_data
        ));

        let invalid_data = [1; 8];
        assert!(!challenge.is_valid(clock.get_time(), &SocketAddress::default(), &invalid_data));

        let expired = Duration::from_millis(150);
        assert!(!challenge.is_valid(
            clock.get_time() + expired,
            &SocketAddress::default(),
            &expected_data
        ));

        let invalid_socket = SocketAddressV4::new([1, 1, 1, 1], 1024);
        assert!(!challenge.is_valid(
            clock.get_time(),
            &SocketAddress::from(invalid_socket),
            &expected_data
        ));
    }
}

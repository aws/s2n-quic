// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Allows applications to limit peer's ability to open new connections

pub use s2n_quic_core::endpoint::{
    limits::{ConnectionAttempt, Outcome},
    Limiter,
};
use s2n_quic_core::{event::Timestamp, path::THROTTLED_PORTS_LEN};

pub trait Provider: 'static {
    type Limits: 'static + Limiter;
    type Error: core::fmt::Display + Send + Sync;

    /// Starts the token provider
    fn start(self) -> Result<Self::Limits, Self::Error>;
}

use core::time::Duration;
pub use default::Limits as Default;

impl_provider_utils!();

impl<T: 'static + Limiter> Provider for T {
    type Limits = T;
    type Error = core::convert::Infallible;

    fn start(self) -> Result<Self::Limits, Self::Error> {
        Ok(self)
    }
}

const THROTTLED_PORT_LIMIT: usize = 10;
const THROTTLE_FREQUENCY: Duration = Duration::from_secs(1);

#[derive(Default, Debug, Clone, Copy)]
struct BasicRateLimiter {
    last_throttle_reset: Option<Timestamp>,
    count: usize,
}

impl BasicRateLimiter {
    /// Throttles a connection based on a limit.
    /// Returns True if the `should_throttle` invoke count is greater than `limit` and the
    /// connection has not been throttled in the last `throttle_frequency` duration.
    ///
    /// If the throttle timer expires the count is reset and `should_throttle` returns False.
    ///
    /// Returns False if the `should_throttle` invoke count is less than `limit`.
    fn should_throttle(
        &mut self,
        limit: usize,
        throttle_frequency: Duration,
        connection_attempt: &ConnectionAttempt,
    ) -> bool {
        self.count += 1;
        let timestamp = connection_attempt.timestamp;

        if self.count > limit {
            match self.last_throttle_reset {
                // If the throttle timer is still within the throttle_frequency
                // then throttle the connection.
                Some(last_throttle_reset)
                    if timestamp.saturating_duration_since(last_throttle_reset)
                        < throttle_frequency =>
                {
                    return true;
                }
                // If the throttle timer is greater than the throttle_frequency
                // then reset the throttle count and the throttle timer.
                // Let the connection through.
                _ => {
                    self.count = 0;
                    self.last_throttle_reset = Some(timestamp);
                    return false;
                }
            };
        }

        // If this is the first time calling instantiate the throttle timer.
        if self.last_throttle_reset.is_none() {
            self.last_throttle_reset = Some(timestamp);
        }

        false
    }
}

#[cfg(test)]
mod tests {
    use super::{BasicRateLimiter, THROTTLED_PORT_LIMIT, THROTTLE_FREQUENCY};
    use core::time::Duration;
    use s2n_quic_core::{
        endpoint::limits::ConnectionAttempt,
        event::IntoEvent,
        inet::SocketAddress,
        time::{testing::Clock as MockClock, Clock},
    };

    #[test]
    fn first_throttle_reset() {
        let remote_address = SocketAddress::default();
        let mock_clock = MockClock::default();
        let info =
            ConnectionAttempt::new(0, 0, &remote_address, mock_clock.get_time().into_event());

        let mut rate_limiter = BasicRateLimiter::default();
        // The first time the throttle limit is hit the timer will be created so we expect to be
        // able to connect THROTTLED_PORT_LIMIT amount of times before the connection is throttled.
        // Note: This test should run fast enough to not let the timer reset.
        let very_long_freq = Duration::MAX;

        for request in 0..(THROTTLED_PORT_LIMIT * 3) {
            if request >= THROTTLED_PORT_LIMIT {
                assert!(rate_limiter.should_throttle(THROTTLED_PORT_LIMIT, very_long_freq, &info));
            } else {
                assert!(!rate_limiter.should_throttle(THROTTLED_PORT_LIMIT, very_long_freq, &info));
            }
        }
    }

    #[test]
    fn throttle_timer_reset() {
        let remote_address = SocketAddress::default();
        let mut mock_clock = MockClock::default();

        let mut rate_limiter = BasicRateLimiter::default();
        let short_freq = Duration::from_millis(10);
        let sleep_longer_than_short_freq = Duration::from_millis(500);

        // This test should never throttle because everytime the limit is about to get hit the
        // thread sleeps long enough for the throttle reset timer to fire.
        for request in 0..(THROTTLED_PORT_LIMIT * 3) {
            let info =
                ConnectionAttempt::new(0, 0, &remote_address, mock_clock.get_time().into_event());
            if request % THROTTLED_PORT_LIMIT == 0 {
                mock_clock.inc_by(sleep_longer_than_short_freq)
            }
            assert!(!rate_limiter.should_throttle(THROTTLED_PORT_LIMIT, short_freq, &info));
        }
    }

    #[test]
    fn throttle_constants_changed() {
        // If the constants change consider modifying the above test cases to make sure we are
        // confident that we are hitting all the correct conditions.
        //
        // For example if we increase THROTTLE_FREQUENCY to a very large period, do the above
        // tests still make sense?
        assert_eq!(THROTTLED_PORT_LIMIT, 10);
        assert_eq!(THROTTLE_FREQUENCY, Duration::from_secs(1));
    }
}

pub mod default {
    //! Default provider for the endpoint limits.

    use super::*;
    use core::convert::Infallible;

    /// Allows the endpoint limits to be built with specific values
    ///
    /// # Examples
    ///
    /// Set the maximum inflight handshakes for this endpoint.
    ///
    /// ```rust
    /// use s2n_quic::provider::endpoint_limits;
    /// # use std::error::Error;
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn Error>> {
    /// let limits = endpoint_limits::Default::builder()
    ///     .with_inflight_handshake_limit(100)?
    ///     .build();
    ///
    ///     Ok(())
    /// # }
    /// ```
    #[derive(Default)]
    pub struct Builder {
        max_inflight_handshake_limit: Option<usize>,
    }

    impl Builder {
        /// Sets limit on inflight handshakes
        pub fn with_inflight_handshake_limit(mut self, limit: usize) -> Result<Self, Infallible> {
            self.max_inflight_handshake_limit = Some(limit);
            Ok(self)
        }

        /// Build the limits
        pub fn build(self) -> Result<Limits, Infallible> {
            Ok(Limits {
                max_inflight_handshake_limit: self.max_inflight_handshake_limit,
                rate_limiter: [BasicRateLimiter::default(); THROTTLED_PORTS_LEN],
            })
        }
    }

    #[derive(Clone, Copy, Debug)]
    pub struct Limits {
        /// Maximum number of handshakes to allow before Retry packets are queued
        max_inflight_handshake_limit: Option<usize>,
        rate_limiter: [BasicRateLimiter; THROTTLED_PORTS_LEN],
    }

    impl Limits {
        pub fn builder() -> Builder {
            Builder::default()
        }
    }

    /// Default implementation for the Limits
    impl super::Limiter for Limits {
        fn on_connection_attempt(&mut self, info: &ConnectionAttempt) -> Outcome {
            let remote_port = info.remote_address.port();
            if s2n_quic_core::path::remote_port_blocked(remote_port) {
                return Outcome::drop();
            }

            if let Some(port_index) = s2n_quic_core::path::remote_port_throttled_index(remote_port)
            {
                let rate_limiter = &mut self.rate_limiter[port_index];
                if rate_limiter.should_throttle(THROTTLED_PORT_LIMIT, THROTTLE_FREQUENCY, info) {
                    return Outcome::drop();
                }
            }

            if let Some(limit) = self.max_inflight_handshake_limit {
                if info.inflight_handshakes >= limit {
                    return Outcome::retry();
                }
            }

            Outcome::allow()
        }
    }

    /// Default limit values are as non-intrusive as possible
    impl std::default::Default for Limits {
        fn default() -> Self {
            Self {
                max_inflight_handshake_limit: None,
                rate_limiter: [BasicRateLimiter::default(); THROTTLED_PORTS_LEN],
            }
        }
    }

    #[test]
    fn builder_test() {
        let elp = Limits::builder()
            .with_inflight_handshake_limit(100)
            .unwrap()
            .build()
            .unwrap();
        assert_eq!(elp.max_inflight_handshake_limit, Some(100));
    }

    #[test]
    fn blocked_port_connection_attempt() {
        use s2n_quic_core::{
            event::IntoEvent,
            inet::SocketAddress,
            time::{testing::Clock as MockClock, Clock},
        };

        let mut remote_address = SocketAddress::default();
        let mut limits = Limits::builder().build().unwrap();
        let mock_clock = MockClock::default();

        for port in 0..u16::MAX {
            let blocked_expected = s2n_quic_core::path::remote_port_blocked(port);

            remote_address.set_port(port);
            let info =
                ConnectionAttempt::new(0, 0, &remote_address, mock_clock.get_time().into_event());
            let outcome = limits.on_connection_attempt(&info);

            if blocked_expected {
                assert_eq!(Outcome::drop(), outcome);
            } else {
                assert_eq!(Outcome::allow(), outcome);
            }
        }
    }
}

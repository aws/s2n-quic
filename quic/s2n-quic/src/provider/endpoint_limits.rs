// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Allows applications to limit peer's ability to open new connections

pub use s2n_quic_core::endpoint::{
    limits::{ConnectionAttemptOutcome, Context},
    Limiter,
};

pub trait Provider: 'static {
    type Limits: 'static + Limiter;
    type Error: core::fmt::Display;

    /// Starts the token provider
    fn start(self) -> Result<Self::Limits, Self::Error>;
}

pub use default::Limits as Default;

impl_provider_utils!();

impl<T: 'static + Limiter> Provider for T {
    type Limits = T;
    type Error = core::convert::Infallible;

    fn start(self) -> Result<Self::Limits, Self::Error> {
        Ok(self)
    }
}

pub mod default {
    //! Default provider for the endpoint limits.

    use super::*;
    use core::{convert::Infallible, time::Duration};
    use s2n_quic_core::endpoint::limits::LimitViolationOutcome;

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
    ///     .with_retry_delay(core::time::Duration::from_millis(100))?
    ///     .build();
    ///
    ///     Ok(())
    /// # }
    /// ```
    pub struct Builder {
        retry_delay: Duration,
        max_inflight_handshake_limit: Option<usize>,
        min_transfer_bytes_per_second: usize,
        min_transfer_rate_connection_count_close_threshold: usize,
    }

    impl std::default::Default for Builder {
        fn default() -> Self {
            Self {
                retry_delay: Duration::from_millis(0),
                max_inflight_handshake_limit: None,
                min_transfer_bytes_per_second: 0,
                min_transfer_rate_connection_count_close_threshold: 0,
            }
        }
    }

    impl Builder {
        /// Sets limit on inflight handshakes
        pub fn with_inflight_handshake_limit(mut self, limit: usize) -> Result<Self, Infallible> {
            self.max_inflight_handshake_limit = Some(limit);
            Ok(self)
        }

        /// Sets the delay when sending Retry packets
        pub fn with_retry_delay(mut self, delay: Duration) -> Result<Self, Infallible> {
            self.retry_delay = delay;
            Ok(self)
        }

        pub fn with_min_transfer_bytes_per_second(
            mut self,
            limit: usize,
        ) -> Result<Self, Infallible> {
            self.min_transfer_bytes_per_second = limit;
            Ok(self)
        }

        pub fn with_min_transfer_rate_connection_count_close_threshold(
            mut self,
            limit: usize,
        ) -> Result<Self, Infallible> {
            self.min_transfer_rate_connection_count_close_threshold = limit;
            Ok(self)
        }

        /// Build the limits
        pub fn build(self) -> Result<Limits, Infallible> {
            if self.min_transfer_rate_connection_count_close_threshold > 0 {
                // TODO: Use a new type around Error instead of panicking
                assert!(
                    self.min_transfer_bytes_per_second > 0,
                    "min_transfer_bytes_per_second should be \
                configured if min_transfer_rate_connection_count_close_threshold is"
                );
            }

            Ok(Limits {
                retry_delay: self.retry_delay,
                max_inflight_handshake_limit: self.max_inflight_handshake_limit,
                min_transfer_bytes_per_second: self.min_transfer_bytes_per_second,
                min_transfer_rate_connection_count_close_threshold: self
                    .min_transfer_rate_connection_count_close_threshold,
            })
        }
    }

    #[derive(Clone, Copy, Debug)]
    pub struct Limits {
        /// Amount of time to wait before sending a Retry packet
        retry_delay: Duration,

        /// Maximum number of handshakes to allow before Retry packets are queued
        max_inflight_handshake_limit: Option<usize>,

        /// The minimum transfer rate a connection must maintain
        min_transfer_bytes_per_second: usize,

        /// If the transfer rate for a connection drops below the configured `min_transfer_bytes_per_second`
        /// and the number of open connections is above the `min_transfer_rate_connection_count_close_threshold`,
        /// the connection will be closed.
        min_transfer_rate_connection_count_close_threshold: usize,
    }

    impl Limits {
        pub fn builder() -> Builder {
            Builder::default()
        }
    }

    /// Default implementation for the Limits
    impl super::Limiter for Limits {
        fn on_connection_attempt(&mut self, info: &Context) -> ConnectionAttemptOutcome {
            if let Some(limit) = self.max_inflight_handshake_limit {
                if info.inflight_handshakes >= limit {
                    return ConnectionAttemptOutcome::Retry {
                        delay: self.retry_delay,
                    };
                }
            }

            ConnectionAttemptOutcome::Allow
        }

        fn on_min_transfer_rate_violation(&mut self, info: &Context) -> LimitViolationOutcome {
            if self.min_transfer_rate_connection_count_close_threshold > info.connection_count {
                LimitViolationOutcome::Close
            } else {
                LimitViolationOutcome::Ignore
            }
        }

        fn min_transfer_bytes_per_second(&self) -> usize {
            self.min_transfer_bytes_per_second
        }
    }

    /// Default limit values are as non-intrusive as possible
    impl std::default::Default for Limits {
        fn default() -> Self {
            Self {
                retry_delay: Duration::from_millis(0),
                max_inflight_handshake_limit: None,
                min_transfer_bytes_per_second: 0,
                min_transfer_rate_connection_count_close_threshold: 0,
            }
        }
    }

    #[test]
    fn builder_test() {
        let elp = Limits::builder()
            .with_inflight_handshake_limit(100)
            .unwrap()
            .with_retry_delay(Duration::from_millis(100))
            .unwrap()
            .with_min_transfer_bytes_per_second(1000)
            .unwrap()
            .with_min_transfer_rate_connection_count_close_threshold(100000)
            .unwrap()
            .build()
            .unwrap();
        assert_eq!(elp.max_inflight_handshake_limit, Some(100));
        assert_eq!(elp.retry_delay, Duration::from_millis(100));
        assert_eq!(elp.min_transfer_bytes_per_second, 1000);
        assert_eq!(
            elp.min_transfer_rate_connection_count_close_threshold,
            100000
        );
    }
}

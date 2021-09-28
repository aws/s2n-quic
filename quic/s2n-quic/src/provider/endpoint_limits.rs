// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Allows applications to limit peer's ability to open new connections

pub use s2n_quic_core::endpoint::{
    limits::{ConnectionAttempt, Outcome},
    Limiter,
};

pub trait Provider: 'static {
    type Limits: 'static + Limiter;
    type Error: core::fmt::Display;

    /// Starts the token provider
    fn start(self) -> Result<Self::Limits, Self::Error>;
}

pub use default::Provider as Default;

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
    }

    impl std::default::Default for Builder {
        fn default() -> Self {
            Self {
                retry_delay: Duration::from_millis(0),
                max_inflight_handshake_limit: None,
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

        /// Build the limits
        pub fn build(self) -> Result<Limits, Infallible> {
            Ok(Limits {
                retry_delay: self.retry_delay,
                max_inflight_handshake_limit: self.max_inflight_handshake_limit,
            })
        }
    }

    #[derive(Clone, Copy, Debug)]
    pub struct Limits {
        /// Amount of time to wait before sending a Retry packet
        retry_delay: Duration,

        /// Maximum number of handshakes to allow before Retry packets are queued
        max_inflight_handshake_limit: Option<usize>,
    }

    /// Default implementation for the Limits
    impl super::Limiter for Limits {
        fn on_connection_attempt(&mut self, info: &ConnectionAttempt) -> Outcome {
            if let Some(limit) = self.max_inflight_handshake_limit {
                if info.inflight_handshakes >= limit {
                    return Outcome::Retry {
                        delay: self.retry_delay,
                    };
                }
            }

            Outcome::Allow
        }
    }

    /// Default limit values are as non-intrusive as possible
    impl std::default::Default for Limits {
        fn default() -> Self {
            Self {
                retry_delay: Duration::from_millis(0),
                max_inflight_handshake_limit: None,
            }
        }
    }

    #[derive(Debug, Default)]
    pub struct Provider(Limits);

    impl super::Provider for Provider {
        type Limits = Limits;
        type Error = Infallible;

        fn start(self) -> Result<Self::Limits, Self::Error> {
            Ok(self.0)
        }
    }

    impl Provider {
        pub fn builder() -> Builder {
            Builder::default()
        }
    }

    #[test]
    fn builder_test() {
        let elp = Provider::builder()
            .with_inflight_handshake_limit(100)
            .unwrap()
            .with_retry_delay(Duration::from_millis(100))
            .unwrap()
            .build()
            .unwrap();
        assert_eq!(elp.max_inflight_handshake_limit, Some(100));
        assert_eq!(elp.retry_delay, Duration::from_millis(100));
    }
}

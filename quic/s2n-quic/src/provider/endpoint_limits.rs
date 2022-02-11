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
            })
        }
    }

    #[derive(Clone, Copy, Debug)]
    pub struct Limits {
        /// Maximum number of handshakes to allow before Retry packets are queued
        max_inflight_handshake_limit: Option<usize>,
    }

    impl Limits {
        pub fn builder() -> Builder {
            Builder::default()
        }
    }

    /// Default implementation for the Limits
    impl super::Limiter for Limits {
        fn on_connection_attempt(&mut self, info: &ConnectionAttempt) -> Outcome {
            if let Some(limit) = self.max_inflight_handshake_limit {
                if info.inflight_handshakes >= limit {
                    return Outcome::retry();
                }
            }

            Outcome::Allow
        }
    }

    /// Default limit values are as non-intrusive as possible
    impl std::default::Default for Limits {
        fn default() -> Self {
            Self {
                max_inflight_handshake_limit: None,
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
}

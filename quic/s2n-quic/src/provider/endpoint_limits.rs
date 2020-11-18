//! Default provider for the endpoint limits.
//!

use s2n_quic_core::endpoint;

pub trait Provider: 'static {
    type Limits: 'static + endpoint::Limits;
    type Error: core::fmt::Display;

    /// Starts the token provider
    fn start(self) -> Result<Self::Limits, Self::Error>;
}

pub use default::Provider as Default;

impl_provider_utils!();

pub mod default {
    use core::convert::Infallible;
    use s2n_quic_core::{endpoint::limits, time};

    /// Allows the endpoint limits to be built with specific values
    ///
    /// # Examples
    ///
    /// Set the maximum inflight handshakes for this endpoint.
    ///
    /// ```rust
    /// use s2n_quic::provider::endpoint_limits;
    /// # use std::error::Error;
    /// # use s2n_quic_core::time;
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn Error>> {
    /// let limits = endpoint_limits::Default::builder()
    ///     .with_inflight_handshake_limit(100)?
    ///     .with_retry_backoff(time::Duration::from_millis(100))?
    ///     .build();
    ///
    ///     Ok(())
    /// # }
    /// ```
    pub struct Builder {
        retry_backoff: time::Duration,
        max_inflight_handshake_limit: Option<usize>,
    }

    impl std::default::Default for Builder {
        fn default() -> Self {
            Self {
                retry_backoff: time::Duration::from_millis(0),
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

        /// Sets the backoff time when sending Retry packets
        pub fn with_retry_backoff(mut self, backoff: time::Duration) -> Result<Self, Infallible> {
            self.retry_backoff = backoff;
            Ok(self)
        }

        /// Build the limits
        pub fn build(self) -> Result<Limits, Infallible> {
            Ok(Limits {
                retry_backoff: self.retry_backoff,
                max_inflight_handshake_limit: self.max_inflight_handshake_limit,
            })
        }
    }

    #[derive(Clone, Copy, Debug)]
    pub struct Limits {
        /// Amount of time to wait before sending a Retry packet
        retry_backoff: time::Duration,

        /// Maximum number of handshakes to allow before Retry packets are queued
        max_inflight_handshake_limit: Option<usize>,
    }

    /// Default implementation for the Limits
    impl super::endpoint::Limits for Limits {
        fn on_connection_attempt(&mut self, info: &limits::ConnectionAttempt) -> limits::Outcome {
            if let Some(limit) = self.max_inflight_handshake_limit {
                if info.inflight_handshakes > limit {
                    return limits::Outcome::Retry {
                        delay: self.retry_backoff,
                    };
                }
            }

            limits::Outcome::Allow
        }
    }

    /// Default limit values are as non-intrusive as possible
    impl std::default::Default for Limits {
        fn default() -> Self {
            Self {
                retry_backoff: time::Duration::from_millis(0),
                max_inflight_handshake_limit: None,
            }
        }
    }

    impl super::TryInto for Limits {
        type Provider = Provider;
        type Error = Infallible;

        fn try_into(self) -> Result<Self::Provider, Self::Error> {
            Ok(Provider(self))
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
            .with_retry_backoff(time::Duration::from_millis(100))
            .unwrap()
            .build()
            .unwrap();
        assert_eq!(elp.max_inflight_handshake_limit, Some(100));
        assert_eq!(elp.retry_backoff, time::Duration::from_millis(100));
    }
}

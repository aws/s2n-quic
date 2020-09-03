use cfg_if::cfg_if;
use core::{future::Future, time::Duration};

/// Provides runtime support for an endpoint
pub trait Provider {
    type Environment: Environment;
    type Error: core::fmt::Display;

    /// Starts the runtime with the given future
    fn start<Start, Fut>(self, start: Start) -> Result<(), Self::Error>
    where
        Start: FnOnce(Self::Environment) -> Fut,
        Fut: 'static + Future<Output = ()> + Send;
}

/// Provides functionality for the runtime environment
pub trait Environment: Send + 'static {
    type Delay: 'static + Future<Output = ()> + Send;

    /// Returns a future that delays for `duration`
    fn delay(&self, duration: Duration) -> Self::Delay;
}

cfg_if! {
    if #[cfg(feature = "tokio")] {
        pub use self::tokio as default;
    } else {
        pub mod default {
            // TODO export stub implementation that panics on initialization
        }
    }
}

pub use default::Provider as Default;

#[cfg(feature = "tokio")]
pub mod tokio;

impl_provider_utils!();

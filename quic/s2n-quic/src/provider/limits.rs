use core::{convert::Infallible, time::Duration};

/// Provides limits support for an endpoint
pub trait Provider {
    // TODO
}

#[derive(Debug, Default)]
pub struct Default {
    // TODO
}

impl Provider for Default {}

#[derive(Debug, Default)]
pub struct Builder {
    // TODO
    limits: Default,
}

impl Builder {
    pub fn with_max_idle_time(self, duration: Duration) -> Result<Self, Infallible> {
        let _ = duration;
        Ok(self)
    }

    pub fn build(self) -> Result<Default, Infallible> {
        Ok(self.limits)
    }
}

impl_provider_utils!();

use core::{convert::Infallible, time::Duration};

/// Provides Limiting support for an endpoint
pub trait Provider {
    // TODO
}

impl Provider for Limits {
    // add code here
}

impl_provider_utils!();

#[derive(Debug, Default)]
pub struct Limits {
    max_idle_time: Option<Duration>,
}

impl Limits {
    pub fn builder() -> Builder {
        Builder::default()
    }
}

#[derive(Debug, Default)]
pub struct Builder(Limits);

impl Builder {
    pub fn with_max_idle_time(mut self, max_idle_time: Duration) -> Result<Self, Infallible> {
        self.0.max_idle_time = Some(max_idle_time);
        Ok(self)
    }

    pub fn build(self) -> Result<Limits, Infallible> {
        Ok(self.0)
    }
}

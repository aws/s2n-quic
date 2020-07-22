/// Provides runtime support for an endpoint
pub trait Provider {
    // TODO
}

#[derive(Debug, Default)]
pub struct Default {
    // TODO
}

impl Provider for Default {}

#[cfg(feature = "tokio")]
pub use crate::tokio::Runtime as Tokio;

impl_provider_utils!();

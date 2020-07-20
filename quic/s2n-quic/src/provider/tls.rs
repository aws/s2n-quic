use std::io;

/// Provides TLS support for an endpoint
pub trait Provider {
    // TODO
}

#[derive(Debug, Default)]
pub struct Default {
    // TODO
}

impl Provider for Default {}

impl TryInto for &str {
    type Error = io::Error;
    type Provider = Default;

    fn try_into(self) -> Result<Self::Provider, Self::Error> {
        Ok(Default::default())
    }
}

impl_provider_utils!();

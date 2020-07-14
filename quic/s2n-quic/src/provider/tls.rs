use std::io::Error as IOError;

/// Provides TLS support for an endpoint
pub trait Provider {
    // TODO
}

impl Provider for usize {
    // add code here
}

impl TryInto for &str {
    type Error = IOError;
    type Provider = usize;

    fn try_into(self) -> Result<Self::Provider, Self::Error> {
        // let _key = std::fs::read_file(self)?;
        Ok(0)
    }
}

impl_provider_utils!();

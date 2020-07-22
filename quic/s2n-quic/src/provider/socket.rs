use std::{io, net::ToSocketAddrs};

/// Provides socket support for an endpoint
pub trait Provider {
    // TODO
}

#[derive(Debug, Default)]
pub struct Default {
    // TODO
}

impl Provider for Default {}

impl TryInto for u16 {
    type Error = io::Error;
    type Provider = Default;

    fn try_into(self) -> Result<Self::Provider, Self::Error> {
        // let _addrs = self.to_socket_addrs()?;
        Ok(Default::default())
    }
}

impl TryInto for &str {
    type Error = io::Error;
    type Provider = Default;

    fn try_into(self) -> Result<Self::Provider, Self::Error> {
        let _addrs = self.to_socket_addrs()?;
        Ok(Default::default())
    }
}

impl_provider_utils!();

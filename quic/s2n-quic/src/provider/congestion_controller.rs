/// Provides congestion controller support for an endpoint
pub trait Provider {
    // TODO
}

#[derive(Debug, Default)]
pub struct Default {
    // TODO
}

impl Provider for Default {}

#[derive(Default, Debug)]
pub struct Cubic {}

impl Provider for Cubic {}

#[derive(Default, Debug)]
pub struct Reno {}

impl Provider for Reno {}

impl_provider_utils!();

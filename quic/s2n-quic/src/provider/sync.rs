/// Provides synchronization support for an endpoint
pub trait Provider {
    // TODO
}

#[derive(Debug, Default)]
pub struct Default {
    // TODO
}

impl Provider for Default {}

#[derive(Debug, Default)]
pub struct Mutex {
    // TODO
}

impl Provider for Mutex {}

impl_provider_utils!();

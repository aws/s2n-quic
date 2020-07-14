/// Provides Congestion Controller support for an endpoint
pub trait Provider {
    // TODO
}

/// Converts `usize` into an initial congestion window
impl Provider for usize {
    // add code here
}

impl_provider_utils!();

#[derive(Debug, Default)]
pub struct Reno {
    // TODO
}

impl Provider for Reno {}

#[derive(Debug, Default)]
pub struct Cubic {
    // TODO
}

impl Provider for Cubic {}

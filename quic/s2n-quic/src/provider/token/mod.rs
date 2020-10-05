//! Default provider for Address Validation
//!
//! Customers will use the default Provider to generate and verify address validation tokens. This
//! means the actual token does not need to be exposed.

use cfg_if::cfg_if;
pub use s2n_quic_core::token::Format;

pub trait Provider: 'static {
    type Format: 'static + Format;
    type Error: core::fmt::Display;

    /// Starts the token provider
    fn start(&self) -> Result<Self::Format, Self::Error>;

    /// Called to update the key used to sign and verify tokens
    fn update_key(&self) -> [u8; 32];
}

cfg_if! {
    if #[cfg(feature = "default-token-provider")] {
        pub mod default;
    } else {
        pub mod default {
            // TODO export stub implementation that panics on initialization
        }
    }
}

pub use default::Provider as Default;

impl_provider_utils!();

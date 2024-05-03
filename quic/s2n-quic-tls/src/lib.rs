// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use s2n_quic_core::application::ServerName;

/// Ensure memory is correctly managed in tests
#[cfg(test)]
#[global_allocator]
static ALLOCATOR: checkers::Allocator = checkers::Allocator::system();

#[cfg(s2n_quic_enable_pq_tls)]
static DEFAULT_POLICY: &s2n_tls::security::Policy = &s2n_tls::security::TESTING_PQ;
#[cfg(not(s2n_quic_enable_pq_tls))]
static DEFAULT_POLICY: &s2n_tls::security::Policy = &s2n_tls::security::DEFAULT_TLS13;

// https://aws.github.io/s2n-tls/usage-guide/ch06-security-policies.html
// "20230317" is the recommended FIPS compliant security policy that supports TLS1.3
#[cfg(feature = "fips")]
static DEFAULT_FIPS_POLICY: &s2n_tls::security::Policy = &Policy::from_version("20230317");

#[non_exhaustive]
pub struct ConnectionContext<'a> {
    pub server_name: Option<&'a ServerName>,
}

/// Loads a config for a given connection
///
/// This trait can be implemented to override the default config loading for a QUIC endpoint
pub trait ConfigLoader: 'static + Send {
    fn load(&mut self, cx: ConnectionContext) -> config::Config;
}

impl ConfigLoader for config::Config {
    #[inline]
    fn load(&mut self, _cx: ConnectionContext) -> config::Config {
        self.clone()
    }
}

impl<T: FnMut(ConnectionContext) -> config::Config + Send + 'static> ConfigLoader for T {
    #[inline]
    fn load(&mut self, cx: ConnectionContext) -> config::Config {
        (self)(cx)
    }
}

impl ConfigLoader for Box<dyn ConfigLoader> {
    #[inline]
    fn load(&mut self, cx: ConnectionContext) -> config::Config {
        (**self).load(cx)
    }
}

mod callback;
mod keylog;
mod params;
mod session;

pub mod certificate;
pub mod client;
pub mod server;

pub use client::Client;
pub use s2n_tls::*;
pub use server::Server;

#[cfg(test)]
mod tests;

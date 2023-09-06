// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use s2n_quic_core::application::ServerName;
pub use s2n_tls::{
    config::{Builder, Config},
    error::Error,
    security::DEFAULT_TLS13,
};

/// Ensure memory is correctly managed in tests
#[cfg(test)]
#[global_allocator]
static ALLOCATOR: checkers::Allocator = checkers::Allocator::system();

#[cfg(all(s2n_quic_unstable, s2n_quic_enable_pq_tls))]
static DEFAULT_POLICY: &s2n_tls::security::Policy = &s2n_tls::security::TESTING_PQ;
#[cfg(not(all(s2n_quic_unstable, s2n_quic_enable_pq_tls)))]
static DEFAULT_POLICY: &s2n_tls::security::Policy = &s2n_tls::security::DEFAULT_TLS13;

#[non_exhaustive]
pub struct ConnectionContext<'a> {
    pub server_name: Option<&'a ServerName>,
}

/// Loads a config for a given connection
///
/// This trait can be implemented to override the default config loading for a QUIC endpoint
pub trait ConfigLoader: 'static + Send {
    fn load(&mut self, cx: ConnectionContext) -> Config;
}

impl ConfigLoader for Config {
    #[inline]
    fn load(&mut self, _cx: ConnectionContext) -> Config {
        self.clone()
    }
}

impl<T: FnMut(ConnectionContext) -> Config + Send + 'static> ConfigLoader for T {
    #[inline]
    fn load(&mut self, cx: ConnectionContext) -> Config {
        (self)(cx)
    }
}

impl ConfigLoader for Box<dyn ConfigLoader> {
    #[inline]
    fn load(&mut self, cx: ConnectionContext) -> Config {
        (**self).load(cx)
    }
}

mod callback;
pub mod keylog;
mod params;
mod session;

pub mod certificate;
pub mod client;
pub mod server;

pub use client::Client;
pub use server::Server;

// Re-export the `ClientHelloHandler` and `Connection` to make it easier for users
// to consume. This depends on experimental behavior in s2n-tls.
#[cfg(any(test, all(s2n_quic_unstable, feature = "unstable_client_hello")))]
pub use s2n_tls::{self, callbacks::ClientHelloCallback, connection::Connection};

#[cfg(test)]
mod tests;

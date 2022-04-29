// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

/// Ensure memory is correctly managed in tests
#[cfg(test)]
#[global_allocator]
static ALLOCATOR: checkers::Allocator = checkers::Allocator::system();

#[cfg(all(s2n_quic_unstable, s2n_quic_enable_pq_tls))]
static DEFAULT_POLICY: &s2n_tls::raw::security::Policy = &s2n_tls::raw::security::TESTING_PQ;
#[cfg(not(all(s2n_quic_unstable, s2n_quic_enable_pq_tls)))]
static DEFAULT_POLICY: &s2n_tls::raw::security::Policy = &s2n_tls::raw::security::DEFAULT_TLS13;

mod callback;
mod keylog;
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
pub use s2n_tls::raw::{config::ClientHelloHandler, connection::Connection};

#[cfg(test)]
mod tests;

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

/// Ensure memory is correctly managed in tests
#[cfg(test)]
#[global_allocator]
static ALLOCATOR: checkers::Allocator = checkers::Allocator::system();

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
#[cfg(all(feature = "unstable_s2n_quic_tls_client_hello", s2n_quic_unstable))]
pub use s2n_tls::raw::{config::ClientHelloHandler, connection::Connection};

#[cfg(test)]
mod tests;

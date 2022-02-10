// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! An implementation of the IETF QUIC protocol
//!
//! ## Feature flags
//!
//! ### `provider-address-token-default`
//!
//! _Enabled by default_
//!
//! Enables the default address token provider, which
//! will securely generate address tokens for a single QUIC server. If your infrastructure requires
//! multiple servers handle address tokens, this provider should not be used. Instead, a custom
//! implementation of [`provider::address_token::Format`] should be specified.
//!
//! ### `provider-event-tracing`
//!
//! Enables event integration with [`tracing`](https://docs.rs/tracing). The
//! default event provider will be set to [`provider::event::tracing::Provider`] and will emit
//! endpoint and connection events to the application's configured
//! [`tracing::Subscriber`](https://docs.rs/tracing/latest/tracing/trait.Subscriber.html).
//!
//! ### `provider-tls-default`
//!
//! _Enabled by default_
//!
//! Enables platform detection for the most appropriate version of TLS. Currently, this uses
//! [`s2n-tls`][s2n-tls] on unix-like platforms and [`rustls`][rustls] on everything else.
//!
//! ### `provider-tls-rustls`
//!
//! Enables the [`rustls`][rustls] TLS provider. The provider will be available at
//! [`provider::tls::rustls`].
//!
//! **NOTE**: this will override the platform detection and always use [`rustls`][rustls] by default.
//!
//! ### `provider-tls-s2n`
//!
//! Enables the [`s2n-tls`][s2n-tls] TLS provider. The provider will be available at
//! [`provider::tls::s2n_tls`].
//!
//! **NOTE**: this will override the platform detection and always use [`s2n-tls`][s2n-tls] by default.
//!
//! [s2n-tls]: https://github.com/aws/s2n-tls
//! [rustls]: https://github.com/rustls/rustls

#[macro_use]
pub mod provider;

pub mod client;
pub mod connection;
pub mod server;
pub mod stream;

pub mod application {
    pub use s2n_quic_core::application::Error;

    #[deprecated(note = "use `s2n_quic::server::Name` instead")]
    pub type Sni = crate::server::Name;
}

pub use client::Client;
pub use connection::Connection;
pub use server::Server;

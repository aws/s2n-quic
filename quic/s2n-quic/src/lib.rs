// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! An implementation of the [IETF QUIC protocol](https://quicwg.org/), featuring:
//! * a simple, easy-to-use API. See [an example](https://github.com/aws/s2n-quic/blob/main/examples/echo/src/bin/quic_echo_server.rs) of an s2n-quic echo server built with just a few API calls
//! * high configurability using [providers](https://docs.rs/s2n-quic/latest/s2n_quic/provider/index.html) for granular control of functionality
//! * extensive automated testing, including fuzz testing, integration testing, unit testing, snapshot testing, efficiency testing, performance benchmarking, interoperability testing and [more](https://github.com/aws/s2n-quic/blob/main/docs/ci.md)
//! * integration with [s2n-tls](https://github.com/aws/s2n-tls), AWS's simple, small, fast and secure TLS implementation, as well as [rustls](https://crates.io/crates/rustls)
//! * thorough [compliance coverage tracking](https://github.com/aws/s2n-quic/blob/main/docs/ci.md#compliance) of normative language in relevant standards
//! * and much more, including [CUBIC congestion controller](https://www.rfc-editor.org/rfc/rfc8312.html) support, [packet pacing](https://www.rfc-editor.org/rfc/rfc9002.html#name-pacing), [Generic Segmentation Offload](https://lwn.net/Articles/188489/) support, [Path MTU discovery](https://www.rfc-editor.org/rfc/rfc8899.html), and unique [connection identifiers](https://www.rfc-editor.org/rfc/rfc9000.html#name-connection-id) detached from the address
//!
//! See the [installation instructions](https://github.com/aws/s2n-quic#installation) and [examples](https://github.com/aws/s2n-quic/tree/main/examples) to get started with `s2n-quic`.
//!
//! ## Feature flags
//!
//! ### `provider-address-token-default`
//!
//! _Enabled by default_
//!
//! Enables the default address token provider, which
//! will securely generate address tokens for a single QUIC server. If your deployment requires
//! that multiple servers handle address tokens, this provider should not be used. Instead, a custom
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
//! Enables platform detection for the recommended implementation of TLS. Currently, this uses
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
//! ### `provider-tls-fips`
//!
//! **FIPS mode with `provider-tls-s2n`**
//!
//! FIPS mode can be enabled with the [`s2n-tls`][s2n-tls] TLS provider on
//! non-windows platforms.
//!
//! Applications wanting to use FIPS-approved cryptography with `provider-tls-s2n` should:
//!
//! 1. Enable the following features:
//!
//!```ignore
//! s2n-quic = { version = "1", features = ["provider-tls-fips", "provider-tls-s2n"] }
//!```
//!
//! 2. Build a custom s2n-tls TLS provider configured with a FIPS approved
//!    [security policy](https://aws.github.io/s2n-tls/usage-guide/ch06-security-policies.html):
//!
//!```ignore
//! use s2n_quic::provider::tls::s2n_tls;
//! use s2n_quic::provider::tls::s2n_tls::security::Policy;
//!
//! let mut tls = s2n_tls::Server::builder();
//! let policy = Policy::from_version("20230317")?;
//! tls.config_mut().set_security_policy(&policy)?;
//! let tls = tls
//!     .with_certificate(..)?
//!     ...
//!     .build()?;
//!
//! let mut server = s2n_quic::Server::builder()
//!     .with_tls(tls)?
//!     ...
//!     .start()?;
//!```
//!
//! **FIPS mode with `provider-tls-rustls`**
//!
//! FIPS mode can be enabled with the [`rustls`][rustls] TLS provider. Applications are
//! responsible for meeting guidelines for using rustls with
//! [FIPS-approved cryptography](https://docs.rs/rustls/latest/rustls/manual/_06_fips/index.html).
//!
//! Applications wanting to use FIPS-approved cryptography with `provider-tls-rustls` should:
//!
//! 1. Enable the following features:
//!
//!```ignore
//! s2n-quic = { version = "1", features = ["provider-tls-fips", "provider-tls-rustls"] }
//!```
//!
//! [s2n-tls]: https://github.com/aws/s2n-tls
//! [rustls]: https://github.com/rustls/rustls

// Tag docs with the required platform and features.
// https://doc.rust-lang.org/rustdoc/unstable-features.html
#![cfg_attr(docsrs, feature(doc_cfg))]
#![cfg_attr(
    docsrs,
    feature(doc_auto_cfg),
    feature(doc_cfg_hide),
    doc(cfg_hide(doc))
)]

#[macro_use]
pub mod provider;

pub mod client;
pub mod connection;
pub mod server;
pub mod stream;

pub mod application {
    pub use s2n_quic_core::application::Error;
}

pub use client::Client;
pub use connection::Connection;
pub use server::Server;

#[cfg(test)]
mod tests;

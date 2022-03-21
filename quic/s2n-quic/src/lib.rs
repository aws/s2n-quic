// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! An implementation of the [IETF QUIC protocol](https://quicwg.org/), featuring:
//! * a simple, easy-to-use API. See [an example](https://github.com/aws/s2n-quic/blob/main/examples/echo/src/bin/quic_echo_server.rs) of an s2n-quic echo server built with just a few API calls
//! * high configurability using [providers](https://docs.rs/s2n-quic/latest/s2n_quic/provider/index.html) for granular control of functionality
//! * extensive automated testing, including fuzz testing, integration testing, unit testing, snapshot testing, efficiency testing, performance benchmarking, interopability testing and [more](https://github.com/aws/s2n-quic/blob/main/docs/ci.md)
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
//! ## Unstable features
//!
//! These features enable **unstable** features. Unstable features are subject to change without
//! notice. To enable these features, the `--cfg s2n_quic_unstable` option must be passed to
//! rustc when compiling. This is easiest done using the RUSTFLAGS env variable:
//! `RUSTFLAGS=\"--cfg s2n_quic_unstable\"`.
//!
//! ### `unstable_client_hello`
//!
//! Enables the `ClientHelloHandler` trait, which can be used to set the client_hello callback on
//! s2n-tls provider.
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
}

pub use client::Client;
pub use connection::Connection;
pub use server::Server;

// Require `--cfg s2n_quic_unstable` is set when using unstable features
#[cfg(
    all(
        any(
            feature = "unstable_client_hello"
        ),
        // any unstable features requires at least one of the following conditions
        not(any(
            // we're running tests
            test,
            doctest,
            // we're compiling docs for docs.rs
            docsrs,
            // we're developing s2n-quic
            s2n_internal_dev,
            // the application has explicitly opted into unstable features
            s2n_quic_unstable,
        ))
    )
)]
std::compile_error!("Application must be built with RUSTFLAGS=\"--cfg s2n_quic_unstable\" to use unstable features.");

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Event integration with [`tracing`](https://docs.rs/tracing).
//!
//! # Security Considerations
//!
//! This module's [`Subscriber`] emits all event fields at the `DEBUG` level,
//! including security-sensitive values such as Stateless Reset Tokens and
//! connection identifiers. Ensure that `tracing` output is not persisted or
//! exposed in environments where this data could be accessed by unauthorized
//! parties. Consider implementing a custom [`event::Subscriber`](super::Subscriber)
//! if you need to redact sensitive fields.

pub use s2n_quic_core::event::tracing::Subscriber;

#[derive(Debug, Default)]
pub struct Provider(());

impl super::Provider for Provider {
    type Subscriber = Subscriber;
    type Error = core::convert::Infallible;

    fn start(self) -> Result<Self::Subscriber, Self::Error> {
        Ok(Subscriber::default())
    }
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use cfg_if::cfg_if;
use core::fmt;

#[macro_use]
mod macros;

pub mod address_token;
pub mod connection_id;
pub mod endpoint_limits;
pub mod event;
pub mod io;
pub mod limits;
pub mod stateless_reset_token;
pub mod tls;

// These providers are not currently exposed to applications
pub(crate) mod congestion_controller;
pub(crate) mod connection_close_formatter;
pub(crate) mod path_migration;
pub(crate) mod sync;

cfg_if!(
    if #[cfg(all(s2n_quic_unstable, feature = "unstable-provider-packet-interceptor"))] {
        pub mod packet_interceptor;
    } else {
        pub(crate) mod packet_interceptor;
    }
);

cfg_if!(
    if #[cfg(all(s2n_quic_unstable, feature = "unstable-provider-random"))] {
        pub mod random;
    } else {
        pub(crate) mod random;
    }
);

/// An error indicating a failure to start an endpoint
pub struct StartError(Box<dyn 'static + fmt::Display>);

impl std::error::Error for StartError {}

impl StartError {
    pub(crate) fn new<T: 'static + fmt::Display>(error: T) -> Self {
        Self(Box::new(error))
    }
}

impl fmt::Debug for StartError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_tuple("StartError")
            .field(&format_args!("{}", self.0))
            .finish()
    }
}

impl fmt::Display for StartError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.0.fmt(f)
    }
}

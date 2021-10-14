// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::fmt;

#[macro_use]
mod macros;

pub mod congestion_controller;
pub mod connection_id;
pub mod endpoint_limits;
pub mod event;
pub mod io;
pub mod limits;
pub mod stateless_reset_token;
pub mod tls;
pub mod token;

// These providers are not currently exposed to applications
pub(crate) mod connection_close_formatter;
pub(crate) mod path_migration;
pub(crate) mod random;
pub(crate) mod sync;

/// An error indicating a failure to start an endpoint
#[cfg_attr(feature = "thiserror", derive(thiserror::Error))]
#[non_exhaustive]
pub struct StartError(Box<dyn 'static + fmt::Display>);

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

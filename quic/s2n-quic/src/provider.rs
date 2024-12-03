// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use cfg_if::cfg_if;
use core::fmt;

#[macro_use]
mod macros;

pub mod address_token;
pub mod congestion_controller;
pub mod connection_id;
pub mod endpoint_limits;
pub mod event;
pub mod io;
pub mod limits;
pub mod mtu;
pub mod stateless_reset_token;
pub mod tls;

// These providers are not currently exposed to applications
#[allow(dead_code)]
pub(crate) mod path_migration;
#[allow(dead_code)]
pub(crate) mod sync;

cfg_if!(
    if #[cfg(any(test, feature = "unstable-provider-connection-close-formatter"))] {
        #[cfg_attr(s2n_docsrs, doc(cfg(feature = "unstable-provider-connection-close-formatter")))]
        pub mod connection_close_formatter;
    } else {
        #[allow(dead_code)]
        pub(crate) mod connection_close_formatter;
    }
);

cfg_if!(
    if #[cfg(any(test, feature = "unstable-provider-packet-interceptor"))] {
        #[cfg_attr(s2n_docsrs, doc(cfg(feature = "unstable-provider-packet-interceptor")))]
        pub mod packet_interceptor;
    } else {
        #[allow(dead_code)]
        pub(crate) mod packet_interceptor;
    }
);

cfg_if!(
    if #[cfg(any(test, feature = "unstable-provider-random"))] {
        #[cfg_attr(s2n_docsrs, doc(cfg(feature = "unstable-provider-random")))]
        pub mod random;
    } else {
        #[allow(dead_code)]
        pub(crate) mod random;
    }
);

cfg_if!(
    if #[cfg(any(test, feature = "unstable-provider-datagram"))] {
        #[cfg_attr(s2n_docsrs, doc(cfg(feature = "unstable-provider-datagram")))]
        pub mod datagram;
    } else {
        #[allow(dead_code)]
        pub(crate) mod datagram;
    }
);

cfg_if!(
    if #[cfg(any(test, feature = "unstable-provider-dc"))] {
        #[cfg_attr(s2n_docsrs, doc(cfg(feature = "unstable-provider-dc")))]
        pub mod dc;
    } else {
        #[allow(dead_code)]
        pub(crate) mod dc;
    }
);

/// An error indicating a failure to start an endpoint
pub struct StartError(Box<dyn 'static + fmt::Display + Send + Sync>);

impl std::error::Error for StartError {}

impl StartError {
    pub(crate) fn new<T: 'static + fmt::Display + Send + Sync>(error: T) -> Self {
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

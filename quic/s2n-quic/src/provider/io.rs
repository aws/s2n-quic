// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use cfg_if::cfg_if;
pub use s2n_quic_core::{endpoint::Endpoint, path::Handle as PathHandle};
use std::io;

/// Provides IO support for an endpoint
pub trait Provider: 'static {
    type PathHandle: PathHandle;
    type Error: 'static + core::fmt::Display;

    fn start<E: Endpoint<PathHandle = Self::PathHandle>>(
        self,
        endpoint: E,
    ) -> Result<(), Self::Error>;
}

cfg_if! {
    if #[cfg(feature = "tokio-runtime")] {
        pub mod tokio;

        pub use self::tokio as default;
    } else {
        // TODO add a default
    }
}

pub use default::Provider as Default;

impl TryInto for u16 {
    type Error = io::Error;
    type Provider = Default;

    fn try_into(self) -> io::Result<Self::Provider> {
        Default::new(("0.0.0.0", self))
    }
}

macro_rules! impl_socket_addrs {
    ($ty:ty) => {
        impl TryInto for $ty {
            type Error = io::Error;
            type Provider = Default;

            fn try_into(self) -> io::Result<Self::Provider> {
                Default::new(self)
            }
        }
    };
}

impl_socket_addrs!((&str, u16));
impl_socket_addrs!(&str);
impl_socket_addrs!(std::net::SocketAddr);
impl_socket_addrs!(std::net::SocketAddrV4);
impl_socket_addrs!(std::net::SocketAddrV6);

impl_provider_utils!();

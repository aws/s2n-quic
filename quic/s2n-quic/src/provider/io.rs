// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Provides IO support for an endpoint

use s2n_quic_core::{endpoint::Endpoint, inet::SocketAddress, path::Handle as PathHandle};
use std::io;

pub trait Provider: 'static {
    type PathHandle: PathHandle;
    type Error: 'static + core::fmt::Display;

    fn start<E: Endpoint<PathHandle = Self::PathHandle>>(
        self,
        endpoint: E,
    ) -> Result<SocketAddress, Self::Error>;
}

#[cfg(any(test, feature = "unstable-provider-io-testing"))]
pub mod testing;

#[cfg(feature = "unstable-provider-io-turmoil")]
pub mod turmoil;

#[cfg(feature = "unstable-provider-io-xdp")]
pub mod xdp;

pub mod tokio;

pub use self::tokio as default;

pub use default::Provider as Default;

impl TryInto for u16 {
    type Error = io::Error;
    type Provider = Default;

    fn try_into(self) -> io::Result<Self::Provider> {
        Default::new(("::", self))
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
impl_socket_addrs!((std::net::IpAddr, u16));
impl_socket_addrs!((std::net::Ipv4Addr, u16));
impl_socket_addrs!((std::net::Ipv6Addr, u16));
impl_socket_addrs!(&str);
impl_socket_addrs!(std::net::SocketAddr);
impl_socket_addrs!(std::net::SocketAddrV4);
impl_socket_addrs!(std::net::SocketAddrV6);

impl_provider_utils!();

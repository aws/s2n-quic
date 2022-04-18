// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

pub use s2n_quic_core::packet::interceptor::{Disabled, Interceptor as PacketInterceptor};

/// Provides packet_interceptor support for an endpoint
pub trait Provider: 'static {
    type PacketInterceptor: 'static + PacketInterceptor;
    type Error: core::fmt::Display;

    fn start(self) -> Result<Self::PacketInterceptor, Self::Error>;
}

pub type Default = Disabled;

impl_provider_utils!();

impl<T: 'static + Send + PacketInterceptor> Provider for T {
    type PacketInterceptor = T;
    type Error = core::convert::Infallible;

    fn start(self) -> Result<Self::PacketInterceptor, Self::Error> {
        Ok(self)
    }
}

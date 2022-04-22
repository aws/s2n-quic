// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Provides unreliable datagram support

use s2n_quic_core::datagram::{DatagramApi, Disabled};
pub trait Provider: 'static {
    type DatagramApi: 'static + DatagramApi;
    type Error: core::fmt::Display;

    fn start(self) -> Result<Self::DatagramApi, Self::Error>;
}

impl_provider_utils!();

pub type Default = Disabled;

impl<T: 'static + Send + DatagramApi> Provider for T {
    type DatagramApi = T;
    type Error = core::convert::Infallible;

    fn start(self) -> Result<Self::DatagramApi, Self::Error> {
        Ok(self)
    }
}

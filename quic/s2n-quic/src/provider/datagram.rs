// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Provides unreliable datagram support

use s2n_quic_core::datagram::Disabled;

// these imports are only accessible if the unstable feature is enabled
#[allow(unused_imports)]
pub use s2n_quic_core::datagram::{
    default,
    traits::{
        ConnectionInfo, Endpoint, Packet, PreConnectionInfo, ReceiveContext, Receiver, Sender,
        WriteError,
    },
};

pub trait Provider {
    type Endpoint: Endpoint;
    type Error: 'static + core::fmt::Display + Send + Sync;

    fn start(self) -> Result<Self::Endpoint, Self::Error>;
}

impl_provider_utils!();

pub type Default = Disabled;

impl<T: 'static + Send + Endpoint> Provider for T {
    type Endpoint = T;
    type Error = core::convert::Infallible;

    fn start(self) -> Result<Self::Endpoint, Self::Error> {
        Ok(self)
    }
}

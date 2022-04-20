// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Provides an implementation of the [`io::Provider`](crate::provider::io::Provider)
//! using the [`Tokio runtime`](https://docs.rs/tokio/latest/tokio/runtime/index.html)

use s2n_quic_core::{endpoint::Endpoint, inet::SocketAddress};
use s2n_quic_platform::io::tokio;
use std::io;

pub use self::tokio::{Builder, Io as Provider};

impl super::Provider for Provider {
    type PathHandle = tokio::PathHandle;
    type Error = io::Error;

    fn start<E: Endpoint<PathHandle = Self::PathHandle>>(
        self,
        endpoint: E,
    ) -> Result<SocketAddress, Self::Error> {
        let (_join_handle, local_addr) = Provider::start(self, endpoint)?;
        Ok(local_addr)
    }
}

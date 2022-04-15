// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use s2n_quic_core::{endpoint::Endpoint, inet::SocketAddress};
pub use s2n_quic_platform::io::testing;
use std::io;

pub use self::testing::*;

impl super::Provider for Io {
    type PathHandle = PathHandle;
    type Error = io::Error;

    fn start<E: Endpoint<PathHandle = Self::PathHandle>>(
        self,
        endpoint: E,
    ) -> Result<SocketAddress, Self::Error> {
        let (_join_handle, local_addr) = Io::start(self, endpoint)?;
        Ok(local_addr)
    }
}

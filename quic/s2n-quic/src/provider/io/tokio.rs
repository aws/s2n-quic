// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use s2n_quic_core::endpoint::Endpoint;
use s2n_quic_platform::io::tokio;
use std::io;

pub use self::tokio::{Builder, Io as Provider};

impl super::Provider for Provider {
    type PathHandle = tokio::PathHandle;
    type Error = io::Error;

    fn start<E: Endpoint<PathHandle = Self::PathHandle>>(
        self,
        endpoint: E,
    ) -> Result<(), Self::Error> {
        self.start(endpoint)?;
        Ok(())
    }
}

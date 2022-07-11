// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::time::Duration;
use s2n_quic_core::{endpoint::Endpoint, inet::SocketAddress};
pub use s2n_quic_platform::io::testing;
use std::io;

pub use self::testing::*;

pub type Error = Box<dyn 'static + std::error::Error>;
pub type Result<T = (), E = Error> = core::result::Result<T, E>;

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

/// Runs a single test with the given network
///
/// Returns the total runtime of the test
pub fn test<N: Network, F: FnOnce(&Handle) -> Result<O>, O>(network: N, f: F) -> Result<Duration> {
    test_seed(network, 123456789, f)
}

/// Runs a single test with the given network and seed value
///
/// Returns the total runtime of the test
pub fn test_seed<N: Network, F: FnOnce(&Handle) -> Result<O>, O>(
    network: N,
    seed: u64,
    f: F,
) -> Result<Duration> {
    let mut executor = Executor::new(network, seed);
    let handle = executor.handle().clone();

    executor.enter(|| f(&handle))?;

    executor.run();

    // return the total runtime of the test
    let now = executor.enter(time::now);

    let now = unsafe { now.as_duration() };

    Ok(now)
}

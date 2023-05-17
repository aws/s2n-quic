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

pub struct Test<N> {
    network: N,
    seed: u64,
    recorder: Option<std::sync::Arc<dyn network::Recorder>>,
}

impl<N: Network> Test<N> {
    pub fn new(network: N) -> Self {
        Self {
            network,
            seed: 123456789,
            recorder: None,
        }
    }

    pub fn with_name(mut self, name: impl AsRef<str>) -> Self {
        if std::env::var("S2N_QUIC_PCAP").is_ok() {
            let name = name.as_ref();
            let file = pcap::File::open(format!("{name}.pcapng")).unwrap();
            self.recorder = Some(std::sync::Arc::new(file));
        }
        self
    }

    pub fn with_recorder(mut self, r: std::sync::Arc<dyn network::Recorder>) -> Self {
        self.recorder = Some(r);
        self
    }

    pub fn with_seed(mut self, seed: u64) -> Self {
        self.seed = seed;
        self
    }

    pub fn run<F, O>(self, f: F) -> Result<(Duration, O)>
    where
        F: FnOnce(&Handle) -> Result<O>,
    {
        let mut executor = if let Some(recorder) = self.recorder {
            Executor::new_recorded(self.network, self.seed, recorder)
        } else {
            Executor::new(self.network, self.seed)
        };
        let handle = executor.handle().clone();

        let out = executor.enter(|| f(&handle))?;

        executor.run();

        // return the total runtime of the test
        let now = executor.enter(time::now);

        let now = unsafe { now.as_duration() };

        Ok((now, out))
    }
}

/// Runs a single test with the given network
///
/// Returns the total runtime of the test
pub fn test<N: Network, F: FnOnce(&Handle) -> Result<O>, O>(
    network: N,
    f: F,
) -> Result<(Duration, O)> {
    Test::new(network).run(f)
}

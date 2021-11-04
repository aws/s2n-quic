// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use structopt::StructOpt;

pub type Error = Box<dyn 'static + std::error::Error + Send + Sync>;
pub type Result<T, E = Error> = core::result::Result<T, E>;

mod client;
mod file;
mod interop;
mod perf;
mod server;

/// This message is searched in interop logs to ensure the application doesn't panic
///
/// Do not change it without updating it elsewhere
const CRASH_ERROR_MESSAGE: &str = "The s2n-quic-qns application shut down unexpectedly";

#[cfg(feature = "dhat")]
#[global_allocator]
static ALLOCATOR: dhat::DhatAlloc = dhat::DhatAlloc;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    // setup heap profiling if enabled
    #[cfg(feature = "dhat")]
    let _dhat = dhat::Dhat::start_heap_profiling();

    tracing_subscriber::fmt::init();

    Arguments::from_args().run().await
}

#[derive(Debug, StructOpt)]
enum Arguments {
    Interop(Interop),
    Perf(Perf),
}

impl Arguments {
    pub async fn run(&self) -> Result<()> {
        match self {
            Self::Interop(subject) => subject.run().await,
            Self::Perf(subject) => subject.run().await,
        }
    }
}

#[derive(Debug, StructOpt)]
enum Interop {
    Server(server::Interop),
    Client(client::Interop),
}

impl Interop {
    pub async fn run(&self) -> Result<()> {
        match self {
            Self::Server(subject) => subject.run().await,
            Self::Client(subject) => subject.run().await,
        }
    }
}

#[derive(Debug, StructOpt)]
enum Perf {
    Server(server::Perf),
    Client(client::Perf),
}

impl Perf {
    pub async fn run(&self) -> Result<()> {
        match self {
            Self::Server(subject) => subject.run().await,
            Self::Client(subject) => subject.run().await,
        }
    }
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use structopt::StructOpt;

pub type Error = Box<dyn 'static + std::error::Error + Send + Sync>;
pub type Result<T> = core::result::Result<T, Error>;
mod client;
mod file;
mod server;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .init();

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

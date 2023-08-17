// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use structopt::StructOpt;

pub type Error = Box<dyn 'static + std::error::Error + Send + Sync>;
pub type Result<T, E = Error> = core::result::Result<T, E>;

mod client;
mod congestion_control;
mod file;
mod interop;
mod io;
mod limits;
mod perf;
mod runtime;
mod server;
mod task;
mod tls;
#[cfg(feature = "xdp")]
mod xdp;

/// This message is searched in interop logs to ensure the application doesn't panic
///
/// Do not change it without updating it elsewhere
const CRASH_ERROR_MESSAGE: &str = "The s2n-quic-qns application shut down unexpectedly";

#[cfg(not(target_os = "android"))]
#[global_allocator]
static ALLOC: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn main() {
    let format = tracing_subscriber::fmt::format()
        .with_level(false) // don't include levels in formatted output
        .with_timer(tracing_subscriber::fmt::time::uptime())
        .with_ansi(false)
        .compact(); // Use a less verbose output format.

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .event_format(format)
        .init();

    match Arguments::from_args_safe() {
        Ok(args) => {
            if let Err(error) = args.run() {
                eprintln!("Error: {error:?}");
                std::process::exit(1);
            }
        }
        Err(error) => {
            if error.use_stderr() {
                eprintln!("{error}");

                // https://github.com/marten-seemann/quic-interop-runner/blob/cd223804bf3f102c3567758ea100577febe486ff/interop.py#L102
                // The interop runner wants us to exit with code 127 when an invalid argument is passed
                std::process::exit(127);
            } else {
                println!("{error}");
            }
        }
    };
}

#[derive(Debug, StructOpt)]
enum Arguments {
    Interop(Interop),
    Perf(Perf),
}

impl Arguments {
    pub fn run(&self) -> Result<()> {
        match self {
            Self::Interop(subject) => subject.run(),
            Self::Perf(subject) => subject.run(),
        }
    }
}

#[derive(Debug, StructOpt)]
enum Interop {
    Server(server::Interop),
    Client(client::Interop),
}

impl Interop {
    pub fn run(&self) -> Result<()> {
        match self {
            Self::Server(subject) => subject.run(),
            Self::Client(subject) => subject.run(),
        }
    }
}

#[derive(Debug, StructOpt)]
enum Perf {
    Server(server::Perf),
    Client(client::Perf),
}

impl Perf {
    pub fn run(&self) -> Result<()> {
        match self {
            Self::Server(subject) => subject.run(),
            Self::Client(subject) => subject.run(),
        }
    }
}

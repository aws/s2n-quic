// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

pub mod analyze;
pub mod cwlogs;
pub mod investigate;
pub mod local;

use anyhow::Result;
use clap::Subcommand;
use xshell::Shell;

#[derive(Subcommand)]
pub enum Command {
    /// Run a local cluster of rpc-tester server(s) and client(s)
    Local(local::Local),
    /// Analyze diagnostic trace files from errored streams
    Analyze(analyze::Analyze),
    /// Pull [METRICS] lines from CloudWatch Logs and write to Parquet
    Cwlogs(cwlogs::Cwlogs),
    /// Investigate s2n-quic-dc metrics stored in Parquet files
    Investigate(investigate::Investigate),
}

impl Command {
    pub fn run(self, sh: &Shell) -> Result<()> {
        match self {
            Self::Local(cmd) => cmd.run(sh),
            Self::Analyze(cmd) => cmd.run(sh),
            Self::Cwlogs(cmd) => cmd.run(sh),
            Self::Investigate(cmd) => cmd.run(sh),
        }
    }
}

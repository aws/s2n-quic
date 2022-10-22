// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use netbench::{scenario::Scenario, units::parse_duration, Result};
use std::time::Duration;
use structopt::StructOpt;

mod bpftrace;
mod generic;
mod procinfo;

#[derive(Debug, StructOpt)]
pub struct Args {
    pub driver: String,

    #[structopt(long, short, env = "SCENARIO")]
    pub scenario: String,

    #[structopt(long, short, parse(try_from_str=parse_duration), default_value = "1s")]
    pub interval: Duration,
}

impl Args {
    pub fn scenario(&self) -> Result<Scenario> {
        Scenario::open(std::path::Path::new(&self.scenario))
    }
}

fn main() -> Result<()> {
    let args = Args::from_args();

    // try to use bpftrace
    if let Some(()) = bpftrace::try_run(&args)? {
        return Ok(());
    }

    // fall back to the generic collector
    generic::run(&args)?;

    Ok(())
}

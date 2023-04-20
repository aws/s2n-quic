// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{scenario::Scenario, units::parse_duration, Result};
use enum_dispatch::enum_dispatch;
use std::time::Duration;
use structopt::StructOpt;

mod bpftrace;
mod generic;
mod procinfo;

use bpftrace::BpftraceHandle;
use generic::GenericHandle;

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

/// Use either the Bpftrace Handle or the Generic Handle
#[enum_dispatch]
pub enum Handle {
    GenericHandle,
    BpftraceHandle,
}

#[enum_dispatch(Handle)]
pub trait RunHandle {
    /// Gracefully wait for the collector to finish running.
    fn wait(self) -> Result<()>;
    /// Force the collector to stop.
    fn kill(self) -> Result<()>;
}

/// Define a task which will run the collector.
///
/// Await the returned JoinHandle to begin the execution. The result
/// of awaiting will be a handle to the running process that can either
/// be killed or waited on.
pub async fn run(args: Args) -> Handle {
    // try to use bpftrace
    if let Some(trace_handle) = bpftrace::try_run(&args).unwrap() {
        return trace_handle.into();
    }

    // fall back to the generic collector
    generic::run(&args).unwrap().into()
}

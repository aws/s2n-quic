// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0
use netbench::{
    collector::{run, Args, RunHandle},
    Result,
};
use structopt::StructOpt;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let args = Args::from_args();

    let run_handle = run(args).await;

    // Let the driver run till it decides to quit on it's own
    run_handle.wait()
}

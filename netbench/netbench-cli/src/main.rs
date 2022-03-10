// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use structopt::StructOpt;

type Error = Box<dyn std::error::Error>;
type Result<T, E = Error> = core::result::Result<T, E>;

mod report;

#[derive(StructOpt)]
enum Args {
    Report(report::Report),
}

fn main() -> Result<()> {
    match Args::from_args() {
        Args::Report(report) => report.run(),
    }
}

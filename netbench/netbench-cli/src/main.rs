// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use netbench::Result;
use structopt::StructOpt;

mod report;
mod report_tree;

#[derive(StructOpt)]
enum Args {
    Report(report::Report),
    ReportTree(report_tree::ReportTree),
}

fn main() -> Result<()> {
    match Args::from_args() {
        Args::Report(report) => report.run(),
        Args::ReportTree(report) => report.run(),
    }
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::Result;
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
pub struct Interop {}

impl Interop {
    pub async fn run(&self) -> Result<()> {
        eprintln!("unsupported");
        std::process::exit(127);
    }
}

#[derive(Debug, StructOpt)]
pub struct Perf {}

impl Perf {
    pub async fn run(&self) -> Result<()> {
        eprintln!("unsupported");
        std::process::exit(127);
    }
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use structopt::StructOpt;

mod annotation;
mod extract;
mod object;
mod parser;
mod pattern;
mod project;
mod report;
mod source;
mod sourcemap;
mod specification;
mod target;

pub use anyhow::Error;

fn main() {
    if let Err(err) = Arguments::from_args().exec() {
        eprintln!("{}", err);
        std::process::exit(1);
    }
}

#[derive(Debug, StructOpt)]
enum Arguments {
    Extract(extract::Extract),
    Report(report::Report),
}

impl Arguments {
    pub fn exec(&self) -> Result<(), Error> {
        match self {
            Self::Extract(args) => args.exec(),
            Self::Report(args) => args.exec(),
        }
    }
}

pub(crate) fn fnv<H: core::hash::Hash + ?Sized>(value: &H) -> u64 {
    use core::hash::Hasher;
    let mut hasher = fnv::FnvHasher::default();
    value.hash(&mut hasher);
    hasher.finish()
}

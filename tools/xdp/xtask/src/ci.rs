// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{build_ebpf, disasm, run};
use anyhow::anyhow;
use std::io;

pub fn run() -> Result<(), anyhow::Error> {
    let before = dump()?;
    build_ebpf::run()?;
    let after = dump()?;

    if before != after {
        let diff = similar::TextDiff::from_lines(&before, &after);
        let mut diff = diff.unified_diff();
        let out = diff.context_radius(10).header("committed", "current");

        println!("{out}");

        return Err(anyhow!("Difference detected"));
    }

    let mut opts = run::Options::default();

    opts.run_args.push("--exit-on-load".into());
    run::run(opts.clone())?;

    opts.run_args.push("--trace".into());
    run::run(opts)?;

    Ok(())
}

fn dump() -> Result<String, anyhow::Error> {
    let mut output = vec![];
    for input in ["bpfel", "bpfeb", "bpfel-trace", "bpfeb-trace"] {
        disasm::piped(input, io::Cursor::new(&mut output))?;
    }
    let output = String::from_utf8(output)?;
    Ok(output)
}

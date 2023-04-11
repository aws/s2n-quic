// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{build_ebpf, disasm, run};
use anyhow::anyhow;
use std::{io, path::Path, process::Command};

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

    // run the normal tests first
    test()?;

    // run the CAP_NET_RAW tests after
    cap_net_raw_tests()?;

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

fn test() -> Result<(), anyhow::Error> {
    let status = Command::new("cargo")
        .arg("test")
        .status()
        .expect("failed to run tests");
    assert!(status.success());
    Ok(())
}

fn cap_net_raw_tests() -> Result<(), anyhow::Error> {
    let mut at_least_one = false;

    for file in std::fs::read_dir("target/debug/deps")?.flatten() {
        let path = file.path();

        if path.extension().is_some()
            || !path
                .file_name()
                .and_then(|name| name.to_str())
                .map_or(false, |name| name.starts_with("s2n_quic_xdp"))
        {
            continue;
        }

        let status = Command::new("sudo")
            .arg(Path::new("../").join(path))
            .arg("--nocapture")
            .current_dir("s2n-quic-xdp")
            .env("CAP_NET_RAW_ENABLED", "1")
            .status()
            .expect("failed to run test case");
        assert!(status.success());

        at_least_one = true;
    }

    assert!(at_least_one);

    Ok(())
}

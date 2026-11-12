// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{build_ebpf, disasm, run};
use anyhow::anyhow;
use std::{
    io,
    path::PathBuf,
    process::{Command, Stdio},
};

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

    // Ask cargo where the test executables are rather than scanning
    // `target/debug/deps`; newer cargo versions place them under the build
    // directory instead (e.g. `target/debug/build/<pkg>/<hash>/out/`).
    for path in test_executables()? {
        if !path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("s2n_quic_xdp"))
        {
            continue;
        }

        let status = Command::new("sudo")
            .arg(&path)
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

/// Returns the absolute paths of the test executables, as reported by cargo
fn test_executables() -> Result<Vec<PathBuf>, anyhow::Error> {
    let output = Command::new("cargo")
        .args(["test", "--no-run", "--message-format=json"])
        .stderr(Stdio::inherit())
        .output()?;

    if !output.status.success() {
        return Err(anyhow!("failed to build tests"));
    }

    let stdout = String::from_utf8(output.stdout)?;
    let mut paths = vec![];
    for line in stdout.lines() {
        let message: serde_json::Value = serde_json::from_str(line)?;
        if message["reason"] != "compiler-artifact" {
            continue;
        }
        // `executable` is null for artifacts that aren't runnable (libs, build scripts)
        if let Some(path) = message["executable"].as_str() {
            paths.push(PathBuf::from(path));
        }
    }

    Ok(paths)
}

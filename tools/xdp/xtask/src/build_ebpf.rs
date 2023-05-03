// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use std::{path::PathBuf, process::Command};

pub fn run() -> Result<(), anyhow::Error> {
    for arch in ["bpfel", "bpfeb"] {
        let target = format!("{arch}-unknown-none");
        let target = &target;

        for features in [None, Some("trace")] {
            eprintln!("    Building {target}");

            let mut command = Command::new("cargo");

            let dir = PathBuf::from("ebpf");
            command.current_dir(&dir).args([
                "build",
                "--target",
                target,
                "-Z",
                "build-std=core",
                "--release",
            ]);

            // Command::new creates a child process which inherits all env variables. This means env
            // vars set by the cargo xtask command are also inherited. RUSTUP_TOOLCHAIN is removed
            // so the rust-toolchain.toml file in the -ebpf folder is honored.
            command.env_remove("RUSTUP_TOOLCHAIN");

            let mut artifact_name = format!("s2n-quic-xdp-{arch}");

            let mut cargo_out = PathBuf::from("target")
                .join(target)
                .join("release")
                .join("s2n-quic-xdp");

            if let Some(features) = features {
                command.arg("--features");
                command.arg(features);
                command.arg("--target-dir");
                command.arg(PathBuf::from("../target").join(features));

                artifact_name += &format!("-{features}");

                cargo_out = PathBuf::from("target")
                    .join(features)
                    .join(target)
                    .join("release")
                    .join("s2n-quic-xdp");
            }

            let status = command.status().expect("failed to build bpf program");
            assert!(status.success());

            let out = PathBuf::from("s2n-quic-xdp/src/bpf")
                .join(artifact_name)
                .with_extension("ebpf");

            std::fs::create_dir_all(out.parent().unwrap())?;
            std::fs::copy(cargo_out, &out)?;
        }
    }
    Ok(())
}

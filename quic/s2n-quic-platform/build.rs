// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::Feature::*;
use std::{fs::read_dir, io::Error, path::Path, process::Command};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
enum Feature {
    ControlMessage,
    SocketMessage,
    SocketMultiMessage,
    MtuDiscovery,
    GenericSegmentationOffload,
    GenericReceiveOffload,
    PacketInfo,
    TypeOfService,
}

impl Feature {
    fn name(&self) -> &str {
        match self {
            ControlMessage => "cmsg",
            SocketMessage => "socket_msg",
            SocketMultiMessage => "socket_mmsg",
            MtuDiscovery => "mtu_disc",
            GenericSegmentationOffload => "gso",
            GenericReceiveOffload => "gro",
            PacketInfo => "pktinfo",
            TypeOfService => "tos",
        }
    }
}

impl From<&str> for Feature {
    fn from(value: &str) -> Self {
        for feature in ALL_FEATURES {
            if feature.name() == value {
                return feature;
            }
        }

        panic!("unsupported feature: {value}");
    }
}

const ALL_FEATURES: [Feature; 8] = [
    ControlMessage,
    SocketMessage,
    SocketMultiMessage,
    MtuDiscovery,
    GenericSegmentationOffload,
    GenericReceiveOffload,
    PacketInfo,
    TypeOfService,
];

fn main() -> Result<(), Error> {
    let mut features = Features::default();

    // allow overriding the detected features with an env variable
    if let Some(list) = option_env("S2N_QUIC_PLATFORM_FEATURES_OVERRIDE") {
        // iterate twice in case there is dependence on another feature that comes later
        for _ in 0..2 {
            for feature in list.split(',').filter(|&s| !s.is_empty()) {
                features.insert(feature.trim().into());
            }
        }
        return Ok(());
    }

    let env = Env::new();

    for feature in read_dir("features")? {
        let path = feature?.path();
        if let Some(name) = path.file_stem() {
            println!("cargo:rerun-if-changed={}", path.display());
            if env.check(&path)? {
                features.insert(name.to_str().expect("valid feature name").into());
            }
        }
    }

    let is_miri = std::env::var("CARGO_CFG_MIRI").is_ok();

    match env.target_os.as_str() {
        "linux" => {
            // miri doesn't support the way we detect syscall support so override it
            if is_miri {
                features.insert(SocketMessage);
                features.insert(SocketMultiMessage);
            }

            features.insert(MtuDiscovery);
            features.insert(GenericSegmentationOffload);
            features.insert(GenericReceiveOffload);
            features.insert(PacketInfo);
            features.insert(TypeOfService);
        }
        "macos" => {
            // miri doesn't support the way we detect syscall support so override it
            if is_miri {
                features.insert(SocketMessage);
            }

            features.insert(PacketInfo);
            features.insert(TypeOfService);
        }
        "android" => {
            features.insert(MtuDiscovery);
            features.insert(PacketInfo);
            features.insert(TypeOfService);
        }
        _ => {
            // TODO others
        }
    }

    // TODO: Uncomment once MSRV is 1.80.0, see https://github.com/aws/s2n-quic/issues/2334
    // for name in ALL_FEATURES.iter().map(|f| f.name()) {
    //     println!("cargo::rustc-check-cfg=cfg(s2n_quic_platform_{name})");
    // }
    //
    // println!("cargo::rustc-check-cfg=cfg(fuzz, kani, kani_slow)");

    Ok(())
}

#[derive(Debug, Default)]
struct Features {
    features: std::collections::HashSet<Feature>,
}

impl Features {
    fn insert(&mut self, feature: Feature) {
        // supporting any kind message implies cmsg support
        if matches!(feature, SocketMessage | SocketMultiMessage) {
            self.insert(ControlMessage);
        }

        // the following features only make sense if cmsg is supported
        if [
            GenericSegmentationOffload,
            GenericReceiveOffload,
            PacketInfo,
            TypeOfService,
        ]
        .contains(&feature)
            && !self.supports(ControlMessage)
        {
            return;
        }

        let newly_inserted = self.features.insert(feature);
        if newly_inserted {
            println!("cargo:rustc-cfg=s2n_quic_platform_{}", feature.name());
        }
    }

    fn supports(&self, feature: Feature) -> bool {
        self.features.contains(&feature)
    }
}

struct Env {
    rustc: String,
    out_dir: String,
    target: String,
    target_os: String,
    rustc_linker: Option<String>,
}

impl Env {
    fn new() -> Self {
        // See https://doc.rust-lang.org/cargo/reference/environment-variables.html#environment-variables-cargo-sets-for-build-scripts
        Self {
            rustc: env("RUSTC"),
            out_dir: env("OUT_DIR"),
            target: env("TARGET"),
            target_os: env("CARGO_CFG_TARGET_OS"),
            rustc_linker: option_env("RUSTC_LINKER"),
        }
    }

    // Tries to compile the program and returns if it was successful
    fn check(&self, path: &Path) -> Result<bool, Error> {
        let mut command = Command::new(&self.rustc);

        command
            .arg("--out-dir")
            .arg(&self.out_dir)
            .arg("--target")
            .arg(&self.target)
            .arg("--crate-type")
            .arg("bin")
            .arg("--codegen")
            .arg("opt-level=0")
            .arg(path);

        if let Some(linker) = self.rustc_linker.as_ref() {
            command.arg(format!("-Clinker={linker}"));
        }

        for (key, _) in std::env::vars() {
            const CARGO_FEATURE: &str = "CARGO_FEATURE_";
            if key.starts_with(CARGO_FEATURE) {
                command.arg("--cfg").arg(format!(
                    "feature=\"{}\"",
                    key.trim_start_matches(CARGO_FEATURE)
                        .to_lowercase()
                        .replace('_', "-")
                ));
            }
        }

        Ok(command.spawn()?.wait()?.success())
    }
}

fn env(name: &str) -> String {
    option_env(name).unwrap_or_else(|| panic!("build script missing {name:?} environment variable"))
}

fn option_env(name: &str) -> Option<String> {
    println!("cargo:rerun-if-env-changed={name}");
    std::env::var(name).ok()
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Emits cfgs describing which optional test dependencies are available for the
//! current target, so the tests can be gated on the reason rather than on a
//! repeated platform matrix.
//!
//! These must stay in sync with the corresponding `[target.'cfg(..)'.dependencies]`
//! sections in `Cargo.toml`; Cargo cannot select dependencies based on a cfg emitted
//! by a build script.

fn main() {
    println!("cargo::rustc-check-cfg=cfg(s2n_tls_provider)");
    println!("cargo::rustc-check-cfg=cfg(boringssl)");

    let target_os = env("CARGO_CFG_TARGET_OS");
    let target_env = env("CARGO_CFG_TARGET_ENV");
    let is_unix = std::env::var_os("CARGO_CFG_UNIX").is_some();
    let is_windows = target_os == "windows";

    // s2n-tls builds on unix and on Windows with the GNU/MinGW toolchain, but not with MSVC.
    if is_unix || (is_windows && target_env == "gnu") {
        println!("cargo::rustc-cfg=s2n_tls_provider");
    }

    // quiche depends on BoringSSL, which builds on unix and on Windows MSVC, but not with the
    // Windows MinGW-family toolchains.
    if !is_windows || target_env == "msvc" {
        println!("cargo::rustc-cfg=boringssl");
    }
}

fn env(name: &str) -> String {
    std::env::var(name)
        .unwrap_or_else(|_| panic!("build script missing {name:?} environment variable"))
}

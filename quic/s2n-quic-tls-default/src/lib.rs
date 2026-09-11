// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

// s2n-tls builds on unix and on Windows with the GNU/MinGW toolchain, but not with MSVC. Use it as
// the default provider wherever it builds; fall back to rustls elsewhere (notably Windows MSVC).
//
// Keep in sync with the target-specific dependencies in Cargo.toml.
#[cfg(not(any(unix, all(target_os = "windows", target_env = "gnu"))))]
pub use s2n_quic_rustls::*;
#[cfg(any(unix, all(target_os = "windows", target_env = "gnu")))]
pub use s2n_quic_tls::*;

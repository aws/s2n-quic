// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#[cfg(target_os = "linux")]
mod bpf;
// this module is used on platforms other than linux, but we still want to make
// sure it compiles
#[cfg_attr(target_os = "linux", allow(dead_code))]
mod pair;

#[cfg(target_os = "linux")]
pub use bpf::Pair;
#[cfg(not(target_os = "linux"))]
pub use pair::Pair;

pub use s2n_quic_platform::socket::options::{Options, ReusePort};

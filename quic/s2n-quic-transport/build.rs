// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

fn main() {
    println!("cargo::rustc-check-cfg=cfg(s2n_quic_dump_on_panic)");
    println!("cargo::rustc-check-cfg=cfg(feature, values(\"testing\"))");
}

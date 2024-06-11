// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use std::process::Command;

fn main() {
    let flags = Command::new("pkg-config")
        .arg("--libs")
        .arg("glib-2.0")
        .arg("wireshark")
        .output()
        .unwrap();

    for flag in core::str::from_utf8(&flags.stdout)
        .unwrap()
        .split(' ')
        .filter(|f| !f.is_empty())
    {
        println!("cargo:rustc-link-arg={flag}");
    }
}

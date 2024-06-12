// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

fn main() {
    let plugin_name = option_env("PLUGIN_NAME").unwrap_or_else(|| "dcQUIC".to_string());
    println!("cargo:rustc-env=PLUGIN_NAME={plugin_name}");
    println!(
        "cargo:rustc-env=PLUGIN_NAME_LOWER={}",
        plugin_name.to_lowercase()
    );

    // don't link any libraries and prefer pulling symbols from the wireshark/tshark binary
    if env("TARGET").contains("darwin") {
        println!("cargo:rustc-link-arg=-Wl,-undefined,dynamic_lookup");
    } else {
        println!("cargo:rustc-link-arg=-U");
    }
    println!("cargo:rustc-link-arg=-shared");
}

fn env<N: AsRef<str>>(name: N) -> String {
    let name = name.as_ref();
    option_env(name).unwrap_or_else(|| panic!("missing env {name}"))
}

fn option_env<N: AsRef<str>>(name: N) -> Option<String> {
    let name = name.as_ref();
    println!("cargo:rerun-if-env-changed={}", name);
    std::env::var(name).ok()
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

include!("update/src/lib.rs");

fn env(name: &str) -> Option<String> {
    println!("cargo:rerun-if-env-changed={}", name);
    std::env::var(name).ok()
}

fn is_vendored() -> bool {
    env("CARGO_FEATURE_VENDORED").is_some() && env("S2N_EXTERNAL_BUILD").is_none()
}

fn main() -> Result<(), Box<dyn 'static + std::error::Error>> {
    // if we've using vendored bindings then this doesn't need to be built

    if is_vendored() {
        let dst = cmake::Config::new("s2n")
            .register_dep("openssl")
            .build_arg("BUILD_TESTING=off")
            .build();

        println!(
            "cargo:rustc-link-search=native={}",
            dst.join("build").join("lib").display()
        );
        println!("cargo:rustc-cfg=vendored");
    } else {
        // Assume the caller is providing their own build of s2n

        println!("cargo:rerun-if-changed=s2n-sys.h");

        let bindings = s2n_tls_bindings(None)
            .generate()
            .map_err(|_| "failed to generate bindings")?;

        let output = Path::new(&env("OUT_DIR").unwrap()).join("bindings.rs");
        bindings.write_to_file(output)?;
    }

    println!("cargo:rustc-link-lib=static=s2n");
    println!("cargo:rustc-link-lib=static=crypto");

    Ok(())
}

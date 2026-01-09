// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

pub fn run() -> Result<(), anyhow::Error> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));

    let bindings = bindgen::Builder::default()
        .header(root.join("src/bindings/input.h").display().to_string())
        .allowlist_var("ETHTOOL_GCHANNELS")
        .allowlist_type("ethtool_channels")
        .rust_target(bindgen::RustTarget::stable(1, 47).map_err(|e| anyhow::anyhow!("{e}"))?)
        .layout_tests(false)
        .raw_line(
            r#"
// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#![allow(non_camel_case_types)]
            "#
            .trim(),
        )
        .generate()?;

    let out = root.join("../s2n-quic-xdp/src/bindings.rs");
    bindings.write_to_file(out)?;

    Ok(())
}

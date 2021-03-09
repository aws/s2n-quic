// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

const ENTRY: &str = r#"
#include <s2n.h>

#include "s2n/tls/s2n_quic_support.h"
"#;

const PRELUDE: &str = r#"
// This file needs to be generated any time the s2n-tls API changes.
//
// In order to regenerate the bindings, run the `update.sh` script.

use libc::{iovec, FILE};
"#;

pub fn s2n_tls_bindings(s2n_dir: Option<&str>) -> bindgen::Builder {
    let builder = bindgen::Builder::default()
        .use_core()
        .detect_include_paths(true)
        .size_t_is_usize(true)
        .rustfmt_bindings(false)
        .header_contents("s2n-sys.h", ENTRY)
        .default_enum_style(bindgen::EnumVariation::Rust {
            non_exhaustive: true,
        })
        // try to be compatible with older versions
        .rust_target(bindgen::RustTarget::Stable_1_36)
        // only export s2n-related stuff
        .blacklist_type("iovec")
        .blacklist_type("FILE")
        .blacklist_type("_IO_.*")
        .blacklist_type("__.*")
        // rust can't access thread-local variables
        // https://github.com/rust-lang/rust/issues/29594
        .blacklist_item("s2n_errno")
        .whitelist_type("s2n_.*")
        .whitelist_function("s2n_.*")
        .whitelist_var("s2n_.*")
        .rustified_enum("s2n_.*")
        .raw_line(PRELUDE)
        .ctypes_prefix("::libc")
        .parse_callbacks(Box::new(S2nCallbacks::default()));

    if let Some(s2n_dir) = s2n_dir {
        builder
            .clang_arg(format!("-I{}/api", s2n_dir))
            .clang_arg(format!("-I{}", s2n_dir))
    } else {
        builder
    }
}

#[derive(Debug)]
struct S2nCallbacks {
    cargo: bindgen::CargoCallbacks,
}

impl Default for S2nCallbacks {
    fn default() -> Self {
        Self {
            cargo: bindgen::CargoCallbacks,
        }
    }
}

impl bindgen::callbacks::ParseCallbacks for S2nCallbacks {
    fn enum_variant_name(
        &self,
        _enum_name: Option<&str>,
        variant_name: &str,
        _variant_value: bindgen::callbacks::EnumVariantValue,
    ) -> Option<String> {
        use heck::CamelCase;

        if !variant_name.starts_with("S2N_") {
            return None;
        }

        let variant_name = variant_name
            .trim_start_matches("S2N_ERR_T_")
            .trim_start_matches("S2N_EXTENSION_")
            // keep the LEN_ so it's a valid identifier
            .trim_start_matches("S2N_TLS_MAX_FRAG_")
            .trim_start_matches("S2N_ALERT_")
            .trim_start_matches("S2N_CT_SUPPORT_")
            .trim_start_matches("S2N_STATUS_REQUEST_")
            .trim_start_matches("S2N_CERT_AUTH_")
            // match everything else
            .trim_start_matches("S2N_");

        Some(variant_name.to_camel_case())
    }

    fn include_file(&self, filename: &str) {
        self.cargo.include_file(filename)
    }
}

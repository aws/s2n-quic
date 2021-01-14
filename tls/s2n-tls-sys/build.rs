use std::{env, path::Path};

fn is_vendored() -> bool {
    env::var("CARGO_FEATURE_VENDORED").is_ok() && !env::var("S2N_EXTERNAL_BUILD").is_ok()
}

fn main() -> Result<(), Box<dyn 'static + std::error::Error>> {
    // if we've using vendored bindings then this doesn't need to be built

    if is_vendored() {
        let dst = cmake::Config::new("s2n").register_dep("openssl").build();
        println!(
            "cargo:rustc-link-search=native={}",
            dst.join("build").join("lib").display()
        );
        println!("cargo:rustc-cfg=vendored");
    } else {
        // Assume the caller is providing their own build of s2n

        println!("cargo:rerun-if-changed=s2n-sys.h");

        let bindings = bindgen::Builder::default()
            .header("s2n-sys.h")
            .parse_callbacks(Box::new(bindgen::CargoCallbacks))
            .generate()
            .map_err(|_| "failed to generate bindings")?;

        let output = Path::new(&env::var("OUT_DIR").unwrap()).join("bindings.rs");
        bindings.write_to_file(output)?;
    }

    println!("cargo:rustc-link-lib=static=s2n");
    println!("cargo:rustc-link-lib=static=crypto");

    Ok(())
}

use std::env;

mod lib;

fn main() {
    let cwd = env::current_dir().unwrap();

    let s2n_dir = cwd.join("s2n").display().to_string();

    let bindings = lib::s2n_bindings(Some(&s2n_dir))
        .rustfmt_bindings(true)
        .generate()
        .expect("could not generate bindings");

    let out = env::args()
        .nth(1)
        .unwrap_or_else(|| cwd.join("src/vendored.rs").display().to_string());

    bindings.write_to_file(out).unwrap();
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use clap::{App, Arg};
pub use netbench::Result;
use std::path::Path;

pub mod config;
pub mod prelude;

pub trait Configs {
    fn registry() -> config::Registry;
    fn new(overrides: &mut config::Overrides) -> Self;
    fn write(self, out_dir: &Path) -> Result<()>;
}

#[doc(hidden)]
pub fn main<C: Configs>() -> Result<()> {
    let app = App::new("netbench scenarios")
        .after_help(LONG_ABOUT.trim())
        .arg(
            Arg::with_name("out_dir")
                .value_name("OUT_DIR")
                .default_value("target/netbench")
                .takes_value(true),
        );

    let map = C::registry();
    let args = map.clap_args().collect::<Vec<_>>();
    let matches = app.args(&args).get_matches();
    let mut overrides = map.load_overrides(&matches);

    let configs = C::new(&mut overrides);

    let mut has_error = false;
    for error in overrides.errors() {
        eprintln!("{error}");
        has_error = true;
    }

    if has_error {
        return Err("bailing due to errors".into());
    }

    let out_dir = matches.value_of("out_dir").unwrap();
    let out_dir = Path::new(out_dir);

    std::fs::create_dir_all(out_dir)?;
    configs.write(out_dir)?;

    Ok(())
}

const LONG_ABOUT: &str = r#"
FORMATS:
    BYTES
        42b         ->    42 bits
        42          ->    42 bytes
        42B         ->    42 bytes
        42K         ->    42000 bytes
        42Kb        ->    42000 bits
        42KB        ->    42000 bytes
        42KiB       ->    43008 bytes

    COUNT
        42          ->    42 units

    RATE
        42bps       ->    42 bits per second
        42Mbps      ->    42 megabits per second
        42MBps      ->    42 megabytes per second
        42MiBps     ->    42 mebibytes per second
        42MB/50ms   ->    42 megabytes per 50 milliseconds

    TIME
        42ms         ->    42 milliseconds
        42s          ->    42 seconds
        1s42ms       ->    1 second + 42 milliseconds
"#;

#[macro_export]
macro_rules! scenarios {
    ($($name:ident),* $(,)?) => {
        $(
            mod $name;
        )*

        #[derive(Debug)]
        pub struct Configs {
            $(
                $name: $name::Config,
            )*
        }

        impl $crate::Configs for Configs {
            fn registry() -> $crate::config::Registry {
                let mut registry = $crate::config::Registry::default();
                $(
                    $name::Config::register(&mut registry);
                )*
                registry
            }

            fn new(overrides: &mut $crate::config::Overrides) -> Self {
                Self {
                    $(
                        $name: $name::Config::new(overrides),
                    )*
                }
            }

            fn write(self, out_dir: &std::path::Path) -> $crate::Result<()> {
                $({
                    let path = out_dir.join(concat!(stringify!($name), ".json"));
                    let mut f = std::fs::File::create(&path)?;
                    let s = $name::scenario(self.$name);
                    s.write(&mut f)?;
                    eprintln!("created: {}", path.display());
                })*

                Ok(())
            }
        }

        fn main() -> $crate::Result<()> {
            $crate::main::<Configs>()
        }
    };
}

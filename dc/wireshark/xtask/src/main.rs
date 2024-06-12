// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use clap::Parser;
use xshell::{cmd, Shell};

type Error = Box<dyn std::error::Error>;
type Result<T = (), E = Error> = core::result::Result<T, E>;

#[derive(Debug, Parser)]
enum Args {
    Bindings(Bindings),
    Build(Build),
    Test(Test),
    Install(Install),
}

impl Args {
    fn run(self) {
        let sh = Shell::new().unwrap();
        match self {
            Self::Bindings(v) => v.run(&sh),
            Self::Build(v) => v.run(&sh),
            Self::Test(v) => v.run(&sh),
            Self::Install(v) => v.run(&sh),
        }
        .unwrap()
    }
}

#[derive(Debug, Parser)]
struct Bindings {}

impl Bindings {
    fn run(self, sh: &Shell) -> Result {
        // TODO port this script to rust
        cmd!(sh, "./generate-bindings.sh").run()?;
        Ok(())
    }
}

#[derive(Debug, Default, Parser)]
struct Build {
    #[arg(long)]
    profile: Option<String>,
    #[arg(long)]
    target: Option<String>,
}

impl Build {
    fn run(self, sh: &Shell) -> Result {
        let target = if let Some(target) = self.target.as_ref() {
            let _ = cmd!(sh, "rustup target add {target}").run();
            vec!["--target", target]
        } else {
            vec![]
        };
        let profile = self.profile.as_deref().unwrap_or("release");
        cmd!(sh, "cargo build --profile {profile} {target...}").run()?;
        Ok(())
    }
}

#[derive(Debug, Parser)]
struct Test {}

impl Test {
    fn run(self, sh: &Shell) -> Result {
        cmd!(sh, "cargo test").run()?;

        sh.create_dir("target/wireshark/plugins/4.2/epan")?;
        sh.create_dir("target/pcaps")?;

        // change the plugin name to avoid conflicts if it's already installed
        let plugin_name = "dcQUIC__DEV";
        let plugin_name_lower = &plugin_name.to_lowercase();
        let _env = sh.push_env("PLUGIN_NAME", plugin_name);

        let profile = "release-test";

        Build {
            profile: Some(profile.into()),
            ..Default::default()
        }
        .run(sh)?;

        let so = so();
        sh.copy_file(
            format!("target/{profile}/libwireshark_dcquic.{so}"),
            // wireshark always looks for `.so` regardless of platform
            "target/wireshark/plugins/4.2/epan/libdcquic.so",
        )?;

        cmd!(
            sh,
            "cargo run --release --bin generate-pcap -- target/pcaps/"
        )
        .run()?;

        let _env = sh.push_env("WIRESHARK_PLUGIN_DIR", "target/wireshark/plugins");

        let pcaps = [
            // TODO add more
            "pcaps/dcquic-stream-tcp.pcapng",
            "pcaps/dcquic-stream-udp.pcapng",
            // TODO figure out why this isn't parsing as dcQUIC
            // "target/pcaps/datagram.pcap",
        ];

        let tshark = tshark(sh)?;

        for pcap in pcaps {
            assert!(
                std::path::Path::new(pcap).exists(),
                "{pcap} is missing - git LFS is required to clone pcaps"
            );

            let cmd = cmd!(
                sh,
                "{tshark} -r {pcap} -2 -O {plugin_name_lower} -R {plugin_name_lower}"
            );

            let Ok(out) = cmd.output() else {
                // if the command failed then re-run it and print it to the console
                cmd.run()?;
                panic!("tshark did not exit successfully");
            };

            let stdout = core::str::from_utf8(&out.stdout).unwrap();
            let stderr = core::str::from_utf8(&out.stderr).unwrap();

            if !stderr.is_empty() {
                eprintln!("{pcap} STDERR\n{stderr}");
                // TODO fix the TCP implementation
                if !pcap.contains("tcp") {
                    panic!();
                }
            }

            assert!(stdout.contains(plugin_name), "{pcap} STDOUT:\n{stdout}");
        }

        Ok(())
    }
}

fn tshark(sh: &Shell) -> Result<String> {
    if let Ok(tshark) = cmd!(sh, "which tshark").read() {
        return Ok(tshark.trim().into());
    }

    if cfg!(target_os = "macos") {
        cmd!(sh, "brew install wireshark").run()?;
        Ok("tshark".into())
    } else if cfg!(target_os = "linux") {
        let is_nix = cmd!(sh, "which nix-shell").run().is_ok();
        if is_nix {
            return Ok(cmd!(
                sh,
                "nix-shell --packages wireshark --run 'echo -n $buildInputs'"
            )
            .read()?);
        }

        let is_apt = cmd!(sh, "which apt-get").run().is_ok();

        if is_apt {
            cmd!(sh, "sudo apt-get install tshark -y").run()?;
        }

        Ok("tshark".into())
    } else {
        Ok("tshark".into())
    }
}

fn so() -> &'static str {
    if cfg!(target_os = "macos") {
        "dylib"
    } else {
        "so"
    }
}

#[derive(Debug, Parser)]
struct Install {}

impl Install {
    fn run(self, sh: &Shell) -> Result {
        Build::default().run(sh)?;

        let dir = if cfg!(target_os = "macos") {
            "~/.local/lib/wireshark/plugins/4-2/epan"
        } else if cfg!(target_os = "linux") {
            "~/.local/lib/wireshark/plugins/4.2/epan"
        } else {
            todo!("OS is currently unsupported")
        };

        sh.create_dir(dir)?;
        let so = so();
        sh.copy_file(
            format!("target/release/libwireshark_dcquic.{so}"),
            // wireshark always looks for `.so`, regardless of platform
            format!("{dir}/libdcquic.so"),
        )?;

        Ok(())
    }
}

fn main() {
    Args::parse().run();
}

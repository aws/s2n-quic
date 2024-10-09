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
    #[command(flatten)]
    wireshark_version: WiresharkVersion,
}

impl Build {
    fn run(mut self, sh: &Shell) -> Result {
        self.wireshark_version.load(sh);

        let target = if let Some(target) = self.target.as_ref() {
            let _ = cmd!(sh, "rustup target add {target}").run();
            vec!["--target", target]
        } else {
            vec![]
        };
        let profile = self.profile.as_deref().unwrap_or("release");

        let _env = sh.push_env(
            "PLUGIN_MAJOR_VERSION",
            self.wireshark_version.major_version(),
        );
        let _env = sh.push_env(
            "PLUGIN_MINOR_VERSION",
            self.wireshark_version.minor_version(),
        );

        cmd!(sh, "cargo build --profile {profile} {target...}").run()?;
        Ok(())
    }
}

#[derive(Debug, Parser)]
struct Test {
    #[command(flatten)]
    wireshark_version: WiresharkVersion,
}

impl Test {
    fn run(mut self, sh: &Shell) -> Result {
        cmd!(sh, "cargo test").run()?;
        let plugin_dir = self.wireshark_version.plugin_dir(sh);

        sh.create_dir(format!("target/wireshark/{plugin_dir}"))?;
        sh.create_dir("target/pcaps")?;

        // change the plugin name to avoid conflicts if it's already installed
        let plugin_name = "dcQUIC__DEV";
        let plugin_name_lower = &plugin_name.to_lowercase();
        let _env = sh.push_env("PLUGIN_NAME", plugin_name);
        let _env = sh.push_env(
            "PLUGIN_MAJOR_VERSION",
            self.wireshark_version.major_version(),
        );
        let _env = sh.push_env(
            "PLUGIN_MINOR_VERSION",
            self.wireshark_version.minor_version(),
        );

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
            format!("target/wireshark/{plugin_dir}/lib{plugin_name_lower}.so"),
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
    } else if cfg!(target_os = "linux") {
        let is_nix = cmd!(sh, "which nix-shell").run().is_ok();
        if is_nix {
            return Ok(cmd!(sh, "nix-shell --packages tshark --run 'which tshark'").read()?);
        }

        let is_apt = cmd!(sh, "which apt-get").run().is_ok();

        if is_apt {
            cmd!(sh, "sudo apt-get install tshark -y").run()?;
        }
    }

    Ok("tshark".into())
}

fn so() -> &'static str {
    if cfg!(target_os = "macos") {
        "dylib"
    } else {
        "so"
    }
}

#[derive(Debug, Parser)]
struct Install {
    #[command(flatten)]
    wireshark_version: WiresharkVersion,
}

impl Install {
    fn run(mut self, sh: &Shell) -> Result {
        let plugin_dir = self.wireshark_version.plugin_dir(sh);

        Build {
            wireshark_version: self.wireshark_version.clone(),
            ..Default::default()
        }
        .run(sh)?;

        let dir = if cfg!(unix) {
            homedir::get_my_home()?
                .expect("missing home dir")
                .join(".local/lib/wireshark")
                .join(plugin_dir)
        } else {
            todo!("OS is currently unsupported")
        };

        sh.create_dir(&dir)?;
        let so = so();
        sh.copy_file(
            format!("target/release/libwireshark_dcquic.{so}"),
            // wireshark always looks for `.so`, regardless of platform
            dir.join("libdcquic.so"),
        )?;

        Ok(())
    }
}

#[derive(Clone, Debug, Default, Parser)]
struct WiresharkVersion {
    #[arg(long, default_value = "DYNAMIC")]
    wireshark_version: String,
}

impl WiresharkVersion {
    fn plugin_dir(&mut self, sh: &Shell) -> String {
        self.load(sh);

        let value = &self.wireshark_version;
        if cfg!(target_os = "macos") {
            format!("plugins/{}/epan", value.replace('.', "-"))
        } else {
            format!("plugins/{value}/epan")
        }
    }

    fn load(&mut self, sh: &Shell) {
        if !(self.wireshark_version.is_empty() || self.wireshark_version == "DYNAMIC") {
            return;
        }

        let tshark = tshark(sh).unwrap();
        let output = cmd!(sh, "{tshark} --version").read().unwrap();
        let version = output.lines().next().unwrap();
        let version = version.trim_start_matches(|v: char| !v.is_digit(10));
        let (version, _) = version
            .split_once(char::is_whitespace)
            .unwrap_or((version, ""));

        let version = version.trim_end_matches('.');

        match version.split('.').count() {
            2 => {
                self.wireshark_version = version.to_string();
            }
            3 => {
                let (version, _) = version.rsplit_once('.').unwrap();
                self.wireshark_version = version.to_string();
            }
            _ => panic!("invalid tshark version: {version}"),
        }
    }

    fn major_version(&self) -> &str {
        let (version, _) = self.wireshark_version.split_once('.').unwrap();
        version
    }

    fn minor_version(&self) -> &str {
        let (_, version) = self.wireshark_version.split_once('.').unwrap();
        version
    }
}

fn main() {
    Args::parse().run();
}

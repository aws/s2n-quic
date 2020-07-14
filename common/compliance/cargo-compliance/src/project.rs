use crate::Error;
use serde::Deserialize;
use std::{
    path::PathBuf,
    process::{Command, Stdio},
};
use structopt::StructOpt;

#[derive(Debug, PartialEq, PartialOrd, Eq, Ord, Hash, StructOpt)]
pub struct Project {
    /// Package to run tests for
    #[structopt(long, short = "p")]
    package: Option<String>,

    /// Space or comma separated list of features to activate
    #[structopt(long)]
    features: Vec<String>,

    /// Activate all available features
    #[structopt(long = "all-features")]
    all_features: bool,

    /// Do not activate the `default` feature
    #[structopt(long = "no-default-features")]
    no_default_features: bool,

    /// TRIPLE
    #[structopt(long)]
    target: Option<String>,

    /// Directory for all generated artifacts
    #[structopt(long = "target-dir", default_value = "target/compliance")]
    target_dir: String,

    /// Path to Cargo.toml
    #[structopt(long = "manifest-path")]
    manifest_path: Option<String>,
}

macro_rules! arg {
    ($cmd:ident, $name:expr, $value:expr) => {
        if let Some(value) = $value {
            $cmd.arg($name).arg(value);
        }
    };
}

macro_rules! flag {
    ($cmd:ident, $name:expr, $value:expr) => {
        if $value {
            $cmd.arg($name);
        }
    };
}

impl Project {
    pub fn executables(&self) -> Result<Vec<PathBuf>, Error> {
        let mut cmd = Command::new("cargo");

        cmd.stdout(Stdio::piped())
            .env("RUSTFLAGS", "--cfg compliance")
            .env("RUSTDOCFLAGS", "--cfg compliance")
            .arg("test")
            .arg("--no-run")
            .arg("--message-format")
            .arg("json-render-diagnostics");

        arg!(cmd, "--package", self.package.as_ref());
        arg!(
            cmd,
            "--features",
            Some(&self.features)
                .filter(|f| !f.is_empty())
                .map(|f| f.join(","))
        );
        flag!(cmd, "--all-features", self.all_features);
        flag!(cmd, "--no-default-features", self.no_default_features);
        arg!(cmd, "--target", self.target.as_ref());
        arg!(cmd, "--target-dir", Some(&self.target_dir));
        arg!(cmd, "--manifest-path", self.manifest_path.as_ref());

        let child = cmd.spawn()?;

        let output = child.wait_with_output()?;

        if !output.status.success() {
            return Err(format!("`cargo test` exited with status {}", output.status).into());
        }

        let stdout = core::str::from_utf8(&output.stdout)?;

        let mut executables = vec![];
        for line in stdout.lines() {
            if let Ok(event) = serde_json::from_str::<Event>(line) {
                if let Some(executable) = event.executable {
                    executables.push(PathBuf::from(executable))
                }
            }
        }

        Ok(executables)
    }
}

#[derive(Deserialize)]
struct Event<'a> {
    executable: Option<&'a str>,
}

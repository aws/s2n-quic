use crate::{pattern::Pattern, source::SourceFile, sourcemap::LinesIter, Error};
use glob::glob;
use serde::Deserialize;
use std::{
    collections::HashSet,
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

    /// Build all packages in the workspace
    #[structopt(long)]
    workspace: bool,

    /// Exclude packages from the test
    #[structopt(long = "exclude")]
    excludes: Vec<String>,

    /// Activate all available features
    #[structopt(long = "all-features")]
    all_features: bool,

    /// Do not activate the `default` feature
    #[structopt(long = "no-default-features")]
    no_default_features: bool,

    /// Disables running cargo commands
    #[structopt(long = "no-cargo")]
    no_cargo: bool,

    /// TRIPLE
    #[structopt(long)]
    target: Option<String>,

    /// Directory for all generated artifacts
    #[structopt(long = "target-dir", default_value = "target/compliance")]
    target_dir: String,

    /// Path to Cargo.toml
    #[structopt(long = "manifest-path")]
    manifest_path: Option<String>,

    /// Glob patterns for additional source files
    #[structopt(long = "source-pattern")]
    source_patterns: Vec<String>,

    /// Glob patterns for spec files
    #[structopt(long = "spec-pattern")]
    spec_patterns: Vec<String>,
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
    pub fn sources(&self) -> Result<HashSet<SourceFile>, Error> {
        let mut sources = HashSet::new();

        self.executables(&mut sources)?;

        self.cargo_files(&mut sources)?;

        for pattern in &self.source_patterns {
            self.source_file(&pattern, &mut sources)?;
        }

        for pattern in &self.spec_patterns {
            self.spec_file(&pattern, &mut sources)?;
        }

        Ok(sources)
    }

    fn executables(&self, sources: &mut HashSet<SourceFile>) -> Result<(), Error> {
        if self.no_cargo {
            return Ok(());
        }

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
        flag!(cmd, "--workspace", self.workspace);
        flag!(cmd, "--all-features", self.all_features);
        flag!(cmd, "--no-default-features", self.no_default_features);
        arg!(cmd, "--target", self.target.as_ref());
        arg!(cmd, "--target-dir", Some(&self.target_dir));
        arg!(cmd, "--manifest-path", self.manifest_path.as_ref());
        for exclude in &self.excludes {
            arg!(cmd, "--exclude", Some(exclude));
        }

        let child = cmd.spawn()?;

        let output = child.wait_with_output()?;

        if !output.status.success() {
            return Err(format!("`cargo test` exited with status {}", output.status).into());
        }

        let stdout = core::str::from_utf8(&output.stdout)?;

        for line in LinesIter::new(stdout) {
            if let Ok(event) = serde_json::from_str::<Event>(line.value) {
                if let Some(executable) = event.executable {
                    sources.insert(SourceFile::Object(PathBuf::from(executable)));
                }
            }
        }

        Ok(())
    }

    fn cargo_files(&self, _files: &mut HashSet<SourceFile>) -> Result<(), Error> {
        if self.no_cargo {
            return Ok(());
        }

        // TODO automatically populate file list from project files

        Ok(())
    }

    fn source_file<'a>(
        &self,
        pattern: &'a str,
        files: &mut HashSet<SourceFile<'a>>,
    ) -> Result<(), Error> {
        let (compliance_pattern, file_pattern) = if let Some(pattern) = pattern.strip_prefix('(') {
            let mut parts = pattern.splitn(2, ')');
            let pattern = parts.next().expect("invalid pattern");
            let file_pattern = parts.next().expect("invalid pattern");

            let pattern = Pattern::from_arg(pattern)?;

            (pattern, file_pattern)
        } else {
            (Pattern::default(), pattern)
        };

        for entry in glob(file_pattern)? {
            files.insert(SourceFile::Text(compliance_pattern, entry?));
        }

        Ok(())
    }

    fn spec_file<'a>(
        &self,
        pattern: &'a str,
        files: &mut HashSet<SourceFile<'a>>,
    ) -> Result<(), Error> {
        for entry in glob(pattern)? {
            files.insert(SourceFile::Spec(entry?));
        }

        Ok(())
    }
}

#[derive(Deserialize)]
struct Event<'a> {
    executable: Option<&'a str>,
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{pattern::Pattern, source::SourceFile, Error};
use glob::glob;
use std::collections::HashSet;
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

impl Project {
    pub fn sources(&self) -> Result<HashSet<SourceFile>, Error> {
        let mut sources = HashSet::new();

        self.cargo_files(&mut sources)?;

        for pattern in &self.source_patterns {
            self.source_file(pattern, &mut sources)?;
        }

        for pattern in &self.spec_patterns {
            self.spec_file(pattern, &mut sources)?;
        }

        Ok(sources)
    }

    #[allow(clippy::unnecessary_wraps)] // this function will eventually return something
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

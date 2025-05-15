// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    run::Config as Sim,
    stats::{Filter, Query},
    Result,
};
use anyhow::anyhow;
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
    process::Command,
    str::FromStr,
};
use structopt::StructOpt;

static INDEX: &str = include_str!("./batch.html");

#[derive(Debug, StructOpt)]
pub struct Batch {
    plans: Vec<Plan>,

    #[structopt(short, long, default_value = "target/s2n-quic-sim")]
    out: PathBuf,

    #[structopt(long)]
    skip_run: bool,
}

impl Batch {
    pub fn run(&self) -> Result {
        let out = &self.out;
        let command = std::env::args().next().unwrap();

        fs::create_dir_all(out)?;
        fs::write(out.join("index.html"), INDEX)?;

        let mut reports = vec![];

        for plan in &self.plans {
            plan.run(out, &command, self.skip_run, &mut reports)?;
        }

        // See https://github.com/rust-lang/rust-clippy/pull/12756
        #[allow(clippy::assigning_clones)]
        for (_title, report) in reports.iter_mut() {
            *report = report.strip_prefix(out).unwrap().to_owned();
        }

        serde_json::to_writer(fs::File::create(out.join("reports.json"))?, &reports)?;

        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize)]
struct Plan {
    #[serde(default)]
    name: Option<String>,

    sim: Sim,

    report: BTreeMap<String, Report>,
}

impl Plan {
    fn run(
        &self,
        out: &Path,
        command: &str,
        skip_run: bool,
        reports: &mut Vec<(String, PathBuf)>,
    ) -> Result {
        let name = self.name.as_ref().unwrap();
        eprintln!("     Running {name}");
        let out = out.join(name);
        fs::create_dir_all(&out)?;

        let db = out.join("db.proto");

        if !skip_run || !db.exists() {
            let status = Command::new(command)
                .arg("run")
                .arg("--progress")
                .args(self.sim.args())
                .stdout(fs::File::create(&db)?)
                .status()?;

            if !status.success() {
                return Err(anyhow!("run did not succeed"));
            }
        }

        for (report_name, report) in self.report.iter() {
            let mut res = report.run(&out, command, &db, report_name)?;
            res.0 = format!("{} - {}", name, res.0);
            reports.push(res);
        }

        Ok(())
    }
}

impl FromStr for Plan {
    type Err = io::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let file = fs::read_to_string(s)?;
        let mut plan: Self = toml::from_str(&file).map_err(io::Error::other)?;

        if plan.name.is_none() {
            plan.name = Some(
                Path::new(s.trim_end_matches(".toml"))
                    .file_name()
                    .and_then(|v| v.to_str())
                    .unwrap_or(s)
                    .to_owned(),
            );
        }

        Ok(plan)
    }
}

#[derive(Clone, Debug, Deserialize)]
struct Report {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    filters: Vec<Filter>,
    x: Query,
    y: Query,
}

impl Report {
    fn run(&self, out: &Path, command: &str, db: &Path, name: &str) -> Result<(String, PathBuf)> {
        let title = self.title.as_deref().unwrap_or(name).to_owned();
        let output = out.join(format!("{name}.json"));

        let mut cmd = Command::new(command);

        cmd.arg("report")
            .arg(db)
            .arg("--x")
            .arg(self.x.to_string())
            .arg("--y")
            .arg(self.y.to_string())
            .arg("--title")
            .arg(&title)
            .stdout(fs::File::create(&output)?);

        for filter in &self.filters {
            cmd.arg("--filter").arg(filter.to_string());
        }

        let status = cmd.status()?;

        if !status.success() {
            return Err(anyhow!("{} report did not succeed", name));
        }

        Ok((title, output))
    }
}

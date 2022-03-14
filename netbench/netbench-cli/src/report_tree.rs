// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{report::Report, Result};
use serde_json::json;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
pub struct ReportTree {
    input_dir: PathBuf,
    out_dir: PathBuf,
}

static INDEX_HTML: &str = include_str!("./report_tree.html");

type ScenarioMap = BTreeMap<String, Report>;

impl ReportTree {
    pub fn run(&self) -> Result<()> {
        let mut client_scenarios: ScenarioMap = Default::default();
        let mut server_scenarios: ScenarioMap = Default::default();

        for scenario in self.input_dir.read_dir()? {
            let scenario = scenario?;
            let path = scenario.path();
            let scenario_name = if let Some(name) = path_name(&path) {
                name
            } else {
                continue;
            };

            for driver in scenario.path().read_dir()? {
                let driver = driver?.path();

                macro_rules! push_scenario {
                    ($target:ident, $name:literal) => {{
                        let endpoint = driver.join(concat!($name, ".json"));
                        if endpoint.exists() {
                            $target
                                .entry(scenario_name.to_string())
                                .or_insert_with(|| Report {
                                    output: Some(
                                        self.out_dir
                                            .join(scenario_name)
                                            .join(concat!($name, "s.json")),
                                    ),
                                    ..Default::default()
                                })
                                .inputs
                                .push(endpoint);
                        }
                    }};
                }

                push_scenario!(client_scenarios, "client");
                push_scenario!(server_scenarios, "server");
            }
        }

        std::fs::create_dir_all(&self.out_dir)?;

        let index = {
            let template = handlebars::Handlebars::new();

            template.render_template(
                INDEX_HTML,
                &json!({
                    "clients": render_scenarios(client_scenarios)?,
                    "servers": render_scenarios(server_scenarios)?,
                }),
            )?
        };

        std::fs::write(self.out_dir.join("index.html"), index)?;

        Ok(())
    }
}

fn render_scenarios(scenarios: ScenarioMap) -> Result<Vec<String>> {
    let mut names = vec![];
    for (name, report) in scenarios {
        names.push(name);
        report.run()?;
    }
    Ok(names)
}

fn path_name(path: &Path) -> Option<&str> {
    let stem = path.file_stem()?.to_str()?;

    // filter out any hidden files
    if stem.starts_with('.') {
        return None;
    }

    Some(stem)
}

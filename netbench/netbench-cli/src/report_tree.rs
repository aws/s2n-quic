// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{report::Report, Result};
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

        let mut scenarios = String::new();

        fn push_scenarios(out: &mut String, name: &str, scenarios: &mut ScenarioMap) -> Result<()> {
            if scenarios.is_empty() {
                return Ok(());
            }

            out.push_str(&format!("<h3>{}</h3>", name));
            out.push_str("<ul>");
            for (scenario, report) in scenarios.iter_mut() {
                report.inputs.sort();
                report.run()?;

                out.push_str(&format!(
                    "<li><a href=\"#{}/{}.json\">{}</a></li>",
                    scenario,
                    name.to_lowercase(),
                    scenario,
                ));
            }
            out.push_str("</ul>");

            Ok(())
        }

        std::fs::create_dir_all(&self.out_dir)?;

        push_scenarios(&mut scenarios, "Clients", &mut client_scenarios)?;
        push_scenarios(&mut scenarios, "Servers", &mut server_scenarios)?;

        let index = INDEX_HTML.replace("__SCENARIOS__", &scenarios);
        std::fs::write(self.out_dir.join("index.html"), index)?;

        Ok(())
    }
}

static INDEX_HTML: &str = r#"
<!DOCTYPE html>
<html>
<head>
  <meta charset="UTF-8">
  <script src="https://cdn.jsdelivr.net/npm/vega@5"></script>
  <script src="https://cdn.jsdelivr.net/npm/vega-lite@4"></script>
  <script src="https://cdn.jsdelivr.net/npm/vega-embed@6"></script>
  <link rel="stylesheet" href="https://cdn.jsdelivr.net/npm/normalize.css@8.0.1/normalize.css">
</head>
<body>

<div id="vis"></div>

__SCENARIOS__

<script type="text/javascript">
  function onChange() {
    var spec = window.location.hash.replace(/^#/, '');
    if (spec) vegaEmbed('#vis', spec).catch(console.error);
  }

  onChange();
  window.onhashchange = onChange;
</script>
<style>
  body {
    box-sizing: border-box;
    font-family: sans-serif;
    padding: 20px;
  }

  .vega-bind-name {
    display: inline-block;
    min-width: 250px;
  }
</style>
</body>
</html>
"#;

fn path_name(path: &Path) -> Option<&str> {
    let stem = path.file_stem()?.to_str()?;

    // filter out any hidden files
    if stem.starts_with('.') {
        return None;
    }

    Some(stem)
}

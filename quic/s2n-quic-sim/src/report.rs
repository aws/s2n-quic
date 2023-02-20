// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    stats::{self, Connection, Filter, Parameters, Query, Stats, QUERY_NAMES},
    Result,
};
use serde_json::json;
use std::{
    collections::{BTreeSet, HashMap},
    fs, io,
    path::PathBuf,
};
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
pub struct Report {
    #[structopt(long, short)]
    filter: Vec<Filter>,

    #[structopt(long, short, possible_values = &*QUERY_NAMES)]
    x: Query,

    #[structopt(long, default_value = "100")]
    x_width: u32,

    #[structopt(long, short, possible_values = &*QUERY_NAMES)]
    y: Query,

    #[structopt(long, default_value = "100")]
    y_width: u32,

    #[structopt(long)]
    title: Option<String>,

    #[structopt(long, default_value = "blues")]
    palette: String,

    input: PathBuf,
}

impl Report {
    pub fn run(&self) -> Result {
        let reader = fs::File::open(&self.input)?;
        let reader = io::BufReader::new(reader);
        let reader = Stats::reader(reader);

        let filters = &self.filter;

        let mut seed_ids: HashMap<u64, usize> = HashMap::new();
        let mut seed_hist = vec![];
        let mut acc: HashMap<usize, Vec<Connection>> = HashMap::new();
        let mut values = vec![];
        let mut x_bounds = Bounds::default();
        let mut y_bounds = Bounds::default();

        let mut seed_id = |seed: u64| {
            use std::collections::hash_map::Entry;
            let next_id = seed_ids.len();
            match seed_ids.entry(seed) {
                Entry::Occupied(entry) => *entry.get(),
                Entry::Vacant(entry) => {
                    entry.insert(next_id);

                    seed_hist.push((seed, vec![], vec![]));

                    next_id
                }
            }
        };

        let mut q =
            |seed_id: usize, p: &Parameters, conn: &Connection, connections: &[Connection]| {
                for filter in filters {
                    if !filter.apply(p, conn, connections) {
                        return;
                    }
                }

                let x = self.x.apply(p, conn, connections);
                let y = self.y.apply(p, conn, connections);

                if let (Some(x), Some(y)) = (x, y) {
                    x_bounds.push(x);
                    y_bounds.push(y);
                    values.push((seed_id, conn.id(), x, y));
                }
            };

        let mut args = vec![];

        for stat in reader {
            match stat? {
                Stats::Setup(s) => {
                    args = s.args;
                }
                Stats::Parameters(p) => {
                    let id = seed_id(p.seed);
                    if let Some(connections) = acc.remove(&id) {
                        for conn in &connections {
                            q(id, &p, conn, &connections);
                        }
                    }
                }
                Stats::Connection(c) => {
                    let id = seed_id(c.seed);
                    acc.entry(id).or_default().push(c);
                }
            }
        }

        let x_width = self.x_width;
        let y_width = self.y_width;
        let mut hist = vec![(0u32, 0u32, BTreeSet::new()); x_width as usize * y_width as usize];

        let x_bin = x_bounds.bin(x_width as usize);
        let y_bin = y_bounds.bin(y_width as usize);

        let mut max_depth = 0;
        for (seed, _id, x, y) in values.iter().copied() {
            let x = x_bin.bin(x);
            let y = y_bin.bin(y);

            let idx = x + y * self.x_width as usize;
            let entry = &mut hist[idx];
            let value = entry.1 + 1;

            entry.0 = idx as _;
            entry.1 = value;
            entry.2.insert(seed);
            max_depth = max_depth.max(value);

            let entry = &mut seed_hist[seed];
            if entry.1.is_empty() {
                entry.1 = vec![(0u32, 0u32); x_width as usize];
            }
            if entry.2.is_empty() {
                entry.2 = vec![(0u32, 0u32); y_width as usize];
            }

            // increment the x hist
            entry.1[x].0 = x as _;
            entry.1[x].1 += 1;
            // increment the y hist
            entry.2[y].0 = y as _;
            entry.2[y].1 += 1;
        }

        //return Ok(());

        // remove any unused entries
        hist.retain(|(_idx, count, _seeds)| *count > 0);

        for (_seed, x_hist, y_hist) in seed_hist.iter_mut() {
            x_hist.retain(|(_idx, count)| *count > 0);
            y_hist.retain(|(_idx, count)| *count > 0);
        }

        let title = self
            .title
            .clone()
            .unwrap_or_else(|| format!("{} vs {}", self.x, self.y));

        let total_height = 1200;
        let total_width = 1200;
        let size = 600;
        let hist_width = size / self.x_width;
        let hist_height = size / self.y_width;

        let group_width = 40;
        let group_height = 15;
        let group_padding = 60;
        let group_offset = size + group_padding;

        let seed_hist_size = size - group_padding;
        let groups_per_line = seed_hist_size / group_width;

        let command = format!(
            "{:?} + (isNumber({s}) ? (' --seed ' + {s}) : '')",
            args.join(" "),
            s = "sig$seed_hist.seed"
        );

        let vega = json!({
            "$schema": "https://vega.github.io/schema/vega/v5.json",
            "width": total_width,
            "height": total_height,
            "padding": 5,
            "background": "white",

            "title": {
                "text": title,
                "anchor": "middle",
                "fontSize": 16,
                "frame": "group",
                "offset": 4
            },

            "signals": [
                {
                    "name": "sig$group",
                    "value": [],
                    "on": [{ "events": "@hist:click", "update": "datum.group" }]
                },
                {
                    "name": "sig$seed",
                    "on": [
                        { "events": "@hist:click", "update": "0" },
                        { "events": "@seed_id:click", "update": "datum.index" }
                    ]
                },
                {
                    "name": "sig$seed_hist",
                    "update": "isNumber(sig$seed) ? data('data$seed_hist')[sig$group[sig$seed]] : {}"
                },
                {
                    "name": "sig$command",
                    "update": command,
                }
            ],

            "scales": [
                {
                    "name": "scale$hist_x",
                    "type": "linear",
                    "domain": [0, self.x_width + 1],
                    "range": [0, size]
                },
                {
                    "name": "scale$axis_x",
                    "type": "linear",
                    "domain": x_bounds.domain(self.x.ty),
                    "range": [0, size],
                    "nice": true,
                    "zero": false,
                },
                {
                    "name": "scale$hist_y",
                    "type": "linear",
                    "domain": [0, self.y_width + 1],
                    "range": [0, size]
                },
                {
                    "name": "scale$axis_y",
                    "type": "linear",
                    "domain": y_bounds.domain(self.y.ty),
                    "range": [0, size],
                    "reverse": true,
                    "nice": true,
                    "zero": false,
                },
                {
                    "name": "scale$seed_hist_x",
                    "type": "linear",
                    "domain": [0, { "signal": "sig$seed_hist_x_extent[1]" }],
                    "range": [0, seed_hist_size]
                },
                {
                    "name": "scale$seed_axis_x",
                    "type": "linear",
                    "domain": { "signal": "sig$seed_hist_x_extent" },
                    "range": [group_offset, total_height],
                    "nice": true,
                    "zero": false,
                },
                {
                    "name": "scale$seed_hist_y",
                    "type": "linear",
                    "domain": [0, { "signal": "sig$seed_hist_y_extent[1]" }],
                    "range": [0, seed_hist_size]
                },
                {
                    "name": "scale$seed_axis_y",
                    "type": "linear",
                    "domain": { "signal": "sig$seed_hist_y_extent" },
                    "range": [group_offset, total_height],
                    "nice": true,
                    "zero": false,
                },
                {
                    "name": "scale$color",
                    "type": "linear",
                    "range": { "scheme": self.palette },
                    "domain": [0, max_depth]
                }
            ],

            "legends": [
                {
                    "orient": "none",
                    "fill": "scale$color",
                    "type": "gradient",
                    "direction": "horizontal",
                    "gradientLength": size,
                    "legendY": size + 20,
                    "legendX": 10
                }
            ],

            "axes": [
                {
                    "orient": "top",
                    "scale": "scale$axis_x",
                    "domain": false,
                    "title": self.x.to_string(),
                    "format": self.x.ty.format(x_bounds.domain(self.x.ty)),
                    "formatType": if self.x.ty.is_duration() {
                        "time"
                    } else {
                        "number"
                    },
                },
                {
                    "orient": "bottom",
                    "scale": "scale$axis_x",
                    "domain": false,
                    "title": self.x.to_string(),
                    "format": self.x.ty.format(x_bounds.domain(self.x.ty)),
                    "formatType": if self.x.ty.is_duration() {
                        "time"
                    } else {
                        "number"
                    },
                },
                {
                    "orient": "left",
                    "scale": "scale$seed_axis_x",
                    "domain": false,
                    "title": "Count",
                },
                {
                    "orient": "left",
                    "scale": "scale$axis_y",
                    "domain": false,
                    "title": self.y.to_string(),
                    "format": self.y.ty.format(y_bounds.domain(self.y.ty)),
                    "formatType": if self.y.ty.is_duration() {
                        "time"
                    } else {
                        "number"
                    },
                },
                {
                    "orient": "top",
                    "scale": "scale$seed_axis_y",
                    "domain": false,
                    "title": "Count",
                },
                {
                    "orient": "right",
                    "scale": "scale$axis_y",
                    "domain": false,
                    "title": self.y.to_string(),
                    "format": self.y.ty.format(y_bounds.domain(self.y.ty)),
                    "formatType": if self.y.ty.is_duration() {
                        "time"
                    } else {
                        "number"
                    },
                }
            ],

            "marks": [
                {
                    "name": "hist",
                    "type": "rect",
                    "from": { "data": "data$hist" },
                    "encode": {
                        "enter": {
                            "width": { "value": hist_width },
                            "height": { "value": hist_height },
                        },
                        "update": {
                            "x": { "scale": "scale$hist_x", "signal": "datum.x" },
                            "y": { "scale": "scale$hist_y", "signal": "datum.y" },
                            "fill": { "scale": "scale$color", "field": "count" },
                            "tooltip": { "signal": "datum.count" },
                        },
                    }
                },
                {
                    "name": "seed_hist_x",
                    "type": "rect",
                    "from": { "data": "data$seed_hist_x" },
                    "encode": {
                        "enter": {
                            "y": { "value": group_offset },
                            "width": { "value": hist_width },
                        },
                        "update": {
                            "x": { "scale": "scale$hist_x", "signal": "datum.x" },
                            "height": { "scale": "scale$seed_hist_x", "field": "count" },
                            "fill": { "scale": "scale$color", "field": "count" },
                            "tooltip": { "signal": "datum.count" },
                        },
                    }
                },
                {
                    "name": "seed_hist_y",
                    "type": "rect",
                    "from": { "data": "data$seed_hist_y" },
                    "encode": {
                        "enter": {
                            "x": { "value": group_offset },
                            "height": { "value": hist_height },
                        },
                        "update": {
                            "y": { "scale": "scale$hist_y", "signal": "datum.y" },
                            "width": { "scale": "scale$seed_hist_y", "field": "count" },
                            "fill": { "scale": "scale$color", "field": "count" },
                            "tooltip": { "signal": "datum.count" },
                        },
                    }
                },
                {
                    "type": "text",
                    "from": { "data": "data$group" },
                    "encode": {
                        "enter": {
                            "width": { "value": group_width },
                            "height": { "value": group_height },
                        },
                        "update": {
                            "x": { "signal": "datum.x" },
                            "y": { "signal": "datum.y" },
                            "text": { "signal": "datum.index + 1" },
                        }
                    }
                },
                {
                    "name": "seed_id",
                    "type": "rect",
                    "from": { "data": "data$group" },
                    "encode": {
                        "enter": {
                            "width": { "value": group_width },
                            "height": { "value": group_height },
                            "fill": { "value": "#125ca4" },
                        },
                        "update": {
                            "x": { "signal": "datum.x - 5" },
                            "y": { "signal": "datum.y - 12" },
                            "fillOpacity": { "signal": "sig$seed == datum.index ? 0.3 : 0.0" },
                        }
                    }
                },
            ],

            "data": [
                {
                    "name": "data$hist",
                    "values": &hist,
                    "transform": [
                        {
                            "type": "formula",
                            "as": "index",
                            "expr": "datum[0]",
                        },
                        {
                            "type": "formula",
                            "as": "count",
                            "expr": "datum[1]",
                        },
                        {
                            "type": "formula",
                            "as": "group",
                            "expr": "datum[2]",
                        },
                        {
                            "type": "formula",
                            "as": "x",
                            "expr": format!("floor(datum.index % {x_width})"),
                        },
                        {
                            "type": "formula",
                            "as": "y",
                            "expr": format!("{y_width} - floor(datum.index / {y_width})"),
                        },
                    ],
                },
                {
                    "name": "data$seed_hist",
                    "values": &seed_hist,
                    "transform": [
                        {
                            "type": "formula",
                            "as": "seed",
                            "expr": "datum[0]",
                        },
                        {
                            "type": "formula",
                            "as": "x_hist",
                            "expr": "datum[1]",
                        },
                        {
                            "type": "formula",
                            "as": "y_hist",
                            "expr": "datum[2]",
                        },
                    ],
                },
                {
                    "name": "data$seed_hist_x",
                    "values": { "signal": "sig$seed_hist.x_hist" },
                    "transform": [
                        {
                            "type": "formula",
                            "as": "index",
                            "expr": "datum[0]",
                        },
                        {
                            "type": "formula",
                            "as": "count",
                            "expr": "datum[1]",
                        },
                        {
                            "type": "formula",
                            "as": "x",
                            "expr": "datum.index",
                        },
                        {
                            "type": "extent",
                            "field": "count",
                            "signal": "sig$seed_hist_x_extent"
                        },
                    ],
                },
                {
                    "name": "data$seed_hist_y",
                    "values": { "signal": "sig$seed_hist.y_hist" },
                    "transform": [
                        {
                            "type": "formula",
                            "as": "index",
                            "expr": "datum[0]",
                        },
                        {
                            "type": "formula",
                            "as": "count",
                            "expr": "datum[1]",
                        },
                        {
                            "type": "formula",
                            "as": "y",
                            "expr": format!("{y_width} - datum.index"),
                        },
                        {
                            "type": "extent",
                            "field": "count",
                            "signal": "sig$seed_hist_y_extent"
                        },
                    ],
                },
                {
                    "name": "data$group",
                    "transform": [
                        {
                            "type": "sequence",
                            "start": 0,
                            "stop": { "signal": "length(sig$group)" },
                            "as": "index",
                        },
                        {
                            "type": "formula",
                            "as": "x",
                            "expr": format!("{} + floor(datum.index % {}) * {}", group_offset + 20, groups_per_line, group_width),
                        },
                        {
                            "type": "formula",
                            "as": "y",
                            "expr": format!("{} + floor(datum.index / {}) * {}", group_offset + 20, groups_per_line, group_height),
                        },
                    ]
                },
            ],
        });

        println!("{vega}");

        Ok(())
    }
}

#[derive(Clone, Copy, Debug)]
struct Bounds {
    min: f64,
    max: f64,
}

impl Default for Bounds {
    fn default() -> Self {
        Self {
            min: f64::MAX,
            max: f64::MIN,
        }
    }
}

impl Bounds {
    pub fn push(&mut self, value: f64) {
        self.min = self.min.min(value);
        self.max = self.max.max(value);
    }

    pub fn bin(&self, width: usize) -> Bin {
        Bin {
            min: self.min,
            step: (self.max - self.min) / (width - 1) as f64,
        }
    }

    pub fn domain(&self, ty: stats::Type) -> [f64; 2] {
        if ty.is_duration() {
            [self.min * 1000.0, self.max * 1000.0]
        } else {
            [self.min, self.max]
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Bin {
    min: f64,
    step: f64,
}

impl Bin {
    pub fn bin(&self, value: f64) -> usize {
        ((value - self.min) / self.step) as usize
    }
}

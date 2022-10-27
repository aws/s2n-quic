// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::Result;
use netbench::stats::{Initialize, Stats};
use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::{BTreeSet, HashMap},
    io::BufRead,
    path::PathBuf,
};
use structopt::StructOpt;

static VEGA: &str = include_str!("./vega.json");
static VEGA_PARSED: OnceCell<serde_json::Value> = OnceCell::new();

fn vega_template() -> serde_json::Value {
    VEGA_PARSED
        .get_or_init(|| serde_json::from_str(VEGA).unwrap())
        .clone()
}

#[derive(Debug, Default, StructOpt)]
pub struct Report {
    pub inputs: Vec<PathBuf>,
    #[structopt(short, long)]
    pub output: Option<PathBuf>,
}

impl Report {
    pub fn run(&self) -> Result<()> {
        let mut stats_table = vec![];
        let mut stream_table = vec![];
        let mut signals = vec![];
        let mut names = vec![];
        let mut scenario_names = BTreeSet::new();
        let mut stream_ids = HashMap::new();
        let mut trace_ids = vec![];
        let mut pids = vec![];

        for (pid, input) in self.inputs.iter().enumerate() {
            let pid = pid as u64;
            let input = std::fs::File::open(input)?;
            let mut input = std::io::BufReader::new(input);

            let mut first = String::new();
            input.read_line(&mut first)?;
            let Initialize {
                driver,
                scenario,
                traces,
                ..
            } = serde_json::from_str(&first)?;

            let name = driver
                .split('/')
                .last()
                .unwrap()
                .trim_start_matches("netbench-driver-")
                .to_string();

            let scenario_name = scenario
                .split('/')
                .last()
                .unwrap()
                .trim_end_matches(".json");

            scenario_names.insert(scenario_name.to_string());

            pids.push(format!("!indata('data$hidden', 'name', {:?})", name));
            names.push(name);

            let input = serde_json::de::IoRead::new(input);
            let input = serde_json::StreamDeserializer::new(input);
            let mut prev_x = 0;
            for event in input {
                let Stats {
                    time,
                    cpu,
                    cycles,
                    instructions,
                    branches,
                    context_switches,
                    memory,
                    virtual_memory,
                    allocs,
                    reallocs,
                    deallocs,
                    syscalls,
                    connections,
                    accept,
                    send,
                    receive,
                    connect_time,
                    profiles,
                } = event?;

                let x = time.as_millis() as u64;

                macro_rules! emit {
                    ($name:ident, $value:expr) => {{
                        let mut y = $value as f64;
                        if !f64::is_normal(y) {
                            y = 0.0;
                        }
                        stats_table.push(Row {
                            x,
                            y,
                            pid,
                            stat: Stat::$name as _,
                            stream_id: None,
                        });
                    }};
                    ($name:ident, $value:expr, $id:expr) => {{
                        let mut y = $value as f64;
                        if !f64::is_normal(y) {
                            y = 0.0;
                        }
                        let next_id = stream_ids.len() as u64;
                        let stream_id = stream_ids.entry((pid, $id)).or_insert_with(|| {
                            // area charts need another point so zero it out first
                            stream_table.push(Row {
                                x: prev_x,
                                y: 0.0,
                                pid,
                                stat: Stat::StreamSendBytes as _,
                                stream_id: Some(next_id),
                            });
                            stream_table.push(Row {
                                x: prev_x,
                                y: 0.0,
                                pid,
                                stat: Stat::StreamReceiveBytes as _,
                                stream_id: Some(next_id),
                            });
                            next_id
                        });
                        stream_table.push(Row {
                            x,
                            y,
                            pid,
                            stat: Stat::$name as _,
                            stream_id: Some(*stream_id),
                        });
                    };};
                }

                emit!(Cpu, cpu);
                emit!(Memory, memory);
                emit!(VirtualMemory, virtual_memory);
                emit!(Cycles, cycles);
                emit!(Instructions, instructions);
                emit!(Branches, branches);
                emit!(ContextSwitches, context_switches);
                emit!(Syscalls, syscalls);
                emit!(Connections, connections);
                emit!(Accept, accept);
                emit!(AllocBytes, allocs.total);
                emit!(AllocCount, allocs.count);
                emit!(ReallocBytes, reallocs.total);
                emit!(ReallocCount, reallocs.count);
                emit!(DeallocBytes, deallocs.total);
                emit!(DeallocCount, deallocs.count);

                {
                    let mut bytes = 0;
                    let mut count = 0;
                    for (id, s) in send {
                        bytes += s.total;
                        count += s.count;
                        emit!(StreamSendBytes, s.total, id);
                    }
                    emit!(SendBytes, bytes);
                    emit!(SendCount, count);
                    emit!(SendBytesPerCpu, bytes as f64 / cpu as f64);
                    emit!(SendBytesPerInstruction, bytes as f64 / instructions as f64);
                }

                {
                    let mut bytes = 0;
                    let mut count = 0;
                    for (id, s) in receive {
                        bytes += s.total;
                        count += s.count;
                        emit!(StreamReceiveBytes, s.total, id);
                    }
                    emit!(ReceiveBytes, bytes);
                    emit!(ReceiveCount, count);
                    emit!(ReceiveBytesPerCpu, bytes as f64 / cpu as f64);
                    emit!(
                        ReceiveBytesPerInstruction,
                        bytes as f64 / instructions as f64
                    );
                }

                {
                    let mut y = connect_time.average();

                    if !f64::is_normal(y) {
                        y = 0.0;
                    }

                    // convert micros to seconds
                    y /= 1_000_000.0;

                    stats_table.push(Row {
                        x,
                        y,
                        pid,
                        stat: Stat::ConnectTime as _,
                        stream_id: None,
                    });
                }

                for (trace_id, hist) in profiles {
                    let trace = &traces[trace_id as usize];
                    let trace_id = if let Some(id) = trace_ids.iter().position(|v| v == trace) {
                        id as u64
                    } else {
                        let id = trace_ids.len() as u64;
                        trace_ids.push(trace.to_string());
                        id
                    };

                    // offset the stat id with the built-in names
                    let stat = Stat::NAMES.len() as u64 + trace_id;

                    let mut y = hist.stat.average();
                    if !f64::is_normal(y) {
                        y = 0.0;
                    }

                    // convert micros to seconds
                    let y = y / 1_000_000.0;

                    stats_table.push(Row {
                        x,
                        y,
                        pid,
                        stat,
                        stream_id: None,
                    });

                    /*
                     // TODO figure out how to visualize multiple histograms over time
                    for bucket in hist.buckets {
                        profile_hist_table.push(Bucket {
                            x,
                            pid,
                            trace_id,
                            lower: bucket.lower,
                            upper: bucket.upper,
                            count: bucket.count,
                        });
                    }
                    */
                }

                prev_x = x;
            }
        }

        stats_table.sort_by(|a, b| {
            a.x.cmp(&b.x)
                .then(a.pid.cmp(&b.pid))
                .then(a.stat.cmp(&b.stat))
        });
        stream_table.sort_by(|a, b| {
            a.x.cmp(&b.x)
                .then(a.pid.cmp(&b.pid))
                .then(a.stat.cmp(&b.stat))
                .then(a.stream_id.cmp(&b.stream_id))
        });

        let mut view_names = Stat::NAMES
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>();

        for trace in &trace_ids {
            view_names.push(format!("trace - {trace}"));
        }

        // expose an option to select the view
        signals.push(json!({
            "name": "ui$view",
            "value": &view_names[0],
            "bind": {
                "input": "select",
                "name": "View",
                "options": view_names,
            },
        }));

        // translate the view name into an index
        signals.push(json!({
            "name": "sig$view",
            "value": "0",
            "update": format!("indexof({:?},ui$view)", view_names),
        }));

        signals.push(json!({
            "name": "pids",
            "value": "[]",
            "update": format!("[{}]", pids.join(",")),
        }));

        let mut stream_counts = vec![0u64; pids.len()];
        for (pid, _) in stream_ids.keys() {
            stream_counts[*pid as usize] += 1;
        }

        let mut stream_count_expr = "0".to_string();
        for (id, count) in stream_counts.iter().enumerate() {
            use core::fmt::Write;
            let _ = write!(stream_count_expr, "+(pids[{}]?{}:0)", id, count);
        }

        signals.push(json!({
            "name": "sig$streamCount",
            "update": stream_count_expr,
        }));

        signals.push(json!({
            "name": "sig$streamTypes",
            "update": format!("{{{}:(ui$sendStreams?-1:0),{}:(ui$recvStreams?1:0)}}", Stat::StreamSendBytes as u64, Stat::StreamReceiveBytes as u64),
        }));

        let mut output: serde_json::Value = vega_template();

        let root = output.as_object_mut().unwrap();

        root.insert(
            "title".to_string(),
            scenario_names
                .iter()
                .map(|v| v.as_str())
                .collect::<Vec<_>>()
                .join(", ")
                .into(),
        );

        root.get_mut("signals")
            .unwrap()
            .as_array_mut()
            .unwrap()
            .extend(signals);

        let data = root.get_mut("data").unwrap().as_array_mut().unwrap();

        data.insert(
            0,
            json!({
                "name": "data$stats",
                "values": &stats_table,
            }),
        );
        data.insert(
            1,
            json!({
                "name": "data$streams",
                "values": &stream_table,
            }),
        );
        data.insert(
            2,
            json!({
                "name": "data$drivers",
                "values": names.iter().map(|name|{
                    json!({ "name": name })
                }).collect::<Vec<_>>()
            }),
        );

        if let Some(out_path) = self.output.as_ref() {
            if let Some(parent) = out_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut out_file = std::fs::File::create(out_path)?;
            serde_json::to_writer(&mut out_file, &output)?;
        } else {
            serde_json::to_writer(std::io::stdout(), &output)?;
        }

        Ok(())
    }
}

macro_rules! stat {
    (enum Stat { $($name:ident = $desc:expr),* $(,)? }) => {
        #[repr(u64)]
        enum Stat {
            $(
                $name,
            )*
        }

        impl Stat {
            const NAMES: &'static [&'static str] = &[$($desc,)*];
        }
    };
}

stat!(
    enum Stat {
        Cpu = "cpu %",
        Memory = "memory (bytes)",
        VirtualMemory = "virtual memory (bytes)",
        Cycles = "cycles",
        Instructions = "instructions",
        Branches = "branches",
        ContextSwitches = "context-switches",
        Syscalls = "syscalls",
        Connections = "connections",
        ConnectTime = "connect-time",
        Accept = "accept (streams)",
        AllocBytes = "alloc (bytes)",
        AllocCount = "alloc (count)",
        ReallocBytes = "realloc (bytes)",
        ReallocCount = "realloc (count)",
        DeallocBytes = "dealloc (bytes)",
        DeallocCount = "dealloc (count)",
        SendBytes = "send (bytes)",
        SendCount = "send (count)",
        SendBytesPerCpu = "send (bytes/cpu %)",
        SendBytesPerInstruction = "send (bytes/instruction)",
        ReceiveBytes = "receive (bytes)",
        ReceiveCount = "receive (count)",
        ReceiveBytesPerCpu = "receive (bytes/cpu %)",
        ReceiveBytesPerInstruction = "receive (bytes/instruction)",
        StreamSendBytes = "stream send",
        StreamReceiveBytes = "stream receive",
    }
);

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
struct Row {
    x: u64,
    y: f64,
    #[serde(rename = "p")]
    pid: u64,
    #[serde(rename = "s")]
    stat: u64,
    #[serde(rename = "i", skip_serializing_if = "Option::is_none")]
    stream_id: Option<u64>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
struct Bucket {
    x: u64,
    #[serde(rename = "p")]
    pid: u64,
    #[serde(rename = "t")]
    trace_id: u64,
    #[serde(rename = "l")]
    lower: f64,
    #[serde(rename = "u")]
    upper: f64,
    #[serde(rename = "c")]
    count: u64,
}

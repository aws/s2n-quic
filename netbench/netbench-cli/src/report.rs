// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::Result;
use netbench::stats::{Initialize, Stats};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{collections::HashMap, io::BufRead, path::PathBuf};
use structopt::StructOpt;

#[derive(StructOpt)]
pub struct Report {
    inputs: Vec<PathBuf>,
}

impl Report {
    pub fn run(&self) -> Result<()> {
        let mut stats_table = vec![];
        let mut stream_table = vec![];
        let mut signals = vec![];
        let mut names = vec![];

        // expose an option to select the view
        signals.push(json!({
            "name": "ui$view",
            "value": Stat::NAMES[0],
            "bind": {
                "input": "select",
                "name": "View",
                "options": Stat::NAMES,
            },
        }));

        // translate the view name into an index
        signals.push(json!({
            "name": "sig$view",
            "value": "0",
            "update": format!("indexof({:?},ui$view)", Stat::NAMES),
        }));

        let mut stream_ids = HashMap::new();
        let mut pids = vec![];

        for (pid, input) in self.inputs.iter().enumerate() {
            let pid = pid as u64;
            let input = std::fs::File::open(input)?;
            let mut input = std::io::BufReader::new(input);

            let mut first = String::new();
            input.read_line(&mut first)?;
            let Initialize {
                driver, scenario, ..
            } = serde_json::from_str(&first)?;

            let cmd = driver
                .split('/')
                .last()
                .unwrap()
                .trim_start_matches("netbench-driver-");
            let scenario = scenario
                .split('/')
                .last()
                .unwrap()
                .trim_end_matches(".json");

            let name = format!("{} {}", cmd, scenario);
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
                    send,
                    receive,
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
            stream_count_expr.push_str(&format!("+(pids[{}]?{}:0)", id, count));
        }

        signals.push(json!({
            "name": "sig$streamCount",
            "update": stream_count_expr,
        }));

        signals.push(json!({
            "name": "sig$streamTypes",
            "update": format!("{{{}:(ui$sendStreams?-1:0),{}:(ui$recvStreams?1:0)}}", Stat::StreamSendBytes as u64, Stat::StreamReceiveBytes as u64),
        }));

        static VEGA: &str = include_str!("./vega.json");
        let mut output: serde_json::Value = serde_json::from_str(VEGA).unwrap();

        let root = output.as_object_mut().unwrap();

        root.insert("title".to_string(), names.join(" vs ").into());

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

        serde_json::to_writer_pretty(std::io::stdout(), &output)?;

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

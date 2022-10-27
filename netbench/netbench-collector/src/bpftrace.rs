// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{procinfo::Proc, Result};
use netbench::{
    stats::{Bucket, Histogram, Initialize, Print, Stat, Stats, StreamId},
    units::ByteExt as _,
};
use serde_json::json;
use std::{
    collections::HashMap,
    io,
    io::BufRead,
    process::{Command, Stdio},
    time::Duration,
};

static PROGRAM: &str = include_str!("./netbench.bt");

#[derive(Debug)]
struct Report {
    count: u64,
    interval: Duration,
    prev: Stats,
    current: Stats,
    send: HashMap<StreamId, Stat>,
    receive: HashMap<StreamId, Stat>,
    profiles: HashMap<u64, Histogram>,
    pending_profile: Option<u64>,
    proc: Option<Proc>,
}

impl Report {
    fn new(interval: Duration) -> Self {
        Self {
            interval,
            count: 0,
            prev: Default::default(),
            current: Default::default(),
            send: Default::default(),
            receive: Default::default(),
            profiles: Default::default(),
            pending_profile: None,
            proc: None,
        }
    }

    fn push(&mut self, line: &str) {
        let line = line.trim();

        if let Some(id) = self.pending_profile {
            if let Some(bucket) = parse_hist_line(line) {
                self.profiles.entry(id).or_default().buckets.push(bucket);
                return;
            } else {
                self.pending_profile = None;
            }
        }

        if line.is_empty() {
            return;
        }

        if line == "===" {
            self.dump();
            return;
        }

        if let Some(count) = line.strip_prefix("@: ") {
            self.count = count.parse().unwrap();
            return;
        }

        if let Some(trace) = line.strip_prefix("@P[") {
            let trace = trace.trim_end_matches("]:");
            if let Ok(trace) = trace.parse() {
                self.pending_profile = Some(trace);
                return;
            }
        }

        if let Some(line) = line.strip_prefix("@p[") {
            let (id, line) = line.split_once("]: ").unwrap();
            let id = id.parse().unwrap();

            let stat = BpfParse::parse(line).unwrap();
            self.profiles.entry(id).or_default().stat = stat;
            return;
        }

        macro_rules! stat {
            ($prefix:literal, $name:ident) => {
                if let Some(value) = line.strip_prefix(concat!("@", $prefix, ": ")) {
                    self.current.$name = BpfParse::parse(value).unwrap();
                    return;
                }
            };
        }

        stat!("c", cycles);
        stat!("i", instructions);
        stat!("b", branches);
        stat!("C", context_switches);
        stat!("a", allocs);
        stat!("R", reallocs);
        stat!("d", deallocs);
        stat!("S", syscalls);
        stat!("O", connections);
        stat!("A", accept);
        stat!("h", connect_time);

        macro_rules! try_map {
            ($prefix:literal, $on_value:expr) => {
                if let Some(line) = line.strip_prefix(concat!("@", $prefix, "[")) {
                    let mut on_value = $on_value;
                    let (id, line) = line.split_once("]: ").unwrap();
                    let (conn, id) = id.split_once(", ").unwrap();
                    let connection_id = conn.parse().unwrap();
                    let id = id.parse().unwrap();
                    let id = StreamId { connection_id, id };
                    let value = BpfParse::parse(line).unwrap();
                    on_value(id, value);
                    return;
                }
            };
        }

        macro_rules! stream_stat {
            ($prefix:literal, $name:ident) => {
                try_map!($prefix, |id, value| self.$name.insert(id, value));
            };
        }

        stream_stat!("s", send);
        stream_stat!("r", receive);

        if let Some(pid) = line.strip_prefix("cpid=") {
            let pid = pid.parse().unwrap();
            self.proc = Some(Proc::new(pid));

            // dump the initial numbers
            self.dump();
            return;
        }

        eprintln!("> {}", line);
    }

    fn dump(&mut self) {
        if let Some(proc) = self.proc.as_mut() {
            proc.load(&mut self.current);
        }

        self.entry().print().unwrap();

        // reset the values
        core::mem::swap(&mut self.prev, &mut self.current);
        self.current.allocs = Default::default();
        self.current.reallocs = Default::default();
        self.current.deallocs = Default::default();
    }

    fn entry(&mut self) -> Stats {
        let current = &self.current;
        let prev = &self.prev;
        let cycles = current.cycles.saturating_sub(prev.cycles);
        let instructions = current.instructions.saturating_sub(prev.instructions);
        let branches = current.branches.saturating_sub(prev.branches);
        let context_switches = current
            .context_switches
            .saturating_sub(prev.context_switches);
        let syscalls = current.syscalls.saturating_sub(prev.syscalls);
        let connections = current.connections.saturating_sub(prev.connections);
        let accept = current.accept.saturating_sub(prev.accept);

        let time = self.interval.as_millis() as u64 * self.count;
        let time = Duration::from_millis(time);

        Stats {
            time,
            cpu: current.cpu,
            cycles,
            instructions,
            branches,
            context_switches,
            syscalls,
            connections,
            accept,
            memory: current.memory,
            virtual_memory: current.virtual_memory,
            allocs: current.allocs,
            reallocs: current.reallocs,
            deallocs: current.deallocs,
            connect_time: current.connect_time,
            send: core::mem::take(&mut self.send),
            receive: core::mem::take(&mut self.receive),
            profiles: core::mem::take(&mut self.profiles),
        }
    }
}

trait BpfParse: Sized {
    fn parse(s: &str) -> Result<Self>;
}

impl BpfParse for u64 {
    fn parse(s: &str) -> Result<Self> {
        Ok(s.parse()?)
    }
}

impl BpfParse for Stat {
    fn parse(s: &str) -> Result<Self> {
        let mut parts = s.split(", ");

        macro_rules! part {
            ($name:ident) => {{
                let err = concat!("missing ", stringify!($name));
                let value = parts.next().ok_or(err)?;
                let value = value
                    .strip_prefix(concat!(stringify!($name), " "))
                    .ok_or(err)?;
                let value = value.parse().map_err(|_| err)?;
                value
            }};
        }

        let count = part!(count);
        // skip average since it can be computed with the other values
        let _average = parts.next();
        let total = part!(total);

        Ok(Self { count, total })
    }
}

fn parse_hist_line(line: &str) -> Option<Bucket> {
    let mut parts = line.split_whitespace();

    let lower = parts.next()?;
    let upper = parts.next()?;
    let count = parts.next()?;

    let lower = lower.trim_start_matches('[');
    let lower = lower.trim_end_matches(',');
    let lower = parse_hist_bound(lower)?;

    let upper = upper.trim_end_matches(')');
    let upper = parse_hist_bound(upper)?;

    let count = count.parse().ok()?;

    Some(Bucket {
        lower,
        upper,
        count,
    })
}

fn parse_hist_bound(s: &str) -> Option<u64> {
    // try parsing without any suffix
    if let Ok(v) = s.parse() {
        return Some(v);
    }

    let number_index = s
        .char_indices()
        .find_map(|(idx, c)| {
            if !(c.is_numeric() || c == '.') {
                Some(idx)
            } else {
                None
            }
        })
        .unwrap_or(s.len());

    let mut v: f64 = s[..number_index].parse().ok()?;

    let suffix = s[number_index..].trim();

    v *= *match suffix {
        "" => 1.bytes(),
        "K" | "k" => 1.kilobytes(),
        "Ki" | "ki" => 1.kibibytes(),
        "M" | "m" => 1.megabytes(),
        "Mi" | "mi" => 1.mebibytes(),
        "G" | "g" => 1.gigabytes(),
        "Gi" | "gi" => 1.gibibytes(),
        "T" | "t" => 1.terabytes(),
        "Ti" | "ti" => 1.tebibytes(),
        _ => return None,
    } as f64;

    Some(v as u64)
}

pub fn try_run(args: &crate::Args) -> Result<Option<()>> {
    let mut command = if let Ok(bpftrace) = find_bpftrace() {
        eprintln!("collecting stats with bpftrace");
        Command::new(bpftrace)
    } else {
        return Ok(None);
    };

    let driver = &args.driver;
    let interval = args.interval;
    let scenario = args.scenario()?;
    let scenario_path = &args.scenario;

    let program = {
        let template = handlebars::Handlebars::new();
        template.render_template(
            PROGRAM,
            &json!({
                "bin": &driver,
                "interval_ms": interval.as_millis() as u64,
                "libc": libc_location(driver)?.unwrap_or_else(|| driver.to_string()),
                "hardware": detect_hardware_events()?
            }),
        )?
    };

    command
        .arg("-c")
        .arg(driver)
        .arg("-e")
        .arg(program)
        .env("TRACE", "disabled")
        .env("SCENARIO", scenario_path)
        .stdout(Stdio::piped());

    let mut proc = command.spawn()?;

    Initialize {
        pid: proc.id() as _,
        driver: driver.to_string(),
        scenario: scenario_path.to_string(),
        traces: scenario.traces.to_vec(),
        ..Default::default()
    }
    .print()?;

    let output = proc.stdout.take().unwrap();
    let handle = std::thread::spawn(move || {
        let output = io::BufReader::new(output);
        let mut report = Report::new(interval);
        for line in output.lines() {
            if let Ok(line) = line {
                report.push(&line);
            } else {
                break;
            }
        }
        report.dump();
    });

    proc.wait()?;

    let _ = handle.join();

    Ok(Some(()))
}

fn find_bpftrace() -> Result<String> {
    let out = Command::new("which").arg("bpftrace").output()?;
    if out.status.success() {
        let out = core::str::from_utf8(&out.stdout)?;
        let out = out.trim();
        Ok(out.to_string())
    } else {
        Err("missing bpftrace".into())
    }
}

// finds the libc location for the given program
fn libc_location(cmd: &str) -> Result<Option<String>> {
    let out = Command::new("ldd").arg(cmd).output()?;
    let out = core::str::from_utf8(&out.stdout)?;
    for line in out.lines() {
        let line = line.trim();
        if line.starts_with("libc") {
            let (_, path) = line.split_once("=>").unwrap();
            let (path, _) = path.split_once('(').unwrap();
            let path = path.trim();
            return Ok(Some(path.to_string()));
        }
    }

    Ok(None)
}

fn detect_hardware_events() -> Result<bool> {
    let out = Command::new("perf").arg("list").arg("hw").output()?;
    let out = core::str::from_utf8(&out.stdout)?;

    if out.contains("cycles") && out.contains("branches") && out.contains("instructions") {
        return Ok(true);
    }

    Ok(false)
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{procinfo::Proc, Result};
use netbench::stats::{Initialize, Print, Stat, Stats, StreamId};
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
            proc: None,
        }
    }

    fn push(&mut self, line: &str) {
        let line = line.trim();

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
            memory: current.memory,
            virtual_memory: current.virtual_memory,
            allocs: current.allocs,
            reallocs: current.reallocs,
            deallocs: current.deallocs,
            send: core::mem::take(&mut self.send),
            receive: core::mem::take(&mut self.receive),
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

pub fn try_run(args: &crate::Args) -> Result<Option<()>> {
    let mut command = if let Ok(bpftrace) = find_bpftrace() {
        eprintln!("collecting stats with bpftrace");
        Command::new(bpftrace)
    } else {
        return Ok(None);
    };

    let driver = &args.driver;
    let interval = args.interval;
    let scenario = &args.scenario;

    let mut program = PROGRAM
        .replace("__BIN__", driver)
        .replace("__INTERVAL_MS__", &interval.as_millis().to_string());

    if let Some(libc) = libc_location(driver)? {
        program = program.replace("__LIBC__", &libc);
    } else {
        program = program.replace("__LIBC__", driver);
    }

    command
        .arg("-c")
        .arg(driver)
        .arg("-e")
        .arg(program)
        .env("TRACE", "disabled")
        .env("SCENARIO", scenario)
        .stdout(Stdio::piped());

    let mut proc = command.spawn()?;

    Initialize {
        pid: proc.id() as _,
        driver: driver.to_string(),
        scenario: scenario.to_string(),
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

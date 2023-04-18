// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::RunHandle;
use crate::{procinfo::Proc, Args, Result};
use netbench::stats::{Initialize, Print, Stats};
use std::{
    process::{Child, Command},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread::JoinHandle,
    time::Duration,
};

pub struct GenericHandle {
    is_open: Arc<AtomicBool>,
    proc: Child,
    handle: JoinHandle<()>,
}

impl RunHandle for GenericHandle {
    fn wait(mut self) -> Result<()> {
        self.proc.wait()?;
        self.is_open.store(false, Ordering::Relaxed);
        let _ = self.handle.join();
        Ok(())
    }

    fn kill(mut self) -> Result<()> {
        self.proc.kill()?;
        self.is_open.store(false, Ordering::Relaxed);
        let _ = self.handle.join();
        Ok(())
    }
}

pub fn run(args: &Args) -> Result<GenericHandle> {
    let mut command = Command::new(&args.driver);

    let driver = &args.driver;
    let interval = args.interval;
    let scenario_path = &args.scenario;
    let scenario = args.scenario()?;

    command
        .env("TRACE", "disabled")
        .env("SCENARIO", scenario_path);

    let proc = command.spawn()?;

    let info = Proc::new(proc.id());

    Initialize {
        pid: proc.id() as _,
        driver: driver.to_string(),
        scenario: scenario_path.to_string(),
        traces: scenario.traces.to_vec(),
        ..Default::default()
    }
    .print()?;

    let is_open = Arc::new(AtomicBool::new(true));
    let is_open_handle = is_open.clone();

    let handle = std::thread::spawn(move || {
        collect(info, interval, is_open_handle);
    });

    Ok(GenericHandle {
        proc,
        handle,
        is_open,
    })
}

fn collect(mut proc: Proc, interval: Duration, is_open: Arc<AtomicBool>) {
    let mut stats = Stats::default();

    loop {
        proc.load(&mut stats);
        stats.print().unwrap();

        if !is_open.load(Ordering::Relaxed) {
            return;
        }

        std::thread::sleep(interval);
        stats.time += interval;
    }
}

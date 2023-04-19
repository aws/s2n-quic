// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::stats::Stats;
use sysinfo::{CpuRefreshKind, Pid, ProcessExt, RefreshKind, System, SystemExt};

#[derive(Debug)]
pub struct Proc {
    pid: Pid,
    system: System,
}

impl Proc {
    pub fn new(pid: u32) -> Self {
        Self {
            pid: Pid::from(pid as i32),
            system: System::new_with_specifics(
                RefreshKind::new()
                    .with_cpu(CpuRefreshKind::new().with_cpu_usage())
                    .with_memory(),
            ),
        }
    }

    pub fn load(&mut self, stats: &mut Stats) {
        self.system.refresh_process(self.pid);
        if let Some(proc) = self.system.process(self.pid) {
            stats.cpu = proc.cpu_usage();
            // memory is returned in KB
            stats.memory = proc.memory() * 1000;
            stats.virtual_memory = proc.virtual_memory() * 1000;
        }
    }
}

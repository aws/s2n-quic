// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#[cfg(target_os = "linux")]
pub fn spawn() {
    tokio::spawn(async {
        use std::time::Duration;
        use tokio::time;
        use tracing::info;

        let mut interval = time::interval(Duration::from_secs(1));
        let mut prev = Counters::read();

        loop {
            interval.tick().await;
            let current = Counters::read();
            let delta = current.delta(&prev);

            if delta.any_nonzero() {
                info!(
                    udp.rx_errors = delta.in_errors,
                    udp.rcvbuf_errors = delta.rcvbuf_errors,
                    udp.sndbuf_errors = delta.sndbuf_errors,
                    udp.csum_errors = delta.in_csum_errors,
                    udp.drops = delta.drops,
                    "kernel UDP drops"
                );
            }

            prev = current;
        }
    });
}

#[cfg(not(target_os = "linux"))]
pub fn spawn() {}

#[cfg(target_os = "linux")]
#[derive(Default, Clone)]
struct Counters {
    in_errors: u64,
    rcvbuf_errors: u64,
    sndbuf_errors: u64,
    in_csum_errors: u64,
    drops: u64,
}

#[cfg(target_os = "linux")]
impl Counters {
    fn read() -> Self {
        let mut c = Self::default();
        c.read_snmp();
        c.read_udp_drops("/proc/net/udp");
        c.read_udp_drops("/proc/net/udp6");
        c
    }

    fn read_snmp(&mut self) {
        let Ok(content) = std::fs::read_to_string("/proc/net/snmp") else {
            return;
        };

        // /proc/net/snmp has pairs of lines: header then values
        // Find the Udp header line, then parse the values line
        let mut lines = content.lines();
        while let Some(header) = lines.next() {
            if !header.starts_with("Udp:") {
                let _ = lines.next(); // skip value line
                continue;
            }
            let Some(values) = lines.next() else { break };

            let headers: Vec<&str> = header.split_whitespace().collect();
            let vals: Vec<&str> = values.split_whitespace().collect();

            for (i, &name) in headers.iter().enumerate() {
                let val: u64 = vals.get(i).and_then(|v| v.parse().ok()).unwrap_or(0);
                match name {
                    "InErrors" => self.in_errors = val,
                    "RcvbufErrors" => self.rcvbuf_errors = val,
                    "SndbufErrors" => self.sndbuf_errors = val,
                    "InCsumErrors" => self.in_csum_errors = val,
                    _ => {}
                }
            }
            break;
        }
    }

    fn read_udp_drops(&mut self, path: &str) {
        let Ok(content) = std::fs::read_to_string(path) else {
            return;
        };

        // Skip header line, sum column 12 (0-indexed) which is "drops"
        for line in content.lines().skip(1) {
            let Some(drops_str) = line.split_whitespace().nth(12) else {
                continue;
            };
            self.drops += drops_str.parse::<u64>().unwrap_or(0);
        }
    }

    fn delta(&self, prev: &Self) -> Self {
        Self {
            in_errors: self.in_errors.saturating_sub(prev.in_errors),
            rcvbuf_errors: self.rcvbuf_errors.saturating_sub(prev.rcvbuf_errors),
            sndbuf_errors: self.sndbuf_errors.saturating_sub(prev.sndbuf_errors),
            in_csum_errors: self.in_csum_errors.saturating_sub(prev.in_csum_errors),
            drops: self.drops.saturating_sub(prev.drops),
        }
    }

    fn any_nonzero(&self) -> bool {
        self.in_errors != 0
            || self.rcvbuf_errors != 0
            || self.sndbuf_errors != 0
            || self.in_csum_errors != 0
            || self.drops != 0
    }
}

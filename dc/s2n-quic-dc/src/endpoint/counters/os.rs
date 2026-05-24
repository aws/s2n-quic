// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Linux OS counter ingestion for endpoint metrics.
//!
//! This module snapshots `/proc` network stats and records per-interval
//! deltas (for monotonic counters) and current values (for gauges) into the
//! shared `counter::Registry`. It is driven by the reporter background thread
//! via [`ReporterConfig::os_stats`][crate::counter::ReporterConfig]; the
//! [`Collector`] is created automatically when that flag is set.

#[cfg(target_os = "linux")]
use std::collections::{hash_map::Entry, HashMap};

/// A Linux OS metrics collector.
///
/// On each call to [`record_delta`][Collector::record_delta] it reads the
/// current `/proc` snapshots and:
///
/// - for **monotonic** sources (`/proc/net/snmp`, `/proc/net/netstat`,
///   `/proc/net/softnet_stat`, and `/proc/net/dev`): computes the per-interval
///   delta and increments the corresponding `Counter`.
/// - for **gauge** sources (`/proc/net/udp`, `/proc/net/udp6`,
///   `/proc/net/sockstat`, `/proc/net/sockstat6`):
///   sets the corresponding `Gauge` to the current absolute value.
#[cfg(target_os = "linux")]
pub struct Collector {
    counters: HashMap<String, crate::counter::Counter>,
    gauges: HashMap<String, crate::counter::Gauge>,
    registry: crate::counter::Registry,
    prev_monotonic: HashMap<String, u64>,
}

#[cfg(not(target_os = "linux"))]
#[derive(Default)]
pub struct Collector;

#[cfg(target_os = "linux")]
impl Collector {
    pub fn new(registry: crate::counter::Registry) -> Self {
        Self {
            counters: Default::default(),
            gauges: Default::default(),
            prev_monotonic: read_monotonic_snapshot(),
            registry,
        }
    }

    fn counter(&mut self, key: &str) -> &crate::counter::Counter {
        let registry = &self.registry;
        self.counters.entry(key.to_string()).or_insert_with(|| {
            registry
                .register(key)
                .with_description(counter_description(key))
        })
    }

    fn gauge(&mut self, key: &str) -> &crate::counter::Gauge {
        let registry = &self.registry;
        self.gauges.entry(key.to_string()).or_insert_with(|| {
            registry
                .register_gauge(key)
                .with_description(gauge_description(key))
        })
    }

    /// Reads fresh snapshots and updates all registered metrics.
    ///
    /// Monotonic sources are delta-encoded into `Counter`s. Gauge sources are
    /// set to their current absolute value.
    pub fn record_delta(&mut self) {
        // ── Monotonic counters ───────────────────────────────────────────────
        let current = read_monotonic_snapshot();
        for (key, value) in &current {
            match self.prev_monotonic.entry(key.clone()) {
                Entry::Occupied(mut entry) => {
                    let previous = entry.insert(*value);
                    let diff = value.saturating_sub(previous);
                    if diff > 0 {
                        self.counter(key).add(diff);
                    }
                }
                Entry::Vacant(entry) => {
                    // Prime newly discovered keys without emitting a since-boot spike.
                    entry.insert(*value);
                }
            }
        }
        // Drop baselines for keys that are no longer present in the latest snapshot.
        self.prev_monotonic
            .retain(|key, _| current.contains_key(key));

        // ── Gauges ───────────────────────────────────────────────────────────
        let gauge_values = read_gauge_snapshot();
        for (key, value) in &gauge_values {
            self.gauge(key).set(*value as i64);
        }
    }
}

#[cfg(not(target_os = "linux"))]
impl Collector {
    pub fn new(_registry: crate::counter::Registry) -> Self {
        Self
    }

    pub fn record_delta(&mut self) {}
}

// ── Snapshot functions ────────────────────────────────────────────────────────

/// Reads all monotonically-increasing OS counters.
#[cfg(target_os = "linux")]
fn read_monotonic_snapshot() -> HashMap<String, u64> {
    let mut values = HashMap::new();
    collect_snmp_pairs(&mut values, "/proc/net/snmp", "os.netstat");
    collect_snmp_pairs(&mut values, "/proc/net/netstat", "os.netstat_ext");
    collect_softnet_stat(&mut values, "/proc/net/softnet_stat");
    collect_netdev(&mut values, "/proc/net/dev");
    values
}

/// Reads all gauge-like OS statistics (current values, not cumulative).
#[cfg(target_os = "linux")]
fn read_gauge_snapshot() -> HashMap<String, u64> {
    let mut values = HashMap::new();
    collect_udp_columns(&mut values, "/proc/net/udp", "os.udp");
    collect_udp_columns(&mut values, "/proc/net/udp6", "os.udp6");
    collect_sockstat(&mut values, "/proc/net/sockstat", "os.sockstat");
    collect_sockstat(&mut values, "/proc/net/sockstat6", "os.sockstat6");
    values
}

// ── Description helpers ───────────────────────────────────────────────────────

#[cfg(target_os = "linux")]
fn counter_description(key: &str) -> String {
    if key.starts_with("os.netstat_ext.") {
        format!(
            "Per-interval delta of /proc/net/netstat:{}",
            key.trim_start_matches("os.netstat_ext.")
        )
    } else if key.starts_with("os.netstat.") {
        format!(
            "Per-interval delta of /proc/net/snmp:{}",
            key.trim_start_matches("os.netstat.")
        )
    } else if key.starts_with("os.qdisc.softnet.") {
        format!(
            "Per-interval delta of /proc/net/softnet_stat:{}",
            key.trim_start_matches("os.qdisc.softnet.")
        )
    } else if key.starts_with("os.ethtool.netdev.") {
        format!(
            "Per-interval delta of /proc/net/dev:{}",
            key.trim_start_matches("os.ethtool.netdev.")
        )
    } else {
        String::new()
    }
}

#[cfg(target_os = "linux")]
fn gauge_description(key: &str) -> String {
    if key.starts_with("os.udp6.") {
        format!(
            "Current sum across sockets of /proc/net/udp6:{}",
            key.trim_start_matches("os.udp6.")
        )
    } else if key.starts_with("os.udp.") {
        format!(
            "Current sum across sockets of /proc/net/udp:{}",
            key.trim_start_matches("os.udp.")
        )
    } else if key.starts_with("os.sockstat6.") {
        format!(
            "Current value of /proc/net/sockstat6:{}",
            key.trim_start_matches("os.sockstat6.")
        )
    } else if key.starts_with("os.sockstat.") {
        format!(
            "Current value of /proc/net/sockstat:{}",
            key.trim_start_matches("os.sockstat.")
        )
    } else {
        String::new()
    }
}

// ── Parsers ───────────────────────────────────────────────────────────────────

/// Parses SNMP-style paired header/value lines from `/proc/net/snmp` and
/// `/proc/net/netstat`.
///
/// Each logical entry spans two consecutive lines that share the same section
/// prefix (e.g. `"Udp:"` / `"Udp:"`): the first lists field names and the
/// second lists the corresponding integer values.
#[cfg(target_os = "linux")]
fn collect_snmp_pairs(out: &mut HashMap<String, u64>, path: &str, prefix: &str) {
    let Ok(content) = std::fs::read_to_string(path) else {
        return;
    };
    parse_snmp_pairs_content(out, &content, prefix);
}

#[cfg(target_os = "linux")]
fn parse_snmp_pairs_content(out: &mut HashMap<String, u64>, content: &str, prefix: &str) {
    let mut lines = content.lines();
    while let Some(header_line) = lines.next() {
        let Some(value_line) = lines.next() else {
            break;
        };
        let headers: Vec<&str> = header_line.split_whitespace().collect();
        let values: Vec<&str> = value_line.split_whitespace().collect();
        if headers.is_empty() || values.is_empty() || headers[0] != values[0] {
            continue;
        }

        let section = sanitize_metric_token(headers[0].trim_end_matches(':'));
        for (idx, header) in headers.iter().enumerate().skip(1) {
            let Some(value) = parse_numeric(values.get(idx).copied()) else {
                continue;
            };
            out.insert(
                format!("{prefix}.{section}.{}", sanitize_metric_token(header)),
                value,
            );
        }
    }
}

/// Collects all parseable columns from `/proc/net/udp` or `/proc/net/udp6`.
///
/// The table is per-socket and may contain both counters and identifiers, so
/// values are exported as gauges (current summed values), not monotonic deltas.
#[cfg(target_os = "linux")]
fn collect_udp_columns(out: &mut HashMap<String, u64>, path: &str, prefix: &str) {
    let Ok(content) = std::fs::read_to_string(path) else {
        return;
    };
    parse_udp_columns_content(out, &content, prefix);
}

#[cfg(target_os = "linux")]
fn parse_udp_columns_content(out: &mut HashMap<String, u64>, content: &str, prefix: &str) {
    let mut lines = content.lines();
    let Some(header_line) = lines.next() else {
        return;
    };
    let headers: Vec<&str> = header_line.split_whitespace().collect();
    if headers.is_empty() {
        return;
    }

    let mut sums: HashMap<String, u64> = HashMap::new();
    for line in lines {
        let values: Vec<&str> = line.split_whitespace().collect();
        if values.is_empty() {
            continue;
        }

        let mut header_idx = 0usize;
        let mut value_idx = 0usize;
        while header_idx < headers.len() && value_idx < values.len() {
            let header = headers[header_idx];
            let value = values[value_idx];

            if header.eq_ignore_ascii_case("tx_queue")
                && headers
                    .get(header_idx + 1)
                    .is_some_and(|next| next.eq_ignore_ascii_case("rx_queue"))
            {
                if let Some((tx, rx)) = value.split_once(':') {
                    for (field, part) in [("tx_queue", tx), ("rx_queue", rx)] {
                        if let Some(parsed) = parse_numeric(Some(part)) {
                            let key = format!("{prefix}.{}", sanitize_metric_token(field));
                            let sum = sums.entry(key).or_default();
                            *sum = sum.saturating_add(parsed);
                        }
                    }
                }
                header_idx += 2;
                value_idx += 1;
                continue;
            }

            if header.eq_ignore_ascii_case("tr")
                && headers
                    .get(header_idx + 1)
                    .is_some_and(|next| next.eq_ignore_ascii_case("tm->when"))
            {
                if let Some((tr, tm_when)) = value.split_once(':') {
                    for (field, part) in [("tr", tr), ("tm->when", tm_when)] {
                        if let Some(parsed) = parse_numeric(Some(part)) {
                            let key = format!("{prefix}.{}", sanitize_metric_token(field));
                            let sum = sums.entry(key).or_default();
                            *sum = sum.saturating_add(parsed);
                        }
                    }
                }
                header_idx += 2;
                value_idx += 1;
                continue;
            }

            if let Some(parsed) = parse_numeric(Some(value)) {
                let key = format!("{prefix}.{}", sanitize_metric_token(header));
                let sum = sums.entry(key).or_default();
                *sum = sum.saturating_add(parsed);
            }
            header_idx += 1;
            value_idx += 1;
        }
    }

    out.extend(sums);
}

/// Parses `/proc/net/sockstat` and `/proc/net/sockstat6`.
///
/// Values are current socket allocation counts (gauges, not monotonic counters).
#[cfg(target_os = "linux")]
fn collect_sockstat(out: &mut HashMap<String, u64>, path: &str, prefix: &str) {
    let Ok(content) = std::fs::read_to_string(path) else {
        return;
    };
    parse_sockstat_content(out, &content, prefix);
}

#[cfg(target_os = "linux")]
fn parse_sockstat_content(out: &mut HashMap<String, u64>, content: &str, prefix: &str) {
    for line in content.lines() {
        let mut cols = line.split_whitespace();
        let Some(section) = cols.next() else {
            continue;
        };
        let section = sanitize_metric_token(section.trim_end_matches(':'));
        let kv: Vec<&str> = cols.collect();
        for pair in kv.chunks_exact(2) {
            if let Some(value) = parse_numeric(Some(pair[1])) {
                out.insert(
                    format!(
                        "{prefix}.{section}.{}",
                        sanitize_metric_token(pair[0].trim_end_matches(':'))
                    ),
                    value,
                );
            }
        }
    }
}

/// Aggregates per-CPU rows from `/proc/net/softnet_stat`.
///
/// Each row corresponds to one CPU and contains space-separated hexadecimal
/// counters. The values are summed across all CPUs.
#[cfg(target_os = "linux")]
fn collect_softnet_stat(out: &mut HashMap<String, u64>, path: &str) {
    let Ok(content) = std::fs::read_to_string(path) else {
        return;
    };
    parse_softnet_content(out, &content);
}

#[cfg(target_os = "linux")]
fn parse_softnet_content(out: &mut HashMap<String, u64>, content: &str) {
    const KNOWN_FIELDS: [&str; 6] = [
        "processed",
        "dropped",
        "time_squeeze",
        "cpu_collision",
        "received_rps",
        "flow_limit_count",
    ];
    let mut sums: Vec<u64> = Vec::new();
    for line in content.lines() {
        for (idx, value) in line.split_whitespace().enumerate() {
            let Ok(value) = u64::from_str_radix(value, 16) else {
                continue;
            };
            if sums.len() <= idx {
                sums.resize(idx + 1, 0);
            }
            sums[idx] = sums[idx].saturating_add(value);
        }
    }
    for (idx, sum) in sums.into_iter().enumerate() {
        let field = KNOWN_FIELDS
            .get(idx)
            .map(|value| value.to_string())
            .unwrap_or_else(|| format!("field_{idx}"));
        out.insert(format!("os.qdisc.softnet.{field}"), sum);
    }
}

/// Reads per-interface RX and TX statistics from `/proc/net/dev`.
#[cfg(target_os = "linux")]
fn collect_netdev(out: &mut HashMap<String, u64>, path: &str) {
    let Ok(content) = std::fs::read_to_string(path) else {
        return;
    };
    let Some((rx_fields, tx_fields)) = parse_netdev_headers(&content) else {
        return;
    };
    let rx_fields: Vec<&str> = rx_fields.iter().map(String::as_str).collect();
    let tx_fields: Vec<&str> = tx_fields.iter().map(String::as_str).collect();
    parse_netdev_content(out, &content, &rx_fields, &tx_fields);
}

#[cfg(target_os = "linux")]
fn parse_netdev_headers(content: &str) -> Option<(Vec<String>, Vec<String>)> {
    let mut lines = content.lines();
    let _ = lines.next()?;
    let header_line = lines.next()?;
    let mut parts = header_line.split('|');
    let _ = parts.next()?;
    let rx = parts.next()?;
    let tx = parts.next()?;
    let rx_fields = rx
        .split_whitespace()
        .map(|field| format!("rx_{}", sanitize_metric_token(field)))
        .collect();
    let tx_fields = tx
        .split_whitespace()
        .map(|field| format!("tx_{}", sanitize_metric_token(field)))
        .collect();
    Some((rx_fields, tx_fields))
}

#[cfg(target_os = "linux")]
fn parse_netdev_content(
    out: &mut HashMap<String, u64>,
    content: &str,
    rx_fields: &[&str],
    tx_fields: &[&str],
) {
    for line in content.lines().skip(2) {
        let Some((iface, rest)) = line.split_once(':') else {
            continue;
        };
        let iface = sanitize_metric_token(iface.trim());
        let values: Vec<&str> = rest.split_whitespace().collect();
        if values.is_empty() {
            continue;
        }
        for (field, value) in rx_fields.iter().zip(values.iter()) {
            if let Some(value) = parse_numeric(Some(value)) {
                out.insert(format!("os.ethtool.netdev.{field}.{iface}"), value);
            }
        }
        for (field, value) in tx_fields.iter().zip(values.iter().skip(rx_fields.len())) {
            if let Some(value) = parse_numeric(Some(value)) {
                out.insert(format!("os.ethtool.netdev.{field}.{iface}"), value);
            }
        }
    }
}

#[cfg(target_os = "linux")]
fn parse_numeric(value: Option<&str>) -> Option<u64> {
    let value = value?;
    value
        .parse::<u64>()
        .ok()
        .or_else(|| u64::from_str_radix(value, 16).ok())
}

/// Normalizes a raw token from `/proc` into a stable metric label segment.
///
/// Only ASCII alphanumeric characters are preserved (lowercased); all other
/// characters are replaced with `_`.
#[cfg(target_os = "linux")]
fn sanitize_metric_token(token: &str) -> String {
    token
        .chars()
        .map(|c| match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' => c.to_ascii_lowercase(),
            _ => '_',
        })
        .collect()
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    // ── SNMP / netstat ────────────────────────────────────────────────────────

    #[test]
    fn snmp_extracts_udp_fields() {
        let content = "\
Udp: InDatagrams NoPorts InErrors OutDatagrams RcvbufErrors SndbufErrors
Udp: 10 0 5 20 3 2
";
        let mut out = HashMap::new();
        parse_snmp_pairs_content(&mut out, content, "os.netstat");
        assert_eq!(out.get("os.netstat.udp.inerrors"), Some(&5));
        assert_eq!(out.get("os.netstat.udp.rcvbuferrors"), Some(&3));
        assert_eq!(out.get("os.netstat.udp.indatagrams"), Some(&10));
    }

    #[test]
    fn snmp_skips_mismatched_section_labels() {
        // Header section prefix doesn't match value line prefix → skip.
        let content = "\
Tcp: RtoAlgorithm RtoMin
Udp: 1 2
";
        let mut out = HashMap::new();
        parse_snmp_pairs_content(&mut out, content, "os.netstat");
        assert!(out.is_empty());
    }

    // ── UDP ───────────────────────────────────────────────────────────────────

    #[test]
    fn udp_collects_all_parseable_columns() {
        // Real /proc/net/udp format: header has merged tx/rx queue and tr/tm->when tokens in rows.
        let content = "\
  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode ref pointer drops
  0: 00000000:0035 00000000:0000 07 00000000:00000000 00:00000000 00000000   101        0 12345 2 0000000000000000 5
  1: 0F02000A:0035 00000000:0000 07 00000000:00000000 00:00000000 00000000   101        0 67890 2 0000000000000000 3
";
        let mut out = HashMap::new();
        parse_udp_columns_content(&mut out, content, "os.udp");
        assert_eq!(out.get("os.udp.uid"), Some(&202));
        assert_eq!(out.get("os.udp.inode"), Some(&80235));
        assert_eq!(out.get("os.udp.tx_queue"), Some(&0));
        assert_eq!(out.get("os.udp.rx_queue"), Some(&0));
        assert_eq!(out.get("os.udp.tm__when"), Some(&0));
        assert_eq!(out.get("os.udp.drops"), Some(&8));
    }

    #[test]
    fn udp_columns_handles_empty_content() {
        let mut out = HashMap::new();
        parse_udp_columns_content(&mut out, "", "os.udp");
        assert!(out.is_empty());
    }

    #[test]
    fn udp_columns_handles_short_header() {
        let content = "\
  sl  local_address rem_address   st
   0: 00000000:0035 00000000:0000 07
";
        let mut out = HashMap::new();
        parse_udp_columns_content(&mut out, content, "os.udp");
        assert_eq!(out.get("os.udp.st"), Some(&7));
    }

    // ── Sockstat ─────────────────────────────────────────────────────────────

    #[test]
    fn sockstat_parses_key_value_pairs() {
        let content = "\
sockets: used 128
TCP: inuse 21 orphan 0 tw 0 alloc 21 mem 3
UDP: inuse 12 mem 0
UDPLITE: inuse 0
RAW: inuse 0
FRAG: inuse 0 memory 0
";
        let mut out = HashMap::new();
        parse_sockstat_content(&mut out, content, "os.sockstat");
        assert_eq!(out.get("os.sockstat.sockets.used"), Some(&128));
        assert_eq!(out.get("os.sockstat.tcp.inuse"), Some(&21));
        assert_eq!(out.get("os.sockstat.udp.inuse"), Some(&12));
        assert_eq!(out.get("os.sockstat.tcp.mem"), Some(&3));
        assert_eq!(out.get("os.sockstat.frag.memory"), Some(&0));
    }

    #[test]
    fn sockstat_handles_empty_content() {
        let mut out = HashMap::new();
        parse_sockstat_content(&mut out, "", "os.sockstat");
        assert!(out.is_empty());
    }

    // ── Softnet ───────────────────────────────────────────────────────────────

    #[test]
    fn softnet_sums_hex_columns_across_cpus() {
        let mut out = HashMap::new();
        parse_softnet_content(
            &mut out,
            "00000001 00000002 00000003 00000004 00000005 00000006 00000007\n\
             00000001 00000001 00000001 00000001 00000001 00000001 00000001\n",
        );
        assert_eq!(out.get("os.qdisc.softnet.processed"), Some(&2));
        assert_eq!(out.get("os.qdisc.softnet.dropped"), Some(&3));
        assert_eq!(out.get("os.qdisc.softnet.time_squeeze"), Some(&4));
        assert_eq!(out.get("os.qdisc.softnet.field_6"), Some(&8));
    }

    // ── Netdev ────────────────────────────────────────────────────────────────

    #[test]
    fn netdev_extracts_rx_tx_per_interface() {
        // /proc/net/dev format: 2 header lines, then "iface: <16 values>"
        let content = "\
Inter-|   Receive                                                |  Transmit
 face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets errs drop fifo colls carrier compressed
    lo:  123456   1000    0    0    0     0          0         0   654321    2000    0    0    0     0       0          0
   eth0: 999999   9999    1    2    3     4          5         6   111111    1111    7    8    9    10      11         12
";
        let mut out = HashMap::new();
        let (rx_fields, tx_fields) = parse_netdev_headers(content).unwrap();
        let rx_fields: Vec<&str> = rx_fields.iter().map(String::as_str).collect();
        let tx_fields: Vec<&str> = tx_fields.iter().map(String::as_str).collect();
        parse_netdev_content(&mut out, content, &rx_fields, &tx_fields);
        assert_eq!(out.get("os.ethtool.netdev.rx_bytes.lo"), Some(&123456));
        assert_eq!(out.get("os.ethtool.netdev.tx_bytes.lo"), Some(&654321));
        assert_eq!(out.get("os.ethtool.netdev.rx_bytes.eth0"), Some(&999999));
        assert_eq!(out.get("os.ethtool.netdev.rx_errs.eth0"), Some(&1));
        assert_eq!(out.get("os.ethtool.netdev.tx_drop.eth0"), Some(&8));
    }

    #[test]
    fn netdev_handles_rows_with_partial_columns() {
        let content = "\
Inter-|   Receive        |  Transmit
 face |bytes packets errs|bytes packets errs
    lo: 1 2 3
";
        let mut out = HashMap::new();
        let (rx_fields, tx_fields) = parse_netdev_headers(content).unwrap();
        let rx_fields: Vec<&str> = rx_fields.iter().map(String::as_str).collect();
        let tx_fields: Vec<&str> = tx_fields.iter().map(String::as_str).collect();
        parse_netdev_content(&mut out, content, &rx_fields, &tx_fields);
        assert_eq!(out.get("os.ethtool.netdev.rx_bytes.lo"), Some(&1));
        assert_eq!(out.get("os.ethtool.netdev.rx_packets.lo"), Some(&2));
        assert_eq!(out.get("os.ethtool.netdev.rx_errs.lo"), Some(&3));
        assert!(!out.contains_key("os.ethtool.netdev.tx_bytes.lo"));
    }

    // ── sanitize_metric_token ─────────────────────────────────────────────────

    #[test]
    fn sanitize_lowercases_and_replaces_special_chars() {
        assert_eq!(sanitize_metric_token("InErrors"), "inerrors");
        assert_eq!(sanitize_metric_token("rx-bytes"), "rx_bytes");
        assert_eq!(sanitize_metric_token("TCP:"), "tcp_");
        assert_eq!(sanitize_metric_token("tm->when"), "tm__when");
        assert_eq!(sanitize_metric_token("abc123"), "abc123");
    }
}

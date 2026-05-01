// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Analyzes diagnostic trace files produced by the diagnostic subscriber.
//!
//! Reads JSON trace files from a directory, groups them by the terminal error
//! event, and presents a summary table showing which error kinds dominate.
//! Also correlates client and server traces by credential_id+key_id to show
//! whether errors are one-sided or bilateral.

use anyhow::{Context, Result};
use clap::Args;
use serde::Deserialize;
use std::{collections::HashMap, path::PathBuf};
use xshell::{Shell, cmd};

#[derive(Args)]
pub struct Analyze {
    /// Directory containing diagnostic trace JSON files
    #[arg(default_value = "/tmp/dc-traces")]
    trace_dir: PathBuf,

    /// Show full traces for this error kind (substring match on the last error event)
    #[arg(long)]
    show: Option<String>,

    /// Maximum number of example traces to display per error kind
    #[arg(long, default_value = "3")]
    examples: usize,

    /// Show correlated client+server timelines for matched trace pairs.
    /// Optionally filter by error kind (substring match). Use "all" to show all pairs.
    #[arg(long)]
    pair: Option<String>,

    /// Maximum number of correlated pairs to display
    #[arg(long, default_value = "3")]
    max_pairs: usize,

    /// Path to node config file (same as xtask local --config).
    /// When provided, traces are collected from remote nodes via rsync before analysis.
    #[arg(long, short)]
    config: Option<PathBuf>,

    /// Local directory to collect remote traces into
    #[arg(long, default_value = "/tmp/dc-traces-collected")]
    collect_dir: PathBuf,
}

#[derive(Debug, Deserialize)]
struct Trace {
    connection_id: u64,
    credential_id: String,
    key_id: u64,
    remote_address: String,
    role: String,
    event_count: usize,
    events: Vec<EventRecord>,
}

#[derive(Debug, Deserialize)]
struct EventRecord {
    seq: u32,
    event: String,
    detail: String,
}

/// Tracks per-error-kind statistics including client/server breakdown.
struct ErrorGroup {
    count: usize,
    client_count: usize,
    server_count: usize,
    examples: Vec<TraceExample>,
}

struct TraceExample {
    filename: String,
    connection_id: u64,
    credential_id: String,
    key_id: u64,
    role: String,
    remote_address: String,
    event_count: usize,
    terminal_event: String,
    terminal_detail: String,
}

/// A parsed trace with its classification.
struct ClassifiedTrace {
    credential_id: String,
    key_id: u64,
    role: String,
    error_kind: String,
}

/// Config file format (reused from local.rs)
#[derive(Debug, Deserialize)]
struct NodeConfig {
    #[serde(default)]
    host: Vec<HostEntry>,
    #[serde(default = "default_remote_dir")]
    remote_dir: PathBuf,
}

fn default_remote_dir() -> PathBuf {
    PathBuf::from("~/s2n-quic")
}

#[derive(Debug, Deserialize)]
struct HostEntry {
    hostname: String,
    #[serde(default)]
    user: Option<String>,
}

impl Analyze {
    pub fn run(self, sh: &Shell) -> Result<()> {
        // If a config file is provided, collect traces from remote nodes first
        let analysis_dir = if let Some(ref config_path) = self.config {
            self.collect_remote_traces(sh, config_path)?
        } else {
            self.trace_dir.clone()
        };

        let entries: Vec<_> = std::fs::read_dir(&analysis_dir)
            .with_context(|| format!("Failed to read trace directory: {}", analysis_dir.display()))?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "json"))
            .collect();

        if entries.is_empty() {
            println!("No trace files found in {}", analysis_dir.display());
            return Ok(());
        }

        println!(
            "Analyzing {} trace files from {}\n",
            entries.len(),
            analysis_dir.display()
        );

        let mut groups: HashMap<String, ErrorGroup> = HashMap::new();
        let mut classified: Vec<ClassifiedTrace> = Vec::new();
        let mut parse_errors = 0u64;

        for entry in &entries {
            let path = entry.path();
            let filename = path
                .file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_default();

            let content = match std::fs::read_to_string(&path) {
                Ok(c) => c,
                Err(_) => {
                    parse_errors += 1;
                    continue;
                }
            };

            let trace: Trace = match serde_json::from_str(&content) {
                Ok(t) => t,
                Err(_) => {
                    parse_errors += 1;
                    continue;
                }
            };

            // Find the terminal error: last event that contains "Errored"
            let terminal = trace
                .events
                .iter()
                .rev()
                .find(|e| e.event.contains("Errored"));

            let error_kind = if let Some(evt) = terminal {
                classify_error(&evt.event, &evt.detail)
            } else {
                let last = trace.events.last();
                last.map(|e| classify_error(&e.event, &e.detail))
                    .unwrap_or_else(|| "unknown (empty trace)".to_string())
            };

            let is_client = trace.role == "client";
            let is_server = trace.role == "server";

            // Save for correlation
            classified.push(ClassifiedTrace {
                credential_id: trace.credential_id.clone(),
                key_id: trace.key_id,
                role: trace.role.clone(),
                error_kind: error_kind.clone(),
            });

            let example = TraceExample {
                filename,
                connection_id: trace.connection_id,
                credential_id: trace.credential_id.clone(),
                key_id: trace.key_id,
                role: trace.role.clone(),
                remote_address: trace.remote_address.clone(),
                event_count: trace.event_count,
                terminal_event: terminal
                    .map(|e| e.event.clone())
                    .unwrap_or_else(|| "unknown".to_string()),
                terminal_detail: terminal.map(|e| e.detail.clone()).unwrap_or_default(),
            };

            let group = groups.entry(error_kind.clone()).or_insert(ErrorGroup {
                count: 0,
                client_count: 0,
                server_count: 0,
                examples: Vec::new(),
            });
            group.count += 1;
            if is_client {
                group.client_count += 1;
            }
            if is_server {
                group.server_count += 1;
            }
            if group.examples.len() < self.examples {
                group.examples.push(example);
            }
        }

        // Sort by count descending
        let mut sorted: Vec<_> = groups.into_iter().collect();
        sorted.sort_by(|a, b| b.1.count.cmp(&a.1.count));

        let total: usize = sorted.iter().map(|(_, g)| g.count).sum();

        // Print summary table with client/server breakdown
        println!("═══════════════════════════════════════════════════════════════════════════════");
        println!("  Error Classification Summary ({total} total traces)");
        println!("═══════════════════════════════════════════════════════════════════════════════");
        println!(
            "{:>6}  {:>6}  {:>7}  {:>7}  Error Kind",
            "Count", "%", "Client", "Server"
        );
        println!("------  ------  -------  -------  ----------");

        for (kind, group) in &sorted {
            let pct = (group.count as f64 / total as f64) * 100.0;
            println!(
                "{:>6}  {:>5.1}%  {:>7}  {:>7}  {}",
                group.count, pct, group.client_count, group.server_count, kind
            );
        }
        println!();

        if parse_errors > 0 {
            println!("  ({parse_errors} files could not be parsed)\n");
        }

        // Correlation analysis: match client+server traces by credential_id+key_id
        self.print_correlation(&classified);

        // If --pair is specified, show correlated client+server timelines
        if let Some(ref filter) = self.pair {
            self.print_pairs(&analysis_dir, filter)?;
        }

        // If --show is specified, print matching traces
        if let Some(ref filter) = self.show {
            let filter_lower = filter.to_lowercase();
            for (kind, group) in &sorted {
                if !kind.to_lowercase().contains(&filter_lower) {
                    continue;
                }

                println!("───────────────────────────────────────────────────────────────────");
                println!("  Examples for: {} ({} occurrences)", kind, group.count);
                println!("───────────────────────────────────────────────────────────────────");

                for (i, ex) in group.examples.iter().enumerate() {
                    println!();
                    println!("  Example {}/{}", i + 1, group.examples.len());
                    println!("    File:          {}", ex.filename);
                    println!("    Connection:    {}", ex.connection_id);
                    println!("    Credential:    {}", ex.credential_id);
                    println!("    Key ID:        {}", ex.key_id);
                    println!("    Role:          {}", ex.role);
                    println!("    Remote:        {}", ex.remote_address);
                    println!("    Events:        {}", ex.event_count);
                    println!("    Terminal:      {}", ex.terminal_event);
                    println!("    Detail:        {}", ex.terminal_detail);
                }
                println!();
            }
        } else {
            println!("Tip: Use --show <keyword> to see example traces for a specific error kind.");
            println!("     e.g.: xtask analyze --show FlowReset");
        }

        Ok(())
    }

    /// Correlate client and server traces by credential_id+key_id.
    fn print_correlation(&self, traces: &[ClassifiedTrace]) {
        // Build a map: (credential_id, key_id) -> (client_errors, server_errors)
        let mut streams: HashMap<(&str, u64), (Vec<&str>, Vec<&str>)> = HashMap::new();

        for t in traces {
            let entry = streams
                .entry((&t.credential_id, t.key_id))
                .or_insert_with(|| (Vec::new(), Vec::new()));
            if t.role == "client" {
                entry.0.push(&t.error_kind);
            } else {
                entry.1.push(&t.error_kind);
            }
        }

        let mut client_only = 0u64;
        let mut server_only = 0u64;
        let mut both_sides = 0u64;

        for (_key, (client_errs, server_errs)) in &streams {
            match (!client_errs.is_empty(), !server_errs.is_empty()) {
                (true, true) => both_sides += 1,
                (true, false) => client_only += 1,
                (false, true) => server_only += 1,
                (false, false) => {}
            }
        }

        let total_streams = client_only + server_only + both_sides;
        if total_streams == 0 {
            return;
        }

        println!("═══════════════════════════════════════════════════════════════════════════════");
        println!(
            "  Stream Correlation ({} unique credential+key_id pairs with errors)",
            total_streams
        );
        println!("═══════════════════════════════════════════════════════════════════════════════");
        println!(
            "    Client-only errors:  {:>6}  ({:.1}%)",
            client_only,
            client_only as f64 / total_streams as f64 * 100.0
        );
        println!(
            "    Server-only errors:  {:>6}  ({:.1}%)",
            server_only,
            server_only as f64 / total_streams as f64 * 100.0
        );
        println!(
            "    Both sides errored:  {:>6}  ({:.1}%)",
            both_sides,
            both_sides as f64 / total_streams as f64 * 100.0
        );
        println!();
    }

    /// Load full traces, match client+server by credential_id+key_id, and display
    /// interleaved timelines for matched pairs.
    fn print_pairs(&self, analysis_dir: &PathBuf, filter: &str) -> Result<()> {
        let show_all = filter.eq_ignore_ascii_case("all");
        let filter_lower = filter.to_lowercase();

        // Load all traces into memory, keyed by (credential_id, key_id)
        type TraceKey = (String, u64);
        let mut client_traces: HashMap<TraceKey, (String, Trace)> = HashMap::new();
        let mut server_traces: HashMap<TraceKey, (String, Trace)> = HashMap::new();

        for entry in std::fs::read_dir(analysis_dir)
            .with_context(|| format!("Failed to read {}", analysis_dir.display()))?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "json"))
        {
            let path = entry.path();
            let filename = path
                .file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_default();

            let content = match std::fs::read_to_string(&path) {
                Ok(c) => c,
                Err(_) => continue,
            };
            let trace: Trace = match serde_json::from_str(&content) {
                Ok(t) => t,
                Err(_) => continue,
            };

            let key = (trace.credential_id.clone(), trace.key_id);
            if trace.role == "client" {
                client_traces.insert(key, (filename, trace));
            } else {
                server_traces.insert(key, (filename, trace));
            }
        }

        // Find matched pairs
        let mut pairs: Vec<_> = client_traces
            .keys()
            .filter(|k| server_traces.contains_key(*k))
            .cloned()
            .collect();
        pairs.sort_by_key(|(_, kid)| *kid);

        // Count terminal-event-pair patterns across all matched pairs
        let mut pair_patterns: HashMap<String, usize> = HashMap::new();
        for key in &pairs {
            let (_, ct) = &client_traces[key];
            let (_, st) = &server_traces[key];
            let c_terminal = ct
                .events
                .iter()
                .rev()
                .find(|e| e.event.contains("Errored"))
                .map(|e| classify_error(&e.event, &e.detail))
                .unwrap_or_else(|| "ok".to_string());
            let s_terminal = st
                .events
                .iter()
                .rev()
                .find(|e| e.event.contains("Errored"))
                .map(|e| classify_error(&e.event, &e.detail))
                .unwrap_or_else(|| "ok".to_string());
            let pattern = format!("client: {} ↔ server: {}", c_terminal, s_terminal);
            *pair_patterns.entry(pattern).or_insert(0) += 1;
        }

        println!("═══════════════════════════════════════════════════════════════════════════════");
        println!(
            "  Matched Pair Patterns ({} pairs with both client+server traces)",
            pairs.len()
        );
        println!("═══════════════════════════════════════════════════════════════════════════════");
        let mut sorted_patterns: Vec<_> = pair_patterns.iter().collect();
        sorted_patterns.sort_by(|a, b| b.1.cmp(a.1));
        for (pattern, count) in &sorted_patterns {
            let pct = **count as f64 / pairs.len().max(1) as f64 * 100.0;
            println!("  {:>6}  ({:>5.1}%)  {}", count, pct, pattern);
        }
        println!();

        // Filter and display individual pair timelines
        let filtered_pairs: Vec<_> = if show_all {
            pairs
        } else {
            pairs
                .into_iter()
                .filter(|key| {
                    let (_, ct) = &client_traces[key];
                    let (_, st) = &server_traces[key];
                    let c_match = ct.events.iter().any(|e| {
                        e.event.to_lowercase().contains(&filter_lower)
                            || e.detail.to_lowercase().contains(&filter_lower)
                    });
                    let s_match = st.events.iter().any(|e| {
                        e.event.to_lowercase().contains(&filter_lower)
                            || e.detail.to_lowercase().contains(&filter_lower)
                    });
                    c_match || s_match
                })
                .collect()
        };

        let display_count = filtered_pairs.len().min(self.max_pairs);
        for (i, key) in filtered_pairs.iter().take(self.max_pairs).enumerate() {
            let (cf, ct) = &client_traces[key];
            let (sf, st) = &server_traces[key];

            println!(
                "═══════════════════════════════════════════════════════════════════════════════"
            );
            println!(
                "  Pair {}/{} — credential: {} key_id: {}",
                i + 1,
                display_count,
                key.0,
                key.1
            );
            println!(
                "═══════════════════════════════════════════════════════════════════════════════"
            );
            println!(
                "  Client: {} (conn={}, {} events)",
                cf, ct.connection_id, ct.event_count
            );
            println!(
                "  Server: {} (conn={}, {} events, remote={})",
                sf, st.connection_id, st.event_count, st.remote_address
            );
            println!();

            // Print client events
            println!("  ┌─ Client Timeline ─────────────────────────────────────────────────────");
            for e in &ct.events {
                let marker = if e.event.contains("Errored") {
                    "✗"
                } else if e.event.contains("Transmitted") || e.event.contains("Flushed") {
                    "→"
                } else if e.event.contains("Received") {
                    "←"
                } else {
                    "·"
                };
                println!("  │ {:>3} {} {}", e.seq, marker, truncate(&e.detail, 100));
            }
            println!(
                "  └─────────────────────────────────────────────────────────────────────────"
            );
            println!();

            // Print server events
            println!("  ┌─ Server Timeline ─────────────────────────────────────────────────────");
            for e in &st.events {
                let marker = if e.event.contains("Errored") {
                    "✗"
                } else if e.event.contains("Transmitted") || e.event.contains("Flushed") {
                    "→"
                } else if e.event.contains("Received") {
                    "←"
                } else {
                    "·"
                };
                println!("  │ {:>3} {} {}", e.seq, marker, truncate(&e.detail, 100));
            }
            println!(
                "  └─────────────────────────────────────────────────────────────────────────"
            );
            println!();
        }

        if filtered_pairs.len() > self.max_pairs {
            println!(
                "  ... and {} more pairs (use --max-pairs to show more)",
                filtered_pairs.len() - self.max_pairs
            );
        }

        Ok(())
    }

    /// Collect trace files from remote nodes via rsync, then delete from remotes.
    fn collect_remote_traces(&self, sh: &Shell, config_path: &PathBuf) -> Result<PathBuf> {
        let content = std::fs::read_to_string(config_path)
            .with_context(|| format!("Failed to read config: {}", config_path.display()))?;
        let cfg: NodeConfig = toml::from_str(&content)
            .with_context(|| format!("Failed to parse config: {}", config_path.display()))?;

        let collect_dir = &self.collect_dir;
        std::fs::create_dir_all(collect_dir)
            .with_context(|| format!("Failed to create collect dir: {}", collect_dir.display()))?;

        let trace_dir = self.trace_dir.display().to_string();

        for host in &cfg.host {
            let ssh_target = if let Some(ref user) = host.user {
                format!("{}@{}", user, host.hostname)
            } else {
                host.hostname.clone()
            };

            let remote_path = format!("{}:{}/{}/", ssh_target, cfg.remote_dir.display(), trace_dir);
            let local_path = format!("{}/", collect_dir.display());

            eprintln!("Collecting traces from {} ...", ssh_target);

            // Use rsync --remove-source-files to pull and delete remote traces
            let _ = cmd!(
                sh,
                "rsync -avz --remove-source-files --include=*.json --exclude=* {remote_path} {local_path}"
            )
            .quiet()
            .run();
        }

        // Also copy local traces if the trace_dir exists locally
        if self.trace_dir.exists() && self.trace_dir != *collect_dir {
            let src = format!("{}/", self.trace_dir.display());
            let dst = format!("{}/", collect_dir.display());
            // Move local traces too (use --remove-source-files)
            let _ = cmd!(
                sh,
                "rsync -avz --remove-source-files --include=*.json --exclude=* {src} {dst}"
            )
            .quiet()
            .run();
        }

        eprintln!("Collected traces into {}\n", collect_dir.display());

        Ok(collect_dir.clone())
    }
}

/// Classifies an error from the event name and debug detail string.
fn classify_error(event_name: &str, detail: &str) -> String {
    // Try to find "kind: <Variant>" pattern — take the LAST occurrence
    let mut best_kind = None;
    let mut search_from = 0;
    while let Some(pos) = detail[search_from..].find("kind: ") {
        let abs_pos = search_from + pos;
        let rest = &detail[abs_pos + 6..];
        let end = rest
            .find(|c: char| c == ',' || c == '}' || c == ')' || c == ' ' || c == '(')
            .unwrap_or(rest.len());
        let kind = rest[..end].trim();
        if !kind.is_empty() {
            best_kind = Some(kind.to_string());
        }
        search_from = abs_pos + 6 + end;
    }

    if let Some(kind) = best_kind {
        return format!("{kind} ({event_name})");
    }

    // Try "ErrorKind::<Variant>" pattern
    if let Some(pos) = detail.find("ErrorKind::") {
        let rest = &detail[pos + 11..];
        let end = rest
            .find(|c: char| !c.is_alphanumeric())
            .unwrap_or(rest.len());
        let kind = &rest[..end];
        if !kind.is_empty() {
            return format!("{kind} ({event_name})");
        }
    }

    event_name.to_string()
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len])
    }
}

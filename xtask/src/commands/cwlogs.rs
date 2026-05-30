// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use anyhow::{Context, Result};
use arrow::{
    array::*,
    datatypes::{DataType, Field, Schema},
};
use clap::Args;
use parquet::{
    arrow::{ArrowWriter, arrow_reader::ParquetRecordBatchReaderBuilder},
    basic::Compression,
    file::properties::WriterProperties,
};
use serde::Deserialize;
use std::{
    path::PathBuf,
    process::Command,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use xshell::Shell;

use super::metrics_parquet::{MetricsBatchBuilder, metrics_schema};

#[derive(Args)]
pub struct Cwlogs {
    /// CloudWatch log group name
    #[arg(long, short = 'g')]
    log_group: Option<String>,

    /// Start time (RFC 3339, relative like "1h"/"30m"/"2d", epoch seconds, or "now")
    #[arg(long)]
    start: Option<String>,

    /// End time (RFC 3339, relative like "1h"/"30m"/"2d", epoch seconds, or "now")
    #[arg(long, default_value = "now")]
    end: String,

    /// Output directory for cached logs + parquet
    #[arg(long, short = 'o', default_value = "logs/cwlogs")]
    output_dir: PathBuf,

    /// Skip fetching, only re-parse existing cached events.parquet
    #[arg(long)]
    parse_only: bool,

    /// Filter pattern passed to aws CLI
    #[arg(long, default_value = "METRICS")]
    filter_pattern: String,

    /// AWS CLI profile name
    #[arg(long)]
    profile: Option<String>,
}

impl Cwlogs {
    pub fn run(self, _sh: &Shell) -> Result<()> {
        std::fs::create_dir_all(&self.output_dir).with_context(|| {
            format!("Failed to create output dir: {}", self.output_dir.display())
        })?;

        let events_path = self.output_dir.join("events.parquet");
        let parquet_path = self.output_dir.join("metrics.parquet");

        if !self.parse_only {
            let log_group = self
                .log_group
                .as_deref()
                .context("--log-group is required unless --parse-only")?;
            let start = self
                .start
                .as_deref()
                .context("--start is required unless --parse-only")?;

            let start_ms = parse_time(start)?;
            let end_ms = parse_time(&self.end)?;

            eprintln!(
                "Fetching logs from {} ({}ms → {}ms)...",
                log_group, start_ms, end_ms
            );

            fetch_logs(
                log_group,
                start_ms,
                end_ms,
                &self.filter_pattern,
                self.profile.as_deref(),
                &events_path,
            )?;
        }

        if !events_path.exists() {
            anyhow::bail!(
                "No cached events file found at {}. Run without --parse-only first.",
                events_path.display()
            );
        }

        let log_group = self.log_group.as_deref().unwrap_or("");
        eprintln!("Parsing metrics → {}", parquet_path.display());
        parse_to_parquet(&events_path, &parquet_path, log_group)?;

        eprintln!("Done: {}", parquet_path.display());
        Ok(())
    }
}

// ── Phase 1: Fetch from CloudWatch ──────────────────────────────────────────

fn fetch_logs(
    log_group: &str,
    start_ms: i64,
    end_ms: i64,
    filter_pattern: &str,
    profile: Option<&str>,
    output_path: &std::path::Path,
) -> Result<()> {
    let file = std::fs::File::create(output_path)
        .with_context(|| format!("Failed to create {}", output_path.display()))?;
    let props = WriterProperties::builder()
        .set_compression(Compression::ZSTD(Default::default()))
        .set_max_row_group_size(100_000)
        .build();
    let mut writer = ArrowWriter::try_new(file, events_schema(), Some(props))
        .context("Failed to create events parquet writer")?;
    let mut event_batch = EventBatchBuilder::new();

    let mut next_token: Option<String> = None;
    let mut total_events = 0u64;
    let mut page = 0u64;

    loop {
        page += 1;
        let mut cmd = Command::new("aws");
        cmd.args([
            "logs",
            "filter-log-events",
            "--log-group-name",
            log_group,
            "--start-time",
            &start_ms.to_string(),
            "--end-time",
            &end_ms.to_string(),
            "--filter-pattern",
            filter_pattern,
            "--output",
            "json",
        ]);

        if let Some(p) = profile {
            cmd.args(["--profile", p]);
        }

        if let Some(token) = &next_token {
            cmd.args(["--next-token", token]);
        }

        let output = cmd
            .output()
            .context("Failed to execute `aws` CLI. Is it installed and configured?")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("aws logs filter-log-events failed: {}", stderr.trim());
        }

        let response: FilterLogEventsResponse = serde_json::from_slice(&output.stdout)
            .context("Failed to parse aws CLI JSON response")?;

        let event_count = response.events.len();
        total_events += event_count as u64;

        for event in &response.events {
            event_batch.push(event);
            if event_batch.row_count >= BATCH_SIZE {
                let record_batch = event_batch.finish();
                writer.write(&record_batch)?;
                event_batch = EventBatchBuilder::new();
            }
        }

        eprint!("\r  page {page}: {total_events} events fetched");

        match response.next_token {
            Some(token) if !token.is_empty() => next_token = Some(token),
            _ => break,
        }
    }
    if event_batch.row_count > 0 {
        let record_batch = event_batch.finish();
        writer.write(&record_batch)?;
    }
    writer.close()?;

    eprintln!();
    eprintln!("  cached to {}", output_path.display());
    Ok(())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FilterLogEventsResponse {
    #[serde(default)]
    events: Vec<LogEvent>,
    next_token: Option<String>,
}

#[derive(Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct LogEvent {
    #[serde(default)]
    timestamp: i64,
    #[serde(default)]
    log_stream_name: String,
    #[serde(default)]
    message: String,
}

fn events_schema() -> Arc<Schema> {
    Arc::new(Schema::new(vec![
        Field::new("timestamp", DataType::Int64, false),
        Field::new("log_stream_name", DataType::Utf8, false),
        Field::new("message", DataType::Utf8, false),
    ]))
}

struct EventBatchBuilder {
    timestamp: Int64Builder,
    log_stream_name: StringBuilder,
    message: StringBuilder,
    row_count: usize,
}

impl EventBatchBuilder {
    fn new() -> Self {
        Self {
            timestamp: Int64Builder::new(),
            log_stream_name: StringBuilder::new(),
            message: StringBuilder::new(),
            row_count: 0,
        }
    }

    fn push(&mut self, event: &LogEvent) {
        self.timestamp.append_value(event.timestamp);
        self.log_stream_name.append_value(&event.log_stream_name);
        self.message.append_value(&event.message);
        self.row_count += 1;
    }

    fn finish(&mut self) -> RecordBatch {
        RecordBatch::try_new(
            events_schema(),
            vec![
                Arc::new(self.timestamp.finish()),
                Arc::new(self.log_stream_name.finish()),
                Arc::new(self.message.finish()),
            ],
        )
        .expect("schema mismatch in events batch builder")
    }
}

// ── Phase 2: Parse + write Parquet ──────────────────────────────────────────

const BATCH_SIZE: usize = 8192;

fn parse_to_parquet(
    events_path: &std::path::Path,
    parquet_path: &std::path::Path,
    log_group: &str,
) -> Result<()> {
    let schema = metrics_schema();
    let props = WriterProperties::builder()
        .set_compression(Compression::ZSTD(Default::default()))
        .set_max_row_group_size(100_000)
        .build();

    let file = std::fs::File::create(parquet_path)
        .with_context(|| format!("Failed to create {}", parquet_path.display()))?;
    let mut writer = ArrowWriter::try_new(file, schema, Some(props))
        .context("Failed to create parquet writer")?;

    let input = std::fs::File::open(events_path)
        .with_context(|| format!("Failed to open {}", events_path.display()))?;
    let mut reader = ParquetRecordBatchReaderBuilder::try_new(input)
        .context("Failed to open cached events parquet")?
        .with_batch_size(BATCH_SIZE)
        .build()
        .context("Failed to build cached events parquet reader")?;

    let mut batch = MetricsBatchBuilder::new();
    let mut total_rows = 0u64;
    let mut lines_processed = 0u64;

    for record_batch in &mut reader {
        let record_batch = record_batch?;
        let timestamps = record_batch
            .column(0)
            .as_any()
            .downcast_ref::<Int64Array>()
            .context("Invalid cached events parquet timestamp column")?;
        let streams = record_batch
            .column(1)
            .as_any()
            .downcast_ref::<StringArray>()
            .context("Invalid cached events parquet stream column")?;
        let messages = record_batch
            .column(2)
            .as_any()
            .downcast_ref::<StringArray>()
            .context("Invalid cached events parquet message column")?;

        for row_idx in 0..record_batch.num_rows() {
            lines_processed += 1;

            let ts = timestamps.value(row_idx) as f64 / 1000.0;
            let stream = streams.value(row_idx);
            let message = messages.value(row_idx);
            let normalized_log_group = log_group.trim();

            let (raw, env) = match extract_metrics_payload(message) {
                Some(v) => v,
                None => continue,
            };

            let parsed = s2n_quic_dc_metrics::format::ParsedMetricsLine::parse(raw);
            if parsed.is_empty() {
                continue;
            }

            for row in parsed.to_rows() {
                batch.push(
                    ts,
                    "cwlogs",
                    (!normalized_log_group.is_empty()).then_some(normalized_log_group),
                    Some(stream),
                    env,
                    &row,
                );
                total_rows += 1;

                if batch.row_count >= BATCH_SIZE {
                    let record_batch = batch.finish();
                    writer.write(&record_batch)?;
                    batch = MetricsBatchBuilder::new();
                }
            }

            if lines_processed % 10_000 == 0 {
                eprint!("\r  {lines_processed} lines → {total_rows} metric rows");
            }
        }
    }

    if batch.row_count > 0 {
        let record_batch = batch.finish();
        writer.write(&record_batch)?;
    }

    writer.close()?;
    eprintln!("\r  {lines_processed} lines → {total_rows} metric rows");

    Ok(())
}

fn extract_metrics_payload(message: &str) -> Option<(&str, Option<&str>)> {
    let pos = message.find("[METRICS")?;
    let after = &message[pos + 8..]; // skip "[METRICS"

    if let Some(rest) = after.strip_prefix(']') {
        let raw = rest.trim();
        if raw.is_empty() {
            return None;
        }
        return Some((raw, None));
    }

    if let Some(after_colon) = after.strip_prefix(':') {
        if let Some((prefix, rest)) = after_colon.split_once(']') {
            let raw = rest.trim();
            if raw.is_empty() {
                return None;
            }
            return Some((raw, Some(prefix)));
        }
    }

    None
}

// ── Time parsing ────────────────────────────────────────────────────────────

fn parse_time(s: &str) -> Result<i64> {
    if s == "now" {
        let ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        return Ok(ms);
    }

    // Relative duration: "1h", "30m", "2d", "90s"
    if let Some(dur) = parse_relative_duration(s) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        return Ok(now - dur.as_millis() as i64);
    }

    // Epoch seconds (bare integer)
    if let Ok(epoch_secs) = s.parse::<i64>() {
        return Ok(epoch_secs * 1000);
    }

    // RFC 3339: "2026-05-22T20:00:00Z" or "2026-05-22T20:00:00+00:00"
    if let Some(ms) = parse_rfc3339(s) {
        return Ok(ms);
    }

    anyhow::bail!(
        "Cannot parse time '{}'. Expected: RFC 3339, relative (1h/30m/2d), epoch seconds, or 'now'",
        s
    )
}

fn parse_relative_duration(s: &str) -> Option<Duration> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }

    let (num_str, unit) = s.split_at(s.len() - 1);
    let num: u64 = num_str.parse().ok()?;

    let secs = match unit {
        "s" => num,
        "m" => num * 60,
        "h" => num * 3600,
        "d" => num * 86400,
        _ => return None,
    };

    Some(Duration::from_secs(secs))
}

fn parse_rfc3339(s: &str) -> Option<i64> {
    // Minimal RFC 3339 parser: YYYY-MM-DDTHH:MM:SSZ or YYYY-MM-DDTHH:MM:SS+00:00
    let s = s.trim();

    let (datetime_part, offset_secs) = if s.ends_with('Z') || s.ends_with('z') {
        (&s[..s.len() - 1], 0i64)
    } else if s.len() > 6
        && (s.as_bytes()[s.len() - 6] == b'+' || s.as_bytes()[s.len() - 6] == b'-')
    {
        let (dt, off) = s.split_at(s.len() - 6);
        let sign = if off.starts_with('-') { -1i64 } else { 1i64 };
        let off = &off[1..];
        let hours: i64 = off[..2].parse().ok()?;
        let minutes: i64 = off[3..5].parse().ok()?;
        (dt, sign * (hours * 3600 + minutes * 60))
    } else {
        return None;
    };

    let parts: Vec<&str> = datetime_part.split('T').collect();
    if parts.len() != 2 {
        return None;
    }

    let date_parts: Vec<&str> = parts[0].split('-').collect();
    if date_parts.len() != 3 {
        return None;
    }
    let year: i64 = date_parts[0].parse().ok()?;
    let month: i64 = date_parts[1].parse().ok()?;
    let day: i64 = date_parts[2].parse().ok()?;

    let time_parts: Vec<&str> = parts[1].split(':').collect();
    if time_parts.len() != 3 {
        return None;
    }
    let hour: i64 = time_parts[0].parse().ok()?;
    let min: i64 = time_parts[1].parse().ok()?;
    let sec: i64 = time_parts[2].split('.').next()?.parse().ok()?;

    // Days from epoch using the same algorithm as local.rs
    let epoch_days = date_to_epoch_days(year, month, day);
    let epoch_secs = epoch_days * 86400 + hour * 3600 + min * 60 + sec - offset_secs;

    Some(epoch_secs * 1000)
}

fn date_to_epoch_days(year: i64, month: i64, day: i64) -> i64 {
    // Inverse of Howard Hinnant's civil days algorithm
    let (y, m) = if month <= 2 {
        (year - 1, month + 9)
    } else {
        (year, month - 3)
    };
    let era = y / 400;
    let yoe = y - era * 400;
    let doy = (153 * m + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    days
}

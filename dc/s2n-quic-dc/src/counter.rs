// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::time::Duration;
use s2n_quic_dc_metrics::format::{
    histogram_count_min_max, histogram_value_at_percentile, parse_histogram_buckets,
    parse_histogram_suffix, ParsedMetricsLine,
};
use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{
        atomic::{AtomicI64, Ordering},
        Arc, Mutex,
    },
    time::Instant,
};
use tokio::sync::mpsc;

pub use s2n_quic_dc_metrics::{Summary, Unit};

/// Stable identifier for metric metadata tracked in [`Registry`].
pub type MetricId = u64;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MetricKind {
    Counter,
    Gauge,
    Summary,
    Timer,
}

impl core::fmt::Display for MetricKind {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Counter => f.write_str("counter"),
            Self::Gauge => f.write_str("gauge"),
            Self::Summary => f.write_str("summary"),
            Self::Timer => f.write_str("timer"),
        }
    }
}

#[derive(Clone, Debug)]
pub struct MetricMetadata {
    pub id: MetricId,
    pub label: String,
    pub variant: Option<String>,
    pub kind: MetricKind,
    pub unit: Option<&'static str>,
    pub description: String,
}

/// A value that displays as empty when zero, suppressing it from metrics output.
struct NonZeroDisplay(i64);

impl core::fmt::Display for NonZeroDisplay {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        if self.0 != 0 {
            write!(f, "{}", self.0)
        } else {
            Ok(())
        }
    }
}

const DEFAULT_STATSD_UDP_MAX_PAYLOAD: usize = 1200;
const STATSD_HISTOGRAM_PERCENTILES: [u32; 4] = [50, 90, 95, 99];

#[derive(Clone, Debug)]
pub enum SparseMode {
    Never,
    Always,
    Once,
    Every(u64),
}

#[derive(Clone, Debug)]
pub struct ReporterConfig {
    pub interval: Duration,
    pub prefix: Option<String>,
    pub include_sparse: bool,
    pub sparse_mode: SparseMode,
    pub sinks: Vec<ReporterSink>,
}

impl ReporterConfig {
    pub fn new(interval: Duration) -> Self {
        Self {
            interval,
            prefix: None,
            include_sparse: false,
            sparse_mode: SparseMode::Never,
            sinks: vec![ReporterSink::Tracing],
        }
    }
}

#[derive(Clone, Debug)]
pub enum ReporterSink {
    Tracing,
    StatsdUdp(StatsdUdpConfig),
}

#[derive(Clone, Debug)]
pub struct StatsdUdpConfig {
    pub addr: SocketAddr,
    pub tx: mpsc::Sender<StatsdUdpPayloadBatch>,
    pub max_payload_size: usize,
}

impl StatsdUdpConfig {
    pub fn new(addr: SocketAddr, tx: mpsc::Sender<StatsdUdpPayloadBatch>) -> Self {
        Self {
            addr,
            tx,
            max_payload_size: DEFAULT_STATSD_UDP_MAX_PAYLOAD,
        }
    }

    /// Creates a `StatsdUdpConfig` and spawns a background task that sends batches over UDP.
    ///
    /// The task owns a single socket connected to `addr` and paces payloads using the provided
    /// `Rate` to avoid overwhelming the local listener.
    pub fn spawn(
        socket: std::net::UdpSocket,
        addr: SocketAddr,
        queue_depth: usize,
        rate: crate::socket::rate::Rate,
    ) -> Self {
        let (tx, rx) = mpsc::channel(queue_depth);
        let config = Self::new(addr, tx);

        tokio::spawn(statsd_udp_sender(socket, rx, rate));

        config
    }
}

async fn statsd_udp_sender(
    socket: std::net::UdpSocket,
    mut rx: mpsc::Receiver<StatsdUdpPayloadBatch>,
    rate: crate::socket::rate::Rate,
) {
    socket.set_nonblocking(true).ok();
    let socket = match tokio::net::UdpSocket::from_std(socket) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(error = %e, "failed to convert statsd UDP socket to async");
            return;
        }
    };

    while let Some(batch) = rx.recv().await {
        for payload in &batch.payloads {
            let target_nanos = rate.nanos_for_bytes(payload.len() as u64);
            let before = Instant::now();
            if let Err(e) = socket.send_to(payload, batch.addr).await {
                tracing::warn!(addr = %batch.addr, error = %e, "statsd UDP send failed");
                break;
            }
            let elapsed = before.elapsed();
            if let Some(remaining) = Duration::from_nanos(target_nanos).checked_sub(elapsed) {
                tokio::time::sleep(remaining).await;
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatsdUdpPayloadBatch {
    pub addr: SocketAddr,
    pub payloads: Vec<Vec<u8>>,
}

#[derive(Debug, PartialEq, Eq)]
struct RawMetricSample<'a> {
    key: &'a str,
    value: &'a str,
}

#[derive(Debug, PartialEq, Eq)]
struct ReportingPayload<'a> {
    raw_line: &'a str,
    parsed: ParsedMetricsLine<'a>,
    raw_samples: Vec<RawMetricSample<'a>>,
}

impl<'a> ReportingPayload<'a> {
    fn from_line(line: &'a str) -> Self {
        Self {
            raw_line: line,
            parsed: ParsedMetricsLine::parse(line),
            raw_samples: parse_raw_metric_samples(line),
        }
    }
}

trait ReporterOutputSink: Send {
    fn emit(&mut self, payload: &ReportingPayload<'_>, prefix: Option<&str>) -> Result<(), String>;
}

struct TracingSink;

impl ReporterOutputSink for TracingSink {
    fn emit(&mut self, payload: &ReportingPayload<'_>, prefix: Option<&str>) -> Result<(), String> {
        let raw = payload.raw_line;
        if let Some(prefix) = prefix.filter(|p| !p.is_empty()) {
            tracing::info!("[METRICS:{prefix}] {raw}");
        } else {
            tracing::info!("[METRICS] {raw}");
        }
        Ok(())
    }
}

struct StatsdUdpSink {
    config: StatsdUdpConfig,
}

impl StatsdUdpSink {
    fn new(config: StatsdUdpConfig) -> Self {
        Self { config }
    }
}

impl ReporterOutputSink for StatsdUdpSink {
    fn emit(&mut self, payload: &ReportingPayload<'_>, prefix: Option<&str>) -> Result<(), String> {
        let lines = encode_statsd_lines(&payload.raw_samples, prefix);
        if lines.is_empty() {
            return Ok(());
        }

        let (payloads, dropped) = chunk_statsd_lines(&lines, self.config.max_payload_size);
        if dropped > 0 {
            tracing::warn!(
                dropped,
                max_payload_size = self.config.max_payload_size,
                "dropped oversized statsd metrics"
            );
        }

        let batch = StatsdUdpPayloadBatch {
            addr: self.config.addr,
            payloads,
        };
        match self.config.tx.try_send(batch) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(_)) => {
                tracing::warn!(addr = %self.config.addr, "statsd payload queue full; dropping batch");
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                return Err("statsd payload queue disconnected".into());
            }
        }

        Ok(())
    }
}

/// Parses a raw metrics line into machine-exportable samples.
///
/// The expected input format is a comma-separated list of `key=value` pairs as emitted by the
/// metrics registry. Malformed segments (missing `=`) are ignored.
fn parse_raw_metric_samples(line: &str) -> Vec<RawMetricSample<'_>> {
    line.split(',')
        .filter_map(|part| {
            let (key, value) = part.split_once('=')?;
            Some(RawMetricSample {
                key: key.trim(),
                value: value.trim(),
            })
        })
        .collect()
}

/// Sanitizes metric names for StatsD formatting.
///
/// Allowed characters (`[a-zA-Z0-9_.-]`) are preserved, `:` is normalized to `.`, and all other
/// characters are replaced with `_`.
fn sanitize_metric_name(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    for c in input.chars() {
        let normalized = match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '_' | '.' | '-' => c,
            ':' => '.',
            _ => '_',
        };
        output.push(normalized);
    }
    output
}

fn with_metric_prefix(name: &str, prefix: Option<&str>) -> String {
    if let Some(prefix) = prefix.filter(|p| !p.is_empty()) {
        let prefix = sanitize_metric_name(prefix);
        if prefix.is_empty() {
            sanitize_metric_name(name)
        } else {
            format!("{prefix}.{}", sanitize_metric_name(name))
        }
    } else {
        sanitize_metric_name(name)
    }
}

/// Parses a scalar metric value and returns `(numeric_value, optional_suffix)` when valid.
///
/// Examples:
/// - `"123"` -> `Some(("123", None))`
/// - `"123 ect0"` -> `Some(("123", Some("ect0")))`
/// - `"abc"` -> `None`
fn parse_scalar_value(value: &str) -> Option<(&str, Option<&str>)> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }

    if let Some((number, suffix)) = value.split_once(' ') {
        if number.parse::<f64>().is_ok() {
            Some((number, Some(suffix.trim()).filter(|v| !v.is_empty())))
        } else {
            None
        }
    } else if value.parse::<f64>().is_ok() {
        Some((value, None))
    } else {
        None
    }
}

fn convert_to_milliseconds(value: u64, unit: &str) -> Option<f64> {
    match unit {
        "us" => Some(value as f64 / 1_000.0),
        "ms" => Some(value as f64),
        "s" => Some(value as f64 * 1_000.0),
        _ => None,
    }
}

/// Encodes raw metric samples into StatsD lines.
///
/// Scalars are emitted as counters (`|c`) except `.depth` metrics, which are emitted as gauges
/// (`|g`) plus timer-formatted distribution samples (`|ms`) for percentile/burst analysis.
/// Histogram-like values (`value` containing `*`) are emitted as `.count` plus percentile
/// (`.p50`, `.p90`, `.p95`, `.p99`, `.max`) metrics; time units (`us`, `ms`, `s`) are normalized
/// to StatsD timer units (`|ms`), while non-time units are emitted as gauges.
fn encode_statsd_lines(samples: &[RawMetricSample<'_>], prefix: Option<&str>) -> Vec<String> {
    let mut lines = Vec::new();

    for sample in samples {
        if sample.value.contains('*') {
            let (data, unit, variant) = parse_histogram_suffix(sample.value);
            let buckets = parse_histogram_buckets(data);
            if buckets.is_empty() {
                continue;
            }

            let mut metric = with_metric_prefix(sample.key, prefix);
            if !variant.is_empty() {
                metric.push('.');
                metric.push_str(&sanitize_metric_name(variant));
            }

            let (count, min, max) = histogram_count_min_max(&buckets);
            lines.push(format!("{metric}.count:{count}|c"));

            if let Some(min) = convert_to_milliseconds(min, unit) {
                lines.push(format!("{metric}.min:{min:.3}|ms"));
            } else {
                lines.push(format!("{metric}.min:{min}|g"));
            }

            for percentile in STATSD_HISTOGRAM_PERCENTILES {
                let value = histogram_value_at_percentile(&buckets, percentile);
                if let Some(value) = convert_to_milliseconds(value, unit) {
                    lines.push(format!("{metric}.p{percentile}:{value:.3}|ms"));
                } else {
                    lines.push(format!("{metric}.p{percentile}:{value}|g"));
                }
            }

            if let Some(max) = convert_to_milliseconds(max, unit) {
                lines.push(format!("{metric}.max:{max:.3}|ms"));
            } else {
                lines.push(format!("{metric}.max:{max}|g"));
            }

            continue;
        }

        let Some((number, suffix)) = parse_scalar_value(sample.value) else {
            continue;
        };

        let mut metric = with_metric_prefix(sample.key, prefix);

        if let Some(suffix) = suffix.filter(|s| *s != "B") {
            metric.push('.');
            metric.push_str(&sanitize_metric_name(suffix));
        }

        if sample.key.ends_with(".depth") {
            lines.push(format!("{metric}:{number}|g"));
            lines.push(format!("{metric}.distribution:{number}|ms"));
        } else {
            lines.push(format!("{metric}:{number}|c"));
        }
    }

    lines
}

/// Batches StatsD lines into newline-delimited UDP payloads up to `max_payload_size`.
///
/// Returns `(payloads, dropped_oversized_lines)`. When `max_payload_size == 0`, no payloads are
/// emitted and all lines are counted as dropped.
fn chunk_statsd_lines(lines: &[String], max_payload_size: usize) -> (Vec<Vec<u8>>, usize) {
    // Zero is an invalid payload size and cannot hold even a single byte, so all lines are
    // reported as dropped in this case.
    if max_payload_size == 0 {
        return (Vec::new(), lines.len());
    }

    let mut payloads = Vec::new();
    let mut current = Vec::new();
    let mut dropped = 0;

    for line in lines {
        let bytes = line.as_bytes();
        if bytes.len() > max_payload_size {
            dropped += 1;
            continue;
        }

        let required = if current.is_empty() {
            bytes.len()
        } else {
            current.len() + 1 + bytes.len()
        };

        if required > max_payload_size && !current.is_empty() {
            payloads.push(std::mem::take(&mut current));
        }

        if !current.is_empty() {
            current.push(b'\n');
        }
        current.extend_from_slice(bytes);
    }

    if !current.is_empty() {
        payloads.push(current);
    }

    (payloads, dropped)
}

fn build_sinks(config: &[ReporterSink]) -> Vec<Box<dyn ReporterOutputSink>> {
    let mut sinks: Vec<Box<dyn ReporterOutputSink>> = Vec::with_capacity(config.len());

    for sink in config {
        match sink {
            ReporterSink::Tracing => sinks.push(Box::new(TracingSink)),
            ReporterSink::StatsdUdp(config) => {
                sinks.push(Box::new(StatsdUdpSink::new(config.clone())))
            }
        }
    }

    sinks
}

fn dispatch_payload_to_sinks(
    sinks: &mut [Box<dyn ReporterOutputSink>],
    payload: &ReportingPayload<'_>,
    prefix: Option<&str>,
) {
    for sink in sinks {
        if let Err(error) = sink.emit(payload, prefix) {
            tracing::warn!(?error, "failed to emit metrics to sink");
        }
    }
}

fn report_once(
    inner: &s2n_quic_dc_metrics::Registry,
    include_sparse: bool,
    prefix: Option<&str>,
    sinks: &mut [Box<dyn ReporterOutputSink>],
) {
    if let Some(line) = inner.try_take_current_metrics_line_sparse(include_sparse) {
        let payload = ReportingPayload::from_line(&line);
        dispatch_payload_to_sinks(sinks, &payload, prefix);
    }
}

// ── Counter ─────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct Counter {
    inner: s2n_quic_dc_metrics::Counter,
    metric_id: MetricId,
    metadata: Arc<Mutex<HashMap<MetricId, MetricMetadata>>>,
}

impl Counter {
    #[inline]
    pub fn add(&self, v: u64) {
        self.inner.increment(v);
    }

    #[inline]
    pub fn metric_metadata(&self) -> Option<MetricMetadata> {
        self.metadata.lock().unwrap().get(&self.metric_id).cloned()
    }

    #[inline]
    pub fn with_description(self, description: impl core::fmt::Display) -> Self {
        if let Some(metadata) = self.metadata.lock().unwrap().get_mut(&self.metric_id) {
            metadata.description = description.to_string();
        }
        self
    }
}

impl core::ops::AddAssign<u64> for Counter {
    #[inline]
    fn add_assign(&mut self, rhs: u64) {
        self.add(rhs);
    }
}

// ── Gauge ───────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct Gauge {
    inner: Arc<AtomicI64>,
    metric_id: MetricId,
    metadata: Arc<Mutex<HashMap<MetricId, MetricMetadata>>>,
}

impl Gauge {
    #[inline]
    pub fn add(&self, v: i64) -> i64 {
        self.inner.fetch_add(v, Ordering::Relaxed) + v
    }

    #[inline]
    pub fn sub(&self, v: i64) -> i64 {
        self.inner.fetch_sub(v, Ordering::Relaxed) - v
    }

    #[inline]
    pub fn get(&self) -> i64 {
        self.inner.load(Ordering::Relaxed)
    }

    #[inline]
    pub fn metric_metadata(&self) -> Option<MetricMetadata> {
        self.metadata.lock().unwrap().get(&self.metric_id).cloned()
    }

    #[inline]
    pub fn with_description(self, description: impl core::fmt::Display) -> Self {
        if let Some(metadata) = self.metadata.lock().unwrap().get_mut(&self.metric_id) {
            metadata.description = description.to_string();
        }
        self
    }
}

// ── Timer ───────────────────────────────────────────────────────────────────

/// A histogram that records elapsed durations via a guard pattern.
///
/// Call `timer.start()` to begin timing; the returned guard records
/// the elapsed duration into the underlying `Summary` on drop.
#[derive(Clone)]
pub struct Timer {
    summary: Summary,
    metric_id: MetricId,
    metadata: Arc<Mutex<HashMap<MetricId, MetricMetadata>>>,
}

impl Timer {
    #[inline]
    pub fn start(&self) -> TimerGuard<'_> {
        self.start_at(Instant::now())
    }

    /// Starts a timer from a caller-provided `Instant`.
    ///
    /// Use this when multiple task metrics should share the exact same poll-start
    /// timestamp (for example, per-poll execution time and inter-poll latency).
    #[inline]
    pub fn start_at(&self, start: Instant) -> TimerGuard<'_> {
        TimerGuard {
            summary: &self.summary,
            start,
            recorded: false,
        }
    }

    #[inline]
    pub fn record(&self, duration: Duration) {
        self.summary.record_duration(duration);
    }

    #[inline]
    pub fn metric_metadata(&self) -> Option<MetricMetadata> {
        self.metadata.lock().unwrap().get(&self.metric_id).cloned()
    }

    #[inline]
    pub fn with_description(self, description: impl core::fmt::Display) -> Self {
        if let Some(metadata) = self.metadata.lock().unwrap().get_mut(&self.metric_id) {
            metadata.description = description.to_string();
        }
        self
    }
}

#[derive(Clone)]
pub struct SummaryMetric {
    summary: Summary,
    metric_id: MetricId,
    metadata: Arc<Mutex<HashMap<MetricId, MetricMetadata>>>,
}

impl SummaryMetric {
    #[inline]
    pub fn record_value(&self, value: u64) {
        self.summary.record_value(value);
    }

    #[inline]
    pub fn metric_metadata(&self) -> Option<MetricMetadata> {
        self.metadata.lock().unwrap().get(&self.metric_id).cloned()
    }

    #[inline]
    pub fn with_description(self, description: impl core::fmt::Display) -> Self {
        if let Some(metadata) = self.metadata.lock().unwrap().get_mut(&self.metric_id) {
            metadata.description = description.to_string();
        }
        self
    }
}

pub struct TimerGuard<'a> {
    summary: &'a Summary,
    start: Instant,
    recorded: bool,
}

impl TimerGuard<'_> {
    #[inline]
    pub fn record(mut self) -> Instant {
        let now = Instant::now();
        self.summary.record_duration(now.duration_since(self.start));
        self.recorded = true;
        now
    }
}

impl Drop for TimerGuard<'_> {
    #[inline]
    fn drop(&mut self) {
        if !self.recorded {
            self.summary.record_duration(self.start.elapsed());
        }
    }
}

// ── Task ────────────────────────────────────────────────────────────────────

/// Metric bundle for a drain-budgeted task poll loop.
///
/// - `drained`: number of items processed in a single poll
/// - `time`: wall-clock duration spent inside a poll
/// - `next_poll_latency`: elapsed time from the end of one poll to the start
///   of the next poll
#[derive(Clone)]
pub struct Task {
    registration: Arc<Mutex<TaskRegistrationMetadata>>,
    pub drained: SummaryMetric,
    pub time: Timer,
    pub next_poll_latency: Timer,
}

#[derive(Clone, Default)]
struct TaskRegistrationMetadata {
    name: String,
    description: String,
    function: String,
}

impl Task {
    pub const DRAINED_DESCRIPTION: &'static str =
        "Number of items processed per poll for this task";
    pub const DRAINED_UNIT: &'static str = "count";
    pub const TIME_DESCRIPTION: &'static str = "Wall-clock duration spent inside each task poll";
    pub const TIME_UNIT: &'static str = "microsecond";
    pub const NEXT_POLL_LATENCY_DESCRIPTION: &'static str =
        "Wall-clock latency between consecutive task polls";
    pub const NEXT_POLL_LATENCY_UNIT: &'static str = "microsecond";

    pub fn metrics(&self) -> Vec<MetricMetadata> {
        [
            self.drained.metric_metadata(),
            self.time.metric_metadata(),
            self.next_poll_latency.metric_metadata(),
        ]
        .into_iter()
        .flatten()
        .collect()
    }

    pub fn with_registration_metadata_ref<T>(&self, f: impl FnOnce(&str, &str, &str) -> T) -> T {
        let registration = self.registration.lock().unwrap();
        f(
            registration.name.as_str(),
            registration.description.as_str(),
            registration.function.as_str(),
        )
    }

    pub fn with_registration_metadata(
        self,
        name: impl core::fmt::Display,
        description: impl core::fmt::Display,
        function: impl core::fmt::Display,
    ) -> Self {
        self.with_registration_name(name)
            .with_registration_description(description)
            .with_registration_function(function)
    }

    pub fn with_registration_name(self, name: impl core::fmt::Display) -> Self {
        {
            let mut registration = self.registration.lock().unwrap();
            registration.name = name.to_string();
        }
        self
    }

    pub fn with_registration_description(self, description: impl core::fmt::Display) -> Self {
        {
            let mut registration = self.registration.lock().unwrap();
            registration.description = description.to_string();
        }
        self
    }

    pub fn with_registration_function(self, function: impl core::fmt::Display) -> Self {
        {
            let mut registration = self.registration.lock().unwrap();
            registration.function = function.to_string();
        }
        self
    }
}

// ── QueueGauge ──────────────────────────────────────────────────────────────
// TODO: add per-item sojourn time tracking (enqueue→dequeue latency as a histogram)

#[derive(Clone)]
pub struct QueueGauge {
    registry: Registry,
    key: String,
    pub throughput: Counter,
    pub drain: Counter,
    pub depth: Gauge,
    pub depth_distribution: SummaryMetric,
}

#[derive(Clone, Default)]
struct QueueRegistrationMetadata {
    label: String,
    variant: Option<String>,
    name: String,
    description: String,
    function: String,
}

#[derive(Clone, Default)]
struct QueueEndpointMetadata {
    task_name: String,
    function: String,
}

#[derive(Clone)]
pub struct QueueSender {
    queue: QueueGauge,
    metric: Counter,
}

#[derive(Clone)]
pub struct QueueReceiver {
    queue: QueueGauge,
    metric: Counter,
}

impl QueueGauge {
    pub const ENQUEUED_DESCRIPTION: &'static str = "Total queue enqueue events";
    pub const ENQUEUED_UNIT: &'static str = "count";
    pub const DEQUEUED_DESCRIPTION: &'static str = "Total queue dequeue events";
    pub const DEQUEUED_UNIT: &'static str = "count";
    pub const DEPTH_DESCRIPTION: &'static str = "Current queue depth";
    pub const DEPTH_UNIT: &'static str = "count";
    pub const DEPTH_DISTRIBUTION_DESCRIPTION: &'static str =
        "Distribution of observed queue depth values";
    pub const DEPTH_DISTRIBUTION_UNIT: &'static str = "count";

    pub fn metrics(&self) -> Vec<MetricMetadata> {
        [
            self.throughput.metric_metadata(),
            self.drain.metric_metadata(),
            self.depth.metric_metadata(),
            self.depth_distribution.metric_metadata(),
        ]
        .into_iter()
        .flatten()
        .collect()
    }

    pub fn with_registration_metadata(
        self,
        name: impl core::fmt::Display,
        description: impl core::fmt::Display,
        function: impl core::fmt::Display,
    ) -> Self {
        self.with_registration_name(name)
            .with_registration_description(description)
            .with_registration_function(function)
    }

    pub fn with_registration_name(self, name: impl core::fmt::Display) -> Self {
        if let Some(registration) = self
            .registry
            .queue_metadata
            .lock()
            .unwrap()
            .get_mut(&self.key)
        {
            registration.name = name.to_string();
        }
        self
    }

    pub fn with_registration_description(self, description: impl core::fmt::Display) -> Self {
        if let Some(registration) = self
            .registry
            .queue_metadata
            .lock()
            .unwrap()
            .get_mut(&self.key)
        {
            registration.description = description.to_string();
        }
        self
    }

    pub fn with_registration_function(self, function: impl core::fmt::Display) -> Self {
        if let Some(registration) = self
            .registry
            .queue_metadata
            .lock()
            .unwrap()
            .get_mut(&self.key)
        {
            registration.function = function.to_string();
        }
        self
    }

    pub fn with_registration_metadata_ref<T>(&self, f: impl FnOnce(&str, &str, &str) -> T) -> T {
        let registrations = self.registry.queue_metadata.lock().unwrap();
        let registration = registrations
            .get(&self.key)
            .expect("queue registration metadata missing");
        f(
            registration.name.as_str(),
            registration.description.as_str(),
            registration.function.as_str(),
        )
    }

    fn register_endpoint_counter(
        &self,
        suffix: &str,
        task_name: &str,
        description: impl core::fmt::Display,
    ) -> Counter {
        let registrations = self.registry.queue_metadata.lock().unwrap();
        let registration = registrations
            .get(&self.key)
            .expect("queue registration metadata missing");
        let label = format!("{}.{}", registration.label, suffix);
        let variant = match registration.variant.as_deref() {
            Some(variant) => Some(format!("{variant}.{task_name}")),
            None => Some(task_name.to_string()),
        };
        drop(registrations);
        let metric_id = self.registry.register_metric_metadata(
            &label,
            variant.as_deref(),
            MetricKind::Counter,
            Some("count"),
            description,
        );
        let counter = self.registry.counter_handle(
            self.registry.inner.register_counter(label, variant),
            metric_id,
        );
        counter
    }

    pub fn sender(&self, task_name: impl core::fmt::Display) -> QueueSender {
        let task_name = task_name.to_string();
        let metric = self.register_endpoint_counter(
            "sender",
            &task_name,
            format_args!("Queue sends performed by {task_name}"),
        );
        self.registry
            .queue_endpoint_metadata
            .lock()
            .unwrap()
            .insert(
                metric.metric_id,
                QueueEndpointMetadata {
                    task_name,
                    ..Default::default()
                },
            );
        QueueSender {
            queue: self.clone(),
            metric,
        }
    }

    pub fn receiver(&self, task_name: impl core::fmt::Display) -> QueueReceiver {
        let task_name = task_name.to_string();
        let metric = self.register_endpoint_counter(
            "receiver",
            &task_name,
            format_args!("Queue receives performed by {task_name}"),
        );
        self.registry
            .queue_endpoint_metadata
            .lock()
            .unwrap()
            .insert(
                metric.metric_id,
                QueueEndpointMetadata {
                    task_name,
                    ..Default::default()
                },
            );
        QueueReceiver {
            queue: self.clone(),
            metric,
        }
    }
}

impl QueueGauge {
    #[inline]
    pub fn enqueue(&self, count: u64) {
        self.throughput.add(count);
        let depth = self.depth.add(count as i64).max(0) as u64;
        self.depth_distribution.record_value(depth);
    }

    #[inline]
    pub fn dequeue(&self) {
        self.drain.add(1);
        self.depth.sub(1);
    }

    #[inline]
    pub fn dequeue_n(&self, count: u64) {
        self.drain.add(count);
        self.depth.sub(count as i64);
    }
}

impl QueueSender {
    pub fn with_description(mut self, description: impl core::fmt::Display) -> Self {
        self.metric = self.metric.with_description(description);
        self
    }

    pub fn with_function(self, function: impl core::fmt::Display) -> Self {
        if let Some(metadata) = self
            .queue
            .registry
            .queue_endpoint_metadata
            .lock()
            .unwrap()
            .get_mut(&self.metric.metric_id)
        {
            metadata.function = function.to_string();
        }
        self
    }

    pub(crate) fn task_name(&self) -> String {
        self.queue
            .registry
            .queue_endpoint_metadata
            .lock()
            .unwrap()
            .get(&self.metric.metric_id)
            .map(|metadata| metadata.task_name.clone())
            .unwrap_or_default()
    }

    pub fn channel_metadata<T>(&self, f: impl FnOnce(&str, &str, &str) -> T) -> T {
        self.queue.with_registration_metadata_ref(f)
    }

    pub fn metrics(&self) -> Vec<MetricMetadata> {
        self.metric.metric_metadata().into_iter().collect()
    }

    pub(crate) fn description(&self) -> String {
        self.metric
            .metric_metadata()
            .map(|metadata| metadata.description)
            .unwrap_or_default()
    }

    pub(crate) fn function(&self) -> String {
        self.queue
            .registry
            .queue_endpoint_metadata
            .lock()
            .unwrap()
            .get(&self.metric.metric_id)
            .map(|metadata| metadata.function.clone())
            .unwrap_or_default()
    }

    #[inline]
    fn on_send(&self, count: u64) {
        self.metric.add(count);
        self.queue.enqueue(count);
    }
}

impl QueueReceiver {
    pub fn with_description(mut self, description: impl core::fmt::Display) -> Self {
        self.metric = self.metric.with_description(description);
        self
    }

    pub fn with_function(self, function: impl core::fmt::Display) -> Self {
        if let Some(metadata) = self
            .queue
            .registry
            .queue_endpoint_metadata
            .lock()
            .unwrap()
            .get_mut(&self.metric.metric_id)
        {
            metadata.function = function.to_string();
        }
        self
    }

    pub(crate) fn task_name(&self) -> String {
        self.queue
            .registry
            .queue_endpoint_metadata
            .lock()
            .unwrap()
            .get(&self.metric.metric_id)
            .map(|metadata| metadata.task_name.clone())
            .unwrap_or_default()
    }

    pub fn channel_metadata<T>(&self, f: impl FnOnce(&str, &str, &str) -> T) -> T {
        self.queue.with_registration_metadata_ref(f)
    }

    pub fn metrics(&self) -> Vec<MetricMetadata> {
        self.metric.metric_metadata().into_iter().collect()
    }

    pub(crate) fn description(&self) -> String {
        self.metric
            .metric_metadata()
            .map(|metadata| metadata.description)
            .unwrap_or_default()
    }

    pub(crate) fn function(&self) -> String {
        self.queue
            .registry
            .queue_endpoint_metadata
            .lock()
            .unwrap()
            .get(&self.metric.metric_id)
            .map(|metadata| metadata.function.clone())
            .unwrap_or_default()
    }

    #[inline]
    fn on_receive(&self, count: u64) {
        self.metric.add(count);
        self.queue.dequeue_n(count);
    }
}

// ── Registry ────────────────────────────────────────────────────────────────

#[derive(Clone, Default)]
pub struct Registry {
    inner: s2n_quic_dc_metrics::Registry,
    queue_gauges: Arc<Mutex<HashMap<String, QueueGauge>>>,
    queue_metadata: Arc<Mutex<HashMap<String, QueueRegistrationMetadata>>>,
    queue_endpoint_metadata: Arc<Mutex<HashMap<MetricId, QueueEndpointMetadata>>>,
    metric_metadata: Arc<Mutex<HashMap<MetricId, MetricMetadata>>>,
}

impl Registry {
    const COUNT_UNIT: &'static str = "count";
    const MICROSECOND_UNIT: &'static str = "microsecond";

    pub fn new() -> Self {
        Self {
            inner: s2n_quic_dc_metrics::Registry::new(),
            queue_gauges: Arc::new(Mutex::new(HashMap::new())),
            queue_metadata: Arc::new(Mutex::new(HashMap::new())),
            queue_endpoint_metadata: Arc::new(Mutex::new(HashMap::new())),
            metric_metadata: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn register_metric_metadata(
        &self,
        label: impl core::fmt::Display,
        variant: Option<&str>,
        kind: MetricKind,
        unit: Option<&'static str>,
        description: impl core::fmt::Display,
    ) -> MetricId {
        let mut metric_metadata = self.metric_metadata.lock().unwrap();
        // Metric metadata entries are append-only for the lifetime of the registry,
        // so deriving the next id from current length preserves uniqueness.
        let id = metric_metadata.len() as MetricId + 1;
        metric_metadata.insert(
            id,
            MetricMetadata {
                id,
                label: label.to_string(),
                variant: variant.map(|variant| variant.to_string()),
                kind,
                unit,
                description: description.to_string(),
            },
        );
        id
    }

    fn counter_handle(&self, inner: s2n_quic_dc_metrics::Counter, metric_id: MetricId) -> Counter {
        Counter {
            inner,
            metric_id,
            metadata: self.metric_metadata.clone(),
        }
    }

    fn gauge_handle(&self, inner: Arc<AtomicI64>, metric_id: MetricId) -> Gauge {
        Gauge {
            inner,
            metric_id,
            metadata: self.metric_metadata.clone(),
        }
    }

    fn timer_handle(&self, summary: Summary, metric_id: MetricId) -> Timer {
        Timer {
            summary,
            metric_id,
            metadata: self.metric_metadata.clone(),
        }
    }

    fn summary_handle(&self, summary: Summary, metric_id: MetricId) -> SummaryMetric {
        SummaryMetric {
            summary,
            metric_id,
            metadata: self.metric_metadata.clone(),
        }
    }

    pub fn register(&self, label: impl core::fmt::Display) -> Counter {
        let label = label.to_string();
        let metric_id = self.register_metric_metadata(&label, None, MetricKind::Counter, None, "");
        self.counter_handle(self.inner.register_counter(label, None), metric_id)
    }

    pub fn register_bytes(&self, label: impl core::fmt::Display) -> Counter {
        let label = label.to_string();
        let metric_id =
            self.register_metric_metadata(&label, None, MetricKind::Counter, Some("B"), "");
        self.counter_handle(
            self.inner.register_counter(label, Some("B".into())),
            metric_id,
        )
    }

    pub fn register_nominal(
        &self,
        label: impl core::fmt::Display,
        variant: impl core::fmt::Display,
    ) -> Counter {
        let label = label.to_string();
        let variant = variant.to_string();
        let metric_id =
            self.register_metric_metadata(&label, Some(&variant), MetricKind::Counter, None, "");
        self.counter_handle(self.inner.register_counter(label, Some(variant)), metric_id)
    }

    pub fn register_queue_gauge(&self, label: impl core::fmt::Display) -> QueueGauge {
        let label = label.to_string();
        let mut gauges = self.queue_gauges.lock().unwrap();
        if let Some(existing) = gauges.get(&label) {
            return existing.clone();
        }

        let enqueued_id = self.register_metric_metadata(
            format!("{label}.enq"),
            None,
            MetricKind::Counter,
            Some(Self::COUNT_UNIT),
            QueueGauge::ENQUEUED_DESCRIPTION,
        );
        let throughput = self.counter_handle(
            self.inner.register_counter(format!("{label}.enq"), None),
            enqueued_id,
        );
        let dequeued_id = self.register_metric_metadata(
            format!("{label}.drain"),
            None,
            MetricKind::Counter,
            Some(Self::COUNT_UNIT),
            QueueGauge::DEQUEUED_DESCRIPTION,
        );
        let drain = self.counter_handle(
            self.inner.register_counter(format!("{label}.drain"), None),
            dequeued_id,
        );
        let depth_inner = Arc::new(AtomicI64::new(0));
        let depth_clone = depth_inner.clone();
        self.inner
            .register_list_callback(format!("{label}.depth"), None, Unit::Count, move || {
                NonZeroDisplay(depth_clone.load(Ordering::Relaxed))
            });
        let depth_id = self.register_metric_metadata(
            format!("{label}.depth"),
            None,
            MetricKind::Gauge,
            Some(Self::COUNT_UNIT),
            QueueGauge::DEPTH_DESCRIPTION,
        );
        let depth_distribution_inner =
            self.inner
                .register_summary(format!("{label}.depth_dist"), None, Unit::Count);
        let depth_distribution_id = self.register_metric_metadata(
            format!("{label}.depth_dist"),
            None,
            MetricKind::Summary,
            Some(Self::COUNT_UNIT),
            QueueGauge::DEPTH_DISTRIBUTION_DESCRIPTION,
        );

        let gauge = QueueGauge {
            registry: self.clone(),
            key: label.clone(),
            throughput,
            drain,
            depth: self.gauge_handle(depth_inner, depth_id),
            depth_distribution: self
                .summary_handle(depth_distribution_inner, depth_distribution_id),
        };
        self.queue_metadata.lock().unwrap().insert(
            label.clone(),
            QueueRegistrationMetadata {
                label: label.clone(),
                variant: None,
                name: label.clone(),
                ..Default::default()
            },
        );
        gauges.insert(label, gauge.clone());
        gauge
    }

    pub fn register_queue_gauge_nominal(
        &self,
        label: impl core::fmt::Display,
        variant: impl core::fmt::Display,
    ) -> QueueGauge {
        let label = label.to_string();
        let variant = variant.to_string();
        let key = format!("{label}.{variant}");
        let mut gauges = self.queue_gauges.lock().unwrap();
        if let Some(existing) = gauges.get(&key) {
            return existing.clone();
        }

        let variant_option = Some(variant.clone());
        let enqueued_id = self.register_metric_metadata(
            format!("{label}.enq"),
            Some(&variant),
            MetricKind::Counter,
            Some(Self::COUNT_UNIT),
            QueueGauge::ENQUEUED_DESCRIPTION,
        );
        let throughput = self.counter_handle(
            self.inner
                .register_counter(format!("{label}.enq"), variant_option.clone()),
            enqueued_id,
        );
        let dequeued_id = self.register_metric_metadata(
            format!("{label}.drain"),
            Some(&variant),
            MetricKind::Counter,
            Some(Self::COUNT_UNIT),
            QueueGauge::DEQUEUED_DESCRIPTION,
        );
        let drain = self.counter_handle(
            self.inner
                .register_counter(format!("{label}.drain"), variant_option.clone()),
            dequeued_id,
        );
        let depth_inner = Arc::new(AtomicI64::new(0));
        let depth_clone = depth_inner.clone();
        self.inner.register_list_callback(
            format!("{label}.depth"),
            variant_option.clone(),
            Unit::Count,
            move || NonZeroDisplay(depth_clone.load(Ordering::Relaxed)),
        );
        let depth_id = self.register_metric_metadata(
            format!("{label}.depth"),
            Some(&variant),
            MetricKind::Gauge,
            Some(Self::COUNT_UNIT),
            QueueGauge::DEPTH_DESCRIPTION,
        );
        let depth_distribution_inner =
            self.inner
                .register_summary(format!("{label}.depth_dist"), variant_option, Unit::Count);
        let depth_distribution_id = self.register_metric_metadata(
            format!("{label}.depth_dist"),
            Some(&variant),
            MetricKind::Summary,
            Some(Self::COUNT_UNIT),
            QueueGauge::DEPTH_DISTRIBUTION_DESCRIPTION,
        );

        let gauge = QueueGauge {
            registry: self.clone(),
            key: key.clone(),
            throughput,
            drain,
            depth: self.gauge_handle(depth_inner, depth_id),
            depth_distribution: self
                .summary_handle(depth_distribution_inner, depth_distribution_id),
        };
        self.queue_metadata.lock().unwrap().insert(
            key.clone(),
            QueueRegistrationMetadata {
                label: label.clone(),
                variant: Some(variant.clone()),
                name: key.clone(),
                ..Default::default()
            },
        );
        gauges.insert(key, gauge.clone());
        gauge
    }

    pub fn register_gauge(&self, label: impl core::fmt::Display) -> Gauge {
        let label = label.to_string();
        let metric_id = self.register_metric_metadata(&label, None, MetricKind::Gauge, None, "");
        let inner = Arc::new(AtomicI64::new(0));
        let inner_clone = inner.clone();
        self.inner
            .register_list_callback(label, None, Unit::Count, move || {
                NonZeroDisplay(inner_clone.load(Ordering::Relaxed))
            });
        self.gauge_handle(inner, metric_id)
    }

    pub fn register_summary(&self, label: impl core::fmt::Display, unit: Unit) -> Summary {
        self.inner.register_summary(label.to_string(), None, unit)
    }

    pub fn register_nominal_summary(
        &self,
        label: impl core::fmt::Display,
        variant: impl core::fmt::Display,
        unit: Unit,
    ) -> Summary {
        self.inner
            .register_summary(label.to_string(), Some(variant.to_string()), unit)
    }

    pub fn register_timer(&self, label: impl core::fmt::Display) -> Timer {
        let label = label.to_string();
        let metric_id = self.register_metric_metadata(
            &label,
            None,
            MetricKind::Timer,
            Some(Self::MICROSECOND_UNIT),
            "",
        );
        self.timer_handle(
            self.inner.register_summary(label, None, Unit::Microsecond),
            metric_id,
        )
    }

    pub fn register_task(&self, label: impl core::fmt::Display) -> Task {
        let label = label.to_string();
        let drained_label = format!("{label}.drained");
        let time_label = format!("{label}.time");
        let next_poll_latency_label = format!("{label}.next_poll_latency");
        let drained_id = self.register_metric_metadata(
            &drained_label,
            None,
            MetricKind::Summary,
            Some(Self::COUNT_UNIT),
            Task::DRAINED_DESCRIPTION,
        );
        let time_id = self.register_metric_metadata(
            &time_label,
            None,
            MetricKind::Timer,
            Some(Self::MICROSECOND_UNIT),
            Task::TIME_DESCRIPTION,
        );
        let next_poll_latency_id = self.register_metric_metadata(
            &next_poll_latency_label,
            None,
            MetricKind::Timer,
            Some(Self::MICROSECOND_UNIT),
            Task::NEXT_POLL_LATENCY_DESCRIPTION,
        );
        Task {
            registration: Arc::new(Mutex::new(TaskRegistrationMetadata {
                name: label.clone(),
                ..Default::default()
            })),
            drained: self.summary_handle(
                self.register_summary(drained_label, Unit::Count),
                drained_id,
            ),
            time: self.timer_handle(
                self.inner
                    .register_summary(time_label, None, Unit::Microsecond),
                time_id,
            ),
            next_poll_latency: self.timer_handle(
                self.inner
                    .register_summary(next_poll_latency_label, None, Unit::Microsecond),
                next_poll_latency_id,
            ),
        }
    }

    pub fn register_nominal_timer(
        &self,
        label: impl core::fmt::Display,
        variant: impl core::fmt::Display,
    ) -> Timer {
        let label = label.to_string();
        let variant = variant.to_string();
        let metric_id = self.register_metric_metadata(
            &label,
            Some(&variant),
            MetricKind::Timer,
            Some(Self::MICROSECOND_UNIT),
            "",
        );
        self.timer_handle(
            self.inner
                .register_summary(label, Some(variant), Unit::Microsecond),
            metric_id,
        )
    }

    pub fn register_nominal_task(
        &self,
        label: impl core::fmt::Display,
        variant: impl core::fmt::Display,
    ) -> Task {
        let label = label.to_string();
        let variant = variant.to_string();
        let drained_label = format!("{label}.drained");
        let time_label = format!("{label}.time");
        let next_poll_latency_label = format!("{label}.next_poll_latency");
        let drained_id = self.register_metric_metadata(
            &drained_label,
            Some(&variant),
            MetricKind::Summary,
            Some(Self::COUNT_UNIT),
            Task::DRAINED_DESCRIPTION,
        );
        let time_id = self.register_metric_metadata(
            &time_label,
            Some(&variant),
            MetricKind::Timer,
            Some(Self::MICROSECOND_UNIT),
            Task::TIME_DESCRIPTION,
        );
        let next_poll_latency_id = self.register_metric_metadata(
            &next_poll_latency_label,
            Some(&variant),
            MetricKind::Timer,
            Some(Self::MICROSECOND_UNIT),
            Task::NEXT_POLL_LATENCY_DESCRIPTION,
        );
        Task {
            registration: Arc::new(Mutex::new(TaskRegistrationMetadata {
                name: format!("{label}.{variant}"),
                ..Default::default()
            })),
            drained: self.summary_handle(
                self.register_nominal_summary(drained_label, &variant, Unit::Count),
                drained_id,
            ),
            time: self.timer_handle(
                self.inner
                    .register_summary(time_label, Some(variant.clone()), Unit::Microsecond),
                time_id,
            ),
            next_poll_latency: self.timer_handle(
                self.inner.register_summary(
                    next_poll_latency_label,
                    Some(variant),
                    Unit::Microsecond,
                ),
                next_poll_latency_id,
            ),
        }
    }

    pub fn spawn_reporter(&self, interval: Duration) {
        self.spawn_reporter_with_label(interval, "")
    }

    pub fn spawn_reporter_with_label(&self, interval: Duration, label: impl Into<String>) {
        let label = label.into();
        let mut config = ReporterConfig::new(interval);
        if !label.is_empty() {
            config.prefix = Some(label);
        }
        self.spawn_reporter_with_config(config);
    }

    pub fn spawn_reporter_with_config(&self, config: ReporterConfig) {
        let inner = self.inner.clone();
        let interval = config.interval;
        let prefix = config.prefix.clone();
        let sparse_mode = config.sparse_mode.clone();
        let sinks = build_sinks(&config.sinks);

        #[cfg(any(test, feature = "testing"))]
        if bach::is_active() {
            bach::spawn(report_loop(inner, sparse_mode, prefix, sinks, move || {
                bach::time::sleep(interval)
            }));
            return;
        }

        tokio::spawn(report_loop(inner, sparse_mode, prefix, sinks, move || {
            tokio::time::sleep(interval)
        }));
    }
}

async fn report_loop<F, Fut>(
    inner: s2n_quic_dc_metrics::Registry,
    sparse_mode: SparseMode,
    prefix: Option<String>,
    mut sinks: Vec<Box<dyn ReporterOutputSink>>,
    sleep: F,
) where
    F: Fn() -> Fut,
    Fut: core::future::Future<Output = ()>,
{
    let mut tick: u64 = 0;
    loop {
        sleep().await;
        if !inner.is_open() {
            break;
        }
        let include_sparse = match &sparse_mode {
            SparseMode::Never => false,
            SparseMode::Always => true,
            SparseMode::Once => tick == 0,
            SparseMode::Every(n) => tick % n == 0,
        };
        report_once(&inner, include_sparse, prefix.as_deref(), &mut sinks);
        tick += 1;
    }
}

// ── GaugedQueueReceiver ─────────────────────────────────────────────────────────────

pub struct GaugedQueueReceiver<T, R> {
    inner: R,
    queue: crate::intrusive::Queue<T>,
    gauge: QueueReceiver,
}

impl<T, R> GaugedQueueReceiver<T, R> {
    pub fn new(inner: R, gauge: QueueReceiver) -> Self {
        Self {
            inner,
            queue: Default::default(),
            gauge,
        }
    }
}

impl<T, R> crate::socket::channel::Receiver<crate::intrusive::Entry<T>>
    for GaugedQueueReceiver<T, R>
where
    R: crate::socket::channel::Receiver<crate::intrusive::Queue<T>>,
{
    fn poll_recv(
        &mut self,
        cx: &mut core::task::Context<'_>,
        budget: &mut crate::socket::channel::Budget,
    ) -> core::task::Poll<Option<crate::intrusive::Entry<T>>> {
        loop {
            if budget.is_exhausted() {
                if !self.queue.is_empty() {
                    budget.set_needs_wake();
                }
                return core::task::Poll::Pending;
            }

            if let Some(entry) = self.queue.pop_front() {
                self.gauge.on_receive(1);
                budget.consume();
                return core::task::Poll::Ready(Some(entry));
            }

            match self.inner.poll_recv(cx, budget) {
                core::task::Poll::Ready(Some(queue)) => {
                    if queue.is_empty() {
                        budget.set_needs_wake();
                        return core::task::Poll::Pending;
                    }
                    self.queue = queue;
                }
                core::task::Poll::Ready(None) => return core::task::Poll::Ready(None),
                core::task::Poll::Pending => return core::task::Poll::Pending,
            }
        }
    }

    fn on_consumed(&mut self, bytes: u64) {
        self.inner.on_consumed(bytes);
    }
}

// ── GaugedSender ────────────────────────────────────────────────────────────

pub struct GaugedSender<S> {
    inner: S,
    gauge: QueueSender,
}

impl<S> GaugedSender<S> {
    pub fn new(inner: S, gauge: QueueSender) -> Self {
        Self { inner, gauge }
    }
}

impl<S: Clone> Clone for GaugedSender<S> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            gauge: self.gauge.clone(),
        }
    }
}

impl<T, S> crate::socket::channel::UnboundedSender<T> for GaugedSender<S>
where
    S: crate::socket::channel::UnboundedSender<T>,
{
    fn send(&mut self, value: T) -> Result<(), T> {
        match self.inner.send(value) {
            Ok(()) => {
                self.gauge.on_send(1);
                Ok(())
            }
            Err(v) => Err(v),
        }
    }
}

// ── GaugedReceiver ──────────────────────────────────────────────────────────

pub struct GaugedReceiver<R> {
    inner: R,
    gauge: QueueReceiver,
}

impl<R> GaugedReceiver<R> {
    pub fn new(inner: R, gauge: QueueReceiver) -> Self {
        Self { inner, gauge }
    }
}

impl<T, R> crate::socket::channel::Receiver<T> for GaugedReceiver<R>
where
    R: crate::socket::channel::Receiver<T>,
{
    fn poll_recv(
        &mut self,
        cx: &mut core::task::Context<'_>,
        budget: &mut crate::socket::channel::Budget,
    ) -> core::task::Poll<Option<T>> {
        match self.inner.poll_recv(cx, budget) {
            core::task::Poll::Ready(Some(v)) => {
                self.gauge.on_receive(1);
                core::task::Poll::Ready(Some(v))
            }
            other => other,
        }
    }

    fn on_consumed(&mut self, bytes: u64) {
        self.inner.on_consumed(bytes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockSink {
        id: &'static str,
        fail: bool,
        calls: std::sync::Arc<Mutex<Vec<String>>>,
    }

    impl ReporterOutputSink for MockSink {
        fn emit(
            &mut self,
            payload: &ReportingPayload<'_>,
            _prefix: Option<&str>,
        ) -> Result<(), String> {
            self.calls.lock().unwrap().push(format!(
                "{}:{}:{}",
                self.id,
                payload.raw_samples.len(),
                payload.parsed.format_pretty()
            ));
            if self.fail {
                return Err(format!("sink {} failed", self.id));
            }
            Ok(())
        }
    }

    #[test]
    fn report_once_fans_out_single_destructive_take() {
        let registry = s2n_quic_dc_metrics::Registry::new();
        let counter = registry.register_counter("rx.data".into(), None);
        counter.increment(7);

        let seen = std::sync::Arc::new(Mutex::new(Vec::new()));
        let mut sinks: Vec<Box<dyn ReporterOutputSink>> = vec![
            Box::new(MockSink {
                id: "a",
                fail: false,
                calls: seen.clone(),
            }),
            Box::new(MockSink {
                id: "b",
                fail: false,
                calls: seen.clone(),
            }),
        ];

        report_once(&registry, false, None, &mut sinks);
        let entries = seen.lock().unwrap().clone();
        assert_eq!(entries.len(), 2);
        assert!(entries[0].contains("rx.data=7"));
        assert!(entries[1].contains("rx.data=7"));

        seen.lock().unwrap().clear();
        report_once(&registry, false, None, &mut sinks);
        let entries = seen.lock().unwrap().clone();
        assert_eq!(entries.len(), 2);
        assert!(entries[0].ends_with(":"));
        assert!(entries[1].ends_with(":"));
    }

    #[test]
    fn dispatch_continues_after_sink_error_in_order() {
        let payload = ReportingPayload::from_line("rx.data=1");
        let seen = std::sync::Arc::new(Mutex::new(Vec::new()));
        let mut sinks: Vec<Box<dyn ReporterOutputSink>> = vec![
            Box::new(MockSink {
                id: "first",
                fail: true,
                calls: seen.clone(),
            }),
            Box::new(MockSink {
                id: "second",
                fail: false,
                calls: seen.clone(),
            }),
        ];

        dispatch_payload_to_sinks(&mut sinks, &payload, None);
        assert_eq!(
            seen.lock().unwrap().as_slice(),
            &[
                "first:1:rx.data=1".to_string(),
                "second:1:rx.data=1".to_string()
            ]
        );
    }

    #[test]
    fn statsd_encoding_covers_counter_gauge_nominal_and_histogram_decisions() {
        let line = "rx.data=255470,q.packet.depth=875,rx.ecn=500 ect0,task.time=5*2+10*1 us packet_dispatch.0";
        let payload = ReportingPayload::from_line(line);
        let lines = encode_statsd_lines(&payload.raw_samples, Some("svc"));

        assert!(lines.contains(&"svc.rx.data:255470|c".to_string()));
        assert!(lines.contains(&"svc.q.packet.depth:875|g".to_string()));
        assert!(lines.contains(&"svc.q.packet.depth.distribution:875|ms".to_string()));
        assert!(lines.contains(&"svc.rx.ecn.ect0:500|c".to_string()));
        assert!(lines.contains(&"svc.task.time.packet_dispatch.0.count:3|c".to_string()));
        assert!(lines.contains(&"svc.task.time.packet_dispatch.0.min:0.005|ms".to_string()));
        assert!(lines.contains(&"svc.task.time.packet_dispatch.0.p50:0.005|ms".to_string()));
        assert!(lines.contains(&"svc.task.time.packet_dispatch.0.p90:0.010|ms".to_string()));
        assert!(lines.contains(&"svc.task.time.packet_dispatch.0.p95:0.010|ms".to_string()));
        assert!(lines.contains(&"svc.task.time.packet_dispatch.0.p99:0.010|ms".to_string()));
        assert!(lines.contains(&"svc.task.time.packet_dispatch.0.max:0.010|ms".to_string()));
    }

    #[test]
    fn statsd_encoding_byte_unit_not_appended_to_name() {
        let payload =
            ReportingPayload::from_line("socket.rx.bytes=273390965 B,socket.tx.bytes=272721617 B");
        let lines = encode_statsd_lines(&payload.raw_samples, Some("svc"));

        assert!(lines.contains(&"svc.socket.rx.bytes:273390965|c".to_string()));
        assert!(lines.contains(&"svc.socket.tx.bytes:272721617|c".to_string()));
    }

    #[test]
    fn statsd_sink_submits_payloads_as_single_batch() {
        let (tx, mut rx) = mpsc::channel(1);
        let config = StatsdUdpConfig::new("127.0.0.1:8125".parse().unwrap(), tx);
        let mut sink = StatsdUdpSink::new(config);
        let payload = ReportingPayload::from_line(
            "rx.data=1,q.packet.depth=2,task.time=5*2+10*1 us dispatch",
        );

        sink.emit(&payload, Some("svc")).unwrap();
        let batch = rx.try_recv().unwrap();
        assert_eq!(batch.addr, "127.0.0.1:8125".parse().unwrap());
        assert!(!batch.payloads.is_empty());
    }

    #[test]
    fn statsd_sink_drops_batch_when_queue_full() {
        let (tx, mut rx) = mpsc::channel(1);
        let config = StatsdUdpConfig::new("127.0.0.1:8125".parse().unwrap(), tx);
        let mut sink = StatsdUdpSink::new(config);
        let payload = ReportingPayload::from_line("rx.data=1");

        sink.emit(&payload, None).unwrap();
        sink.emit(&payload, None).unwrap();

        // only the first batch is retained in the bounded queue
        assert!(rx.try_recv().is_ok());
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn statsd_chunking_honors_boundaries_and_drops_oversized_lines() {
        let lines = vec![
            "a:1|c".to_string(),
            "b:22|c".to_string(),
            "this.metric.is.way.too.long:1|c".to_string(),
            "c:3|c".to_string(),
        ];
        let (payloads, dropped) = chunk_statsd_lines(&lines, 12);

        assert_eq!(dropped, 1);
        assert_eq!(payloads.len(), 2);
        assert_eq!(payloads[0], b"a:1|c\nb:22|c".to_vec());
        assert_eq!(payloads[1], b"c:3|c".to_vec());
    }

    #[test]
    fn chunking_drops_all_when_payload_size_is_zero() {
        let lines = vec!["a:1|c".to_string(), "b:1|c".to_string()];
        let (payloads, dropped) = chunk_statsd_lines(&lines, 0);
        assert!(payloads.is_empty());
        assert_eq!(dropped, 2);
    }

    #[test]
    fn single_metric_take_fans_out_to_multiple_sinks() {
        let registry = s2n_quic_dc_metrics::Registry::new();
        let counter = registry.register_counter("rx.data".into(), None);
        counter.increment(1);

        let seen = std::sync::Arc::new(Mutex::new(Vec::new()));
        let mut sinks: Vec<Box<dyn ReporterOutputSink>> = vec![
            Box::new(MockSink {
                id: "1",
                fail: false,
                calls: seen.clone(),
            }),
            Box::new(MockSink {
                id: "2",
                fail: false,
                calls: seen.clone(),
            }),
            Box::new(MockSink {
                id: "3",
                fail: false,
                calls: seen.clone(),
            }),
        ];

        if let Some(line) = registry.try_take_current_metrics_line_sparse(false) {
            let payload = ReportingPayload::from_line(&line);
            dispatch_payload_to_sinks(&mut sinks, &payload, None);
        }

        assert_eq!(seen.lock().unwrap().len(), 3);
    }
}

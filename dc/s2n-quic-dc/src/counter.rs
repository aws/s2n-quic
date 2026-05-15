// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::time::Duration;
use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{
        atomic::{AtomicI64, Ordering},
        Arc, Mutex,
    },
};
use tokio::sync::mpsc;

pub use s2n_quic_dc_metrics::{Summary, Unit};
use std::time::Instant;

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
pub struct ReporterConfig {
    pub interval: Duration,
    pub prefix: Option<String>,
    pub include_sparse: bool,
    pub sinks: Vec<ReporterSink>,
}

impl ReporterConfig {
    pub fn new(interval: Duration) -> Self {
        Self {
            interval,
            prefix: None,
            include_sparse: false,
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
    parsed: ParsedMetricsLine<'a>,
    raw_samples: Vec<RawMetricSample<'a>>,
}

impl<'a> ReportingPayload<'a> {
    fn from_line(line: &'a str) -> Self {
        Self {
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
        if payload.parsed.is_empty() {
            tracing::info!("<no metrics>");
            return Ok(());
        }

        let formatted = payload.parsed.format_pretty();
        if let Some(prefix) = prefix.filter(|p| !p.is_empty()) {
            tracing::info!("[{prefix}] {formatted}");
        } else {
            tracing::info!("{formatted}");
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

fn parse_byte_value(value: &str) -> Option<u64> {
    let (number, unit) = value.split_once(' ')?;
    if unit.trim() == "B" {
        number.parse().ok()
    } else {
        None
    }
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
pub struct Counter(s2n_quic_dc_metrics::Counter);

impl Counter {
    #[inline]
    pub fn add(&self, v: u64) {
        self.0.increment(v);
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
pub struct Gauge(Arc<AtomicI64>);

impl Gauge {
    #[inline]
    pub fn add(&self, v: i64) -> i64 {
        self.0.fetch_add(v, Ordering::Relaxed) + v
    }

    #[inline]
    pub fn sub(&self, v: i64) -> i64 {
        self.0.fetch_sub(v, Ordering::Relaxed) - v
    }

    #[inline]
    pub fn get(&self) -> i64 {
        self.0.load(Ordering::Relaxed)
    }
}

// ── Timer ───────────────────────────────────────────────────────────────────

/// A histogram that records elapsed durations via a guard pattern.
///
/// Call `timer.start()` to begin timing; the returned guard records
/// the elapsed duration into the underlying `Summary` on drop.
#[derive(Clone)]
pub struct Timer(Summary);

impl Timer {
    #[inline]
    pub fn start(&self) -> TimerGuard {
        TimerGuard {
            summary: &self.0,
            start: Instant::now(),
        }
    }

    #[inline]
    pub fn record(&self, duration: Duration) {
        self.0.record_duration(duration);
    }
}

pub struct TimerGuard<'a> {
    summary: &'a Summary,
    start: Instant,
}

impl Drop for TimerGuard<'_> {
    #[inline]
    fn drop(&mut self) {
        self.summary.record_duration(self.start.elapsed());
    }
}

// ── QueueGauge ──────────────────────────────────────────────────────────────
// TODO: add per-item sojourn time tracking (enqueue→dequeue latency as a histogram)

#[derive(Clone)]
pub struct QueueGauge {
    pub throughput: Counter,
    pub drain: Counter,
    pub depth: Gauge,
    pub depth_distribution: Summary,
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

// ── Registry ────────────────────────────────────────────────────────────────

#[derive(Clone, Default)]
pub struct Registry {
    inner: s2n_quic_dc_metrics::Registry,
    queue_gauges: Arc<Mutex<HashMap<String, QueueGauge>>>,
}

impl Registry {
    pub fn new() -> Self {
        Self {
            inner: s2n_quic_dc_metrics::Registry::new(),
            queue_gauges: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn register(&self, label: impl core::fmt::Display) -> Counter {
        Counter(self.inner.register_counter(label.to_string(), None))
    }

    pub fn register_bytes(&self, label: impl core::fmt::Display) -> Counter {
        Counter(
            self.inner
                .register_counter(label.to_string(), Some("B".into())),
        )
    }

    pub fn register_nominal(
        &self,
        label: impl core::fmt::Display,
        variant: impl core::fmt::Display,
    ) -> Counter {
        Counter(
            self.inner
                .register_counter(label.to_string(), Some(variant.to_string())),
        )
    }

    pub fn register_queue_gauge(&self, label: impl core::fmt::Display) -> QueueGauge {
        let label = label.to_string();
        let mut gauges = self.queue_gauges.lock().unwrap();
        if let Some(existing) = gauges.get(&label) {
            return existing.clone();
        }

        let throughput = Counter(self.inner.register_counter(format!("{label}.enq"), None));
        let drain = Counter(self.inner.register_counter(format!("{label}.drain"), None));
        let depth = Arc::new(AtomicI64::new(0));
        let depth_clone = depth.clone();
        self.inner
            .register_list_callback(format!("{label}.depth"), None, Unit::Count, move || {
                NonZeroDisplay(depth_clone.load(Ordering::Relaxed))
            });
        let depth_distribution =
            self.inner
                .register_summary(format!("{label}.depth_dist"), None, Unit::Count);

        let gauge = QueueGauge {
            throughput,
            drain,
            depth: Gauge(depth),
            depth_distribution,
        };
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

        let var = Some(variant);
        let throughput = Counter(
            self.inner
                .register_counter(format!("{label}.enq"), var.clone()),
        );
        let drain = Counter(
            self.inner
                .register_counter(format!("{label}.drain"), var.clone()),
        );
        let depth = Arc::new(AtomicI64::new(0));
        let depth_clone = depth.clone();
        self.inner.register_list_callback(
            format!("{label}.depth"),
            var.clone(),
            Unit::Count,
            move || NonZeroDisplay(depth_clone.load(Ordering::Relaxed)),
        );
        let depth_distribution =
            self.inner
                .register_summary(format!("{label}.depth_dist"), var, Unit::Count);

        let gauge = QueueGauge {
            throughput,
            drain,
            depth: Gauge(depth),
            depth_distribution,
        };
        gauges.insert(key, gauge.clone());
        gauge
    }

    pub fn register_gauge(&self, label: impl core::fmt::Display) -> Gauge {
        let inner = Arc::new(AtomicI64::new(0));
        let inner_clone = inner.clone();
        self.inner
            .register_list_callback(label.to_string(), None, Unit::Count, move || {
                NonZeroDisplay(inner_clone.load(Ordering::Relaxed))
            });
        Gauge(inner)
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
        Timer(
            self.inner
                .register_summary(label.to_string(), None, Unit::Microsecond),
        )
    }

    pub fn register_nominal_timer(
        &self,
        label: impl core::fmt::Display,
        variant: impl core::fmt::Display,
    ) -> Timer {
        Timer(self.inner.register_summary(
            label.to_string(),
            Some(variant.to_string()),
            Unit::Microsecond,
        ))
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
        let include_sparse = config.include_sparse;
        let sinks = build_sinks(&config.sinks);

        #[cfg(any(test, feature = "testing"))]
        if bach::is_active() {
            bach::spawn(report_loop(
                inner,
                include_sparse,
                prefix,
                sinks,
                move || bach::time::sleep(interval),
            ));
            return;
        }

        tokio::spawn(report_loop(
            inner,
            include_sparse,
            prefix,
            sinks,
            move || tokio::time::sleep(interval),
        ));
    }
}

async fn report_loop<F, Fut>(
    inner: s2n_quic_dc_metrics::Registry,
    include_sparse: bool,
    prefix: Option<String>,
    mut sinks: Vec<Box<dyn ReporterOutputSink>>,
    sleep: F,
) where
    F: Fn() -> Fut,
    Fut: core::future::Future<Output = ()>,
{
    loop {
        sleep().await;
        if !inner.is_open() {
            break;
        }
        report_once(&inner, include_sparse, prefix.as_deref(), &mut sinks);
    }
}

// ── GaugedQueueReceiver ─────────────────────────────────────────────────────────────

pub struct GaugedQueueReceiver<T, R> {
    inner: R,
    queue: crate::intrusive_queue::Queue<T>,
    gauge: QueueGauge,
}

impl<T, R> GaugedQueueReceiver<T, R> {
    pub fn new(inner: R, gauge: QueueGauge) -> Self {
        Self {
            inner,
            queue: Default::default(),
            gauge,
        }
    }
}

impl<T, R> crate::socket::channel::Receiver<crate::intrusive_queue::Entry<T>>
    for GaugedQueueReceiver<T, R>
where
    R: crate::socket::channel::Receiver<crate::intrusive_queue::Queue<T>>,
{
    fn poll_recv(
        &mut self,
        cx: &mut core::task::Context<'_>,
        budget: &mut crate::socket::channel::Budget,
    ) -> core::task::Poll<Option<crate::intrusive_queue::Entry<T>>> {
        loop {
            if budget.is_exhausted() {
                if !self.queue.is_empty() {
                    budget.set_needs_wake();
                }
                return core::task::Poll::Pending;
            }

            if let Some(entry) = self.queue.pop_front() {
                self.gauge.dequeue();
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
    gauge: QueueGauge,
}

impl<S> GaugedSender<S> {
    pub fn new(inner: S, gauge: QueueGauge) -> Self {
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
                self.gauge.enqueue(1);
                Ok(())
            }
            Err(v) => Err(v),
        }
    }
}

// ── GaugedReceiver ──────────────────────────────────────────────────────────

pub struct GaugedReceiver<R> {
    inner: R,
    gauge: QueueGauge,
}

impl<R> GaugedReceiver<R> {
    pub fn new(inner: R, gauge: QueueGauge) -> Self {
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
                self.gauge.dequeue();
                core::task::Poll::Ready(Some(v))
            }
            other => other,
        }
    }

    fn on_consumed(&mut self, bytes: u64) {
        self.inner.on_consumed(bytes);
    }
}

// ── Metrics line formatting ─────────────────────────────────────────────────

#[derive(Debug, Default, PartialEq, Eq)]
struct ParsedMetricsLine<'a> {
    entries: Vec<MetricEntry<'a>>,
}

impl<'a> ParsedMetricsLine<'a> {
    fn parse(line: &'a str) -> Self {
        use std::collections::{BTreeMap, BTreeSet, HashSet};

        let mut metrics: BTreeMap<&'a str, &'a str> = BTreeMap::new();
        let mut nominals: BTreeMap<&'a str, Vec<NominalVariant<'a>>> = BTreeMap::new();
        let mut variant_histograms: BTreeMap<&'a str, Vec<HistogramVariant<'a>>> = BTreeMap::new();

        for part in line.split(',') {
            let Some((key, value)) = part.split_once('=') else {
                continue;
            };

            if let Some((val, rest)) = value.split_once(' ') {
                if val.bytes().all(|b| b.is_ascii_digit()) {
                    if is_valid_scalar_unit(rest) {
                        metrics.insert(key, value);
                        continue;
                    }
                    nominals.entry(key).or_default().push(NominalVariant {
                        label: rest,
                        value: val,
                    });
                    continue;
                }

                if val.contains('*') && !is_histogram_unit_only(rest) {
                    variant_histograms
                        .entry(key)
                        .or_default()
                        .push(HistogramVariant::parse(value));
                    continue;
                }
            }

            metrics.insert(key, value);
        }

        let mut queue_bases = BTreeSet::new();
        let mut hit_miss_bases = BTreeSet::new();
        for key in metrics.keys() {
            if let Some(base) = key.strip_suffix(".enq") {
                queue_bases.insert(base);
            } else if let Some(base) = key.strip_suffix(".drain") {
                queue_bases.insert(base);
            } else if let Some(base) = key.strip_suffix(".depth") {
                queue_bases.insert(base);
            } else if let Some(base) = key.strip_suffix(".hit") {
                hit_miss_bases.insert(base);
            } else if let Some(base) = key.strip_suffix(".miss") {
                hit_miss_bases.insert(base);
            }
        }

        let mut entries = Vec::new();
        let mut consumed: HashSet<&'a str> = HashSet::new();

        for (&key, &value) in &metrics {
            if consumed.contains(key) {
                continue;
            }

            let queue_base = key
                .strip_suffix(".enq")
                .or_else(|| key.strip_suffix(".drain"))
                .or_else(|| key.strip_suffix(".depth"));

            if let Some(base) = queue_base.filter(|base| queue_bases.contains(base)) {
                let enq_key = format!("{base}.enq");
                let drain_key = format!("{base}.drain");
                let depth_key = format!("{base}.depth");
                if let Some((&enq_key, _)) = metrics.get_key_value(enq_key.as_str()) {
                    consumed.insert(enq_key);
                }
                if let Some((&drain_key, _)) = metrics.get_key_value(drain_key.as_str()) {
                    consumed.insert(drain_key);
                }
                if let Some((&depth_key, _)) = metrics.get_key_value(depth_key.as_str()) {
                    consumed.insert(depth_key);
                }

                entries.push(MetricEntry::QueueGauge(QueueGaugeMetric {
                    name: base,
                    enq: metrics.get(enq_key.as_str()).copied().unwrap_or("0"),
                    drain: metrics.get(drain_key.as_str()).copied().unwrap_or("0"),
                    depth: metrics
                        .get(depth_key.as_str())
                        .copied()
                        .filter(|depth| *depth != "0"),
                }));
                continue;
            }

            let hit_miss_base = key
                .strip_suffix(".hit")
                .or_else(|| key.strip_suffix(".miss"));

            if let Some(base) = hit_miss_base.filter(|base| hit_miss_bases.contains(base)) {
                let hit_key = format!("{base}.hit");
                let miss_key = format!("{base}.miss");
                if let Some((&hit_key, _)) = metrics.get_key_value(hit_key.as_str()) {
                    consumed.insert(hit_key);
                }
                if let Some((&miss_key, _)) = metrics.get_key_value(miss_key.as_str()) {
                    consumed.insert(miss_key);
                }

                entries.push(MetricEntry::HitMiss(HitMissMetric {
                    name: base,
                    hit: metrics.get(hit_key.as_str()).copied().unwrap_or("0"),
                    miss: metrics.get(miss_key.as_str()).copied().unwrap_or("0"),
                }));
                continue;
            }

            consumed.insert(key);

            if let Some(name) = key.strip_suffix(":bytes") {
                let bytes = value
                    .parse::<u64>()
                    .ok()
                    .or_else(|| parse_byte_value(value));
                if let Some(bytes) = bytes {
                    if bytes == 0 {
                        continue;
                    }

                    entries.push(MetricEntry::Throughput(ThroughputMetric { name, bytes }));
                }

                continue;
            }

            if let Some(bytes) = parse_byte_value(value) {
                if bytes == 0 {
                    continue;
                }

                entries.push(MetricEntry::Throughput(ThroughputMetric {
                    name: key,
                    bytes,
                }));
                continue;
            }

            if value.contains('*') {
                entries.push(MetricEntry::Histogram(HistogramMetric {
                    name: key,
                    histogram: Histogram::parse(value),
                }));
                continue;
            }

            entries.push(MetricEntry::Scalar(ScalarMetric { name: key, value }));
        }

        for (key, variants) in nominals {
            entries.push(MetricEntry::Nominal(NominalMetric {
                name: key,
                variants,
            }));
        }

        for (key, variants) in variant_histograms {
            entries.push(MetricEntry::VariantHistograms(VariantHistogramMetric {
                name: key,
                variants,
            }));
        }

        Self { entries }
    }

    fn format_pretty(&self) -> String {
        let mut output = String::new();

        for entry in &self.entries {
            entry.write_to(&mut output);
        }

        output
    }

    fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[derive(Debug, PartialEq, Eq)]
enum MetricEntry<'a> {
    QueueGauge(QueueGaugeMetric<'a>),
    HitMiss(HitMissMetric<'a>),
    Throughput(ThroughputMetric<'a>),
    Histogram(HistogramMetric<'a>),
    Scalar(ScalarMetric<'a>),
    Nominal(NominalMetric<'a>),
    VariantHistograms(VariantHistogramMetric<'a>),
}

impl MetricEntry<'_> {
    fn write_to(&self, output: &mut String) {
        use std::fmt::Write;

        if !output.is_empty() {
            output.push(' ');
        }

        match self {
            Self::QueueGauge(metric) => match &metric.depth {
                Some(depth) => {
                    write!(
                        output,
                        "{}={}/{}({depth})",
                        metric.name, metric.enq, metric.drain
                    )
                    .unwrap();
                }
                None => write!(output, "{}={}/{}", metric.name, metric.enq, metric.drain).unwrap(),
            },
            Self::HitMiss(metric) => {
                write!(output, "{}={}/{}", metric.name, metric.hit, metric.miss).unwrap();
            }
            Self::Throughput(metric) => {
                let (rate, prefix) = format_bits_per_second(metric.bytes);
                write!(output, "{}={rate:.2}{prefix}bps", metric.name).unwrap();
            }
            Self::Histogram(metric) => metric.histogram.write_summary(&metric.name, output),
            Self::Scalar(metric) => write!(output, "{}={}", metric.name, metric.value).unwrap(),
            Self::Nominal(metric) => {
                write!(output, "{}(", metric.name).unwrap();
                let mut first = true;
                for variant in &metric.variants {
                    if !first {
                        output.push(' ');
                    }
                    first = false;
                    write!(output, "{}={}", variant.label, variant.value).unwrap();
                }
                output.push(')');
            }
            Self::VariantHistograms(metric) => {
                write!(output, "{}(", metric.name).unwrap();
                let mut first = true;
                for variant in &metric.variants {
                    if !first {
                        output.push(' ');
                    }
                    first = false;
                    variant.write_to(output);
                }
                output.push(')');
            }
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
struct QueueGaugeMetric<'a> {
    name: &'a str,
    enq: &'a str,
    drain: &'a str,
    depth: Option<&'a str>,
}

#[derive(Debug, PartialEq, Eq)]
struct HitMissMetric<'a> {
    name: &'a str,
    hit: &'a str,
    miss: &'a str,
}

#[derive(Debug, PartialEq, Eq)]
struct ThroughputMetric<'a> {
    name: &'a str,
    bytes: u64,
}

#[derive(Debug, PartialEq, Eq)]
struct HistogramMetric<'a> {
    name: &'a str,
    histogram: Histogram<'a>,
}

#[derive(Debug, PartialEq, Eq)]
struct ScalarMetric<'a> {
    name: &'a str,
    value: &'a str,
}

#[derive(Debug, PartialEq, Eq)]
struct NominalMetric<'a> {
    name: &'a str,
    variants: Vec<NominalVariant<'a>>,
}

#[derive(Debug, PartialEq, Eq)]
struct NominalVariant<'a> {
    label: &'a str,
    value: &'a str,
}

#[derive(Debug, PartialEq, Eq)]
struct VariantHistogramMetric<'a> {
    name: &'a str,
    variants: Vec<HistogramVariant<'a>>,
}

#[derive(Debug, PartialEq, Eq)]
struct HistogramVariant<'a> {
    label: &'a str,
    histogram: Histogram<'a>,
}

impl<'a> HistogramVariant<'a> {
    fn parse(value: &'a str) -> Self {
        let (data, unit, variant) = parse_histogram_suffix(value);

        Self {
            label: if variant.is_empty() { "?" } else { variant },
            histogram: Histogram::parse_parts(data, unit),
        }
    }

    fn write_to(&self, output: &mut String) {
        self.histogram.write_variant(&self.label, output);
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Histogram<'a> {
    buckets: Vec<HistogramBucket>,
    unit: &'a str,
}

impl<'a> Histogram<'a> {
    fn parse(value: &'a str) -> Self {
        let (data, unit, _variant) = parse_histogram_suffix(value);
        Self::parse_parts(data, unit)
    }

    fn parse_parts(data: &'a str, unit: &'a str) -> Self {
        Self {
            buckets: parse_histogram_buckets(data),
            unit,
        }
    }

    fn summarize(&self) -> (u64, u64, u64, u64) {
        compute_histogram_summary(&self.buckets)
    }

    fn write_summary(&self, key: &str, output: &mut String) {
        use std::fmt::Write;

        let (total_count, p50, p99, max) = self.summarize();
        if total_count == 0 {
            write!(output, "{key}=0").unwrap();
            return;
        }

        if self.unit == "us" {
            write!(
                output,
                "{key}(n={total_count} p50={} p99={} max={})",
                format_duration_us(p50),
                format_duration_us(p99),
                format_duration_us(max),
            )
            .unwrap();
            return;
        }

        write!(
            output,
            "{key}(n={total_count} p50={p50} p99={p99} max={max}"
        )
        .unwrap();
        if !self.unit.is_empty() {
            write!(output, " {}", self.unit).unwrap();
        }
        output.push(')');
    }

    fn write_variant(&self, label: &str, output: &mut String) {
        use std::fmt::Write;

        let (total_count, p50, p99, max) = self.summarize();
        if total_count == 0 {
            write!(output, "{label}=0").unwrap();
            return;
        }

        if self.unit == "us" {
            write!(
                output,
                "{label}=(n={total_count} p50={} p99={} max={})",
                format_duration_us(p50),
                format_duration_us(p99),
                format_duration_us(max),
            )
            .unwrap();
            return;
        }

        write!(
            output,
            "{label}=(n={total_count} p50={p50} p99={p99} max={max}"
        )
        .unwrap();
        if !self.unit.is_empty() {
            write!(output, " {}", self.unit).unwrap();
        }
        output.push(')');
    }
}

#[derive(Debug, PartialEq, Eq)]
struct HistogramBucket {
    value: u64,
    count: u64,
}

/// Returns true if `rest` (the part after the first space in a histogram value)
/// is a recognized unit suffix only (e.g. "us", "B"), as opposed to containing a
/// variant name.
fn is_histogram_unit_only(rest: &str) -> bool {
    matches!(rest, "us" | "ms" | "s" | "B" | "KB" | "MB" | "GB")
}

fn is_valid_scalar_unit(rest: &str) -> bool {
    is_histogram_unit_only(rest) || rest == "%"
}

fn compute_histogram_summary(buckets: &[HistogramBucket]) -> (u64, u64, u64, u64) {
    let total_count = buckets.iter().map(|bucket| bucket.count).sum();
    if total_count == 0 {
        return (0, 0, 0, 0);
    }

    let p50 = histogram_value_at_percentile(buckets, 50);
    let p99 = histogram_value_at_percentile(buckets, 99);
    let max = buckets.last().map_or(0, |bucket| bucket.value);

    (total_count, p50, p99, max)
}

/// Returns `(total_count, min_value, max_value)` for histogram buckets.
///
/// For empty buckets, all values are `0`.
fn histogram_count_min_max(buckets: &[HistogramBucket]) -> (u64, u64, u64) {
    let count = buckets.iter().map(|bucket| bucket.count).sum();
    let min = buckets.first().map_or(0, |bucket| bucket.value);
    let max = buckets.last().map_or(0, |bucket| bucket.value);
    (count, min, max)
}

/// Returns the value at the requested percentile for histogram buckets.
///
/// `percentile` is expected in the range `0..=100`. Empty buckets return `0`.
fn histogram_value_at_percentile(buckets: &[HistogramBucket], percentile: u32) -> u64 {
    let total_count: u64 = buckets.iter().map(|bucket| bucket.count).sum();
    if total_count == 0 {
        return 0;
    }

    let target = ((total_count as f64) * (percentile as f64 / 100.0)).ceil() as u64;
    let mut cumulative = 0u64;

    for bucket in buckets {
        cumulative += bucket.count;
        if cumulative >= target {
            return bucket.value;
        }
    }

    buckets.last().map_or(0, |bucket| bucket.value)
}

fn parse_histogram_buckets(data: &str) -> Vec<HistogramBucket> {
    let mut buckets = Vec::new();

    for entry in data.split('+') {
        if let Some((value, count)) = entry.split_once('*') {
            if let (Ok(value), Ok(count)) = (value.parse::<u64>(), count.parse::<u64>()) {
                buckets.push(HistogramBucket { value, count });
            }
        }
    }

    buckets
}

/// Splits a histogram value string into (data, unit, variant).
fn parse_histogram_suffix(value: &str) -> (&str, &str, &str) {
    // Find the first space — everything before it might be histogram data
    let Some(first_space) = value.find(' ') else {
        return (value, "", "");
    };

    let data = &value[..first_space];
    let rest = value[first_space + 1..].trim();

    // rest could be: "us", "B", "packet_dispatch.0", "us packet_dispatch.0"
    if let Some((first_word, remainder)) = rest.split_once(' ') {
        if is_histogram_unit_only(first_word) {
            // "us packet_dispatch.0"
            return (data, first_word, remainder.trim());
        }
        // Shouldn't happen in practice, but treat everything as variant
        (data, "", rest)
    } else if is_histogram_unit_only(rest) {
        (data, rest, "")
    } else {
        (data, "", rest)
    }
}

fn format_duration_us(us: u64) -> String {
    if us >= 1_000_000 {
        format!("{:.2}s", us as f64 / 1_000_000.0)
    } else if us >= 1_000 {
        format!("{:.2}ms", us as f64 / 1_000.0)
    } else {
        format!("{us}us")
    }
}

fn format_bits_per_second(bytes: u64) -> (f64, &'static str) {
    let mut rate = bytes as f64 * 8.0;
    let prefixes = [("G", 1e9), ("M", 1e6), ("K", 1e3)];
    let mut prefix = "";

    for (candidate, divisor) in prefixes {
        if rate >= divisor {
            rate /= divisor;
            prefix = candidate;
            break;
        }
    }

    (rate, prefix)
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
    fn parse_structures_formatted_metrics() {
        let parsed = ParsedMetricsLine::parse(
            "q.packet.drain=45662,q.packet.depth=875,q.packet.enq=46180,rx.ecn=500 ect0,rx.ecn=3 ect1",
        );

        assert_eq!(
            parsed,
            ParsedMetricsLine {
                entries: vec![
                    MetricEntry::QueueGauge(QueueGaugeMetric {
                        name: "q.packet",
                        enq: "46180",
                        drain: "45662",
                        depth: Some("875"),
                    }),
                    MetricEntry::Nominal(NominalMetric {
                        name: "rx.ecn",
                        variants: vec![
                            NominalVariant {
                                label: "ect0",
                                value: "500",
                            },
                            NominalVariant {
                                label: "ect1",
                                value: "3",
                            },
                        ],
                    }),
                ],
            }
        );
    }

    #[test]
    fn parse_histogram_variants_into_structured_data() {
        let parsed = ParsedMetricsLine::parse(
            "task.time=5*2+10*1 us packet_dispatch.0,task.time=7*3 us packet_dispatch.1",
        );

        assert_eq!(
            parsed,
            ParsedMetricsLine {
                entries: vec![MetricEntry::VariantHistograms(VariantHistogramMetric {
                    name: "task.time",
                    variants: vec![
                        HistogramVariant {
                            label: "packet_dispatch.0",
                            histogram: Histogram {
                                buckets: vec![
                                    HistogramBucket { value: 5, count: 2 },
                                    HistogramBucket {
                                        value: 10,
                                        count: 1
                                    },
                                ],
                                unit: "us",
                            },
                        },
                        HistogramVariant {
                            label: "packet_dispatch.1",
                            histogram: Histogram {
                                buckets: vec![HistogramBucket { value: 7, count: 3 }],
                                unit: "us",
                            },
                        },
                    ],
                })],
            }
        );
    }

    #[test]
    fn format_queue_gauge() {
        let line = "q.packet.drain=45662,q.packet.depth=875,q.packet.enq=46180";
        let result = ParsedMetricsLine::parse(line).format_pretty();
        assert_eq!(result, "q.packet=46180/45662(875)");
    }

    #[test]
    fn format_queue_gauge_no_depth() {
        let line = "q.packet.drain=100,q.packet.enq=100";
        let result = ParsedMetricsLine::parse(line).format_pretty();
        assert_eq!(result, "q.packet=100/100");
    }

    #[test]
    fn format_bytes_as_bps() {
        let line = "socket.rx.bytes=273390965 B";
        let result = ParsedMetricsLine::parse(line).format_pretty();
        assert_eq!(result, "socket.rx.bytes=2.19Gbps");
    }

    #[test]
    fn format_plain_counter() {
        let line = "rx.data=255470";
        let result = ParsedMetricsLine::parse(line).format_pretty();
        assert_eq!(result, "rx.data=255470");
    }

    #[test]
    fn format_mixed() {
        let line = "q.ack.drain=46005,q.ack.enq=46005,rx.data=255470,socket.tx.bytes=272721617 B";
        let result = ParsedMetricsLine::parse(line).format_pretty();
        assert_eq!(
            result,
            "q.ack=46005/46005 rx.data=255470 socket.tx.bytes=2.18Gbps"
        );
    }

    #[test]
    fn format_hit_miss() {
        let line = "rx.peer_cache.hit=80000,rx.peer_cache.miss=5";
        let result = ParsedMetricsLine::parse(line).format_pretty();
        assert_eq!(result, "rx.peer_cache=80000/5");
    }

    #[test]
    fn format_histogram_us() {
        let line = "rx.decrypt_time=0*4541+1*4552+1*4527+2*4617+5*378+13*45 us";
        let result = ParsedMetricsLine::parse(line).format_pretty();
        // total = 4541+4552+4527+4617+378+45 = 18660
        // p50 target = 9330, cumulative: 4541, 9093, 13620 → p50=1us
        // p99 target = 18474, cumulative: ...18237, 18615, 18660 → p99=5us
        assert_eq!(result, "rx.decrypt_time(n=18660 p50=1us p99=5us max=13us)");
    }

    #[test]
    fn format_histogram_count() {
        let line = "rx.frames_per_packet=4*5036+7*5079+15*6952+25*349";
        let result = ParsedMetricsLine::parse(line).format_pretty();
        // total = 5036+5079+6952+349 = 17416
        // p50 target = 8709 → cumulative 5036, 10115 → p50=7
        // p99 target = 17242 → cumulative 5036, 10115, 17067, 17416 → p99=25
        assert_eq!(result, "rx.frames_per_packet(n=17416 p50=7 p99=25 max=25)");
    }

    #[test]
    fn format_nominal() {
        let line = "rx.ecn=500 ect0,rx.ecn=3 ect1,rx.ecn=2 ce,rx.ecn=0 not_ect";
        let result = ParsedMetricsLine::parse(line).format_pretty();
        assert_eq!(result, "rx.ecn(ect0=500 ect1=3 ce=2 not_ect=0)");
    }

    #[test]
    fn format_variant_histogram_with_unit() {
        let line = "task.time=5*5000+10*3000+50*1500+200*500 us packet_dispatch.0,task.time=3*4000+20*3000+80*2000+300*1000 us packet_dispatch.1";
        let result = ParsedMetricsLine::parse(line).format_pretty();
        assert_eq!(
            result,
            "task.time(packet_dispatch.0=(n=10000 p50=5us p99=200us max=200us) packet_dispatch.1=(n=10000 p50=20us p99=300us max=300us))"
        );
    }

    #[test]
    fn format_variant_histogram_no_unit() {
        let line =
            "task.budget=1*8000+2*1500+4*300+10*200 packet_dispatch.0,task.budget=1*7000+2*2000+5*800+12*200 packet_dispatch.1";
        let result = ParsedMetricsLine::parse(line).format_pretty();
        assert_eq!(
            result,
            "task.budget(packet_dispatch.0=(n=10000 p50=1 p99=10 max=10) packet_dispatch.1=(n=10000 p50=1 p99=12 max=12))"
        );
    }

    #[test]
    fn format_variant_histogram_real_log() {
        let line = "task.budget=2*5321+3*208+4*1586+5*638+6*513+7*404+8*267+9*193+10*157+11*112+12*100+103*562 packet_dispatch.0,task.budget=2*6022+3*192+4*1576+5*544+6*514+7*413+8*309+9*236+10*157+11*138+12*115+147*840 packet_dispatch.1";
        let result = ParsedMetricsLine::parse(line).format_pretty();
        assert!(result.starts_with("task.budget(packet_dispatch.0=(n="));
        assert!(result.contains("packet_dispatch.1=(n="));
        assert!(result.ends_with(')'));
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

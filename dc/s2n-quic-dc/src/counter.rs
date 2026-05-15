// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::time::Duration;
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicI64, Ordering},
        Arc, Mutex,
    },
};

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
    pub fn add(&self, v: i64) {
        self.0.fetch_add(v, Ordering::Relaxed);
    }

    #[inline]
    pub fn sub(&self, v: i64) {
        self.0.fetch_sub(v, Ordering::Relaxed);
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

#[derive(Clone)]
pub struct QueueGauge {
    pub throughput: Counter,
    pub drain: Counter,
    pub depth: Gauge,
}

impl QueueGauge {
    #[inline]
    pub fn enqueue(&self, count: u64) {
        self.throughput.add(count);
        self.depth.add(count as i64);
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

        let gauge = QueueGauge {
            throughput,
            drain,
            depth: Gauge(depth),
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
        self.inner
            .register_list_callback(format!("{label}.depth"), var, Unit::Count, move || {
                NonZeroDisplay(depth_clone.load(Ordering::Relaxed))
            });

        let gauge = QueueGauge {
            throughput,
            drain,
            depth: Gauge(depth),
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
        let inner = self.inner.clone();
        let label = label.into();

        #[cfg(any(test, feature = "testing"))]
        if bach::is_active() {
            bach::spawn(report_loop(inner, label, move || {
                bach::time::sleep(interval)
            }));
            return;
        }

        tokio::spawn(report_loop(inner, label, move || {
            tokio::time::sleep(interval)
        }));
    }
}

async fn report_loop<F, Fut>(inner: s2n_quic_dc_metrics::Registry, label: String, sleep: F)
where
    F: Fn() -> Fut,
    Fut: core::future::Future<Output = ()>,
{
    loop {
        sleep().await;
        if !inner.is_open() {
            break;
        }
        if let Some(line) = inner.try_take_current_metrics_line_sparse(false) {
            // eprintln!("[raw] {line}");
            let line = Some(line).filter(|v| !v.is_empty()).and_then(|v| {
                let metrics = ParsedMetricsLine::parse(&v);
                (!metrics.is_empty()).then(|| metrics.format_pretty())
            });

            if let Some(formatted) = line {
                if label.is_empty() {
                    tracing::info!("{formatted}");
                } else {
                    tracing::info!("[{label}] {formatted}");
                }
            } else {
                tracing::info!("<no metrics>");
            }
        }
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
                if let Ok(bytes) = value.parse::<u64>() {
                    if bytes == 0 {
                        continue;
                    }

                    entries.push(MetricEntry::Throughput(ThroughputMetric { name, bytes }));
                }

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
        compute_histogram_percentiles(&self.buckets)
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

fn compute_histogram_percentiles(buckets: &[HistogramBucket]) -> (u64, u64, u64, u64) {
    let total_count = buckets.iter().map(|bucket| bucket.count).sum();
    if total_count == 0 {
        return (0, 0, 0, 0);
    }

    let p50_target = ((total_count as f64) * 0.5).ceil() as u64;
    let p99_target = ((total_count as f64) * 0.99).ceil() as u64;

    let mut cumulative: u64 = 0;
    let mut p50: u64 = 0;
    let mut p99: u64 = 0;
    let mut max: u64 = 0;

    for bucket in buckets {
        cumulative += bucket.count;
        if p50 == 0 && cumulative >= p50_target {
            p50 = bucket.value;
        }
        if p99 == 0 && cumulative >= p99_target {
            p99 = bucket.value;
        }
        max = bucket.value;
    }

    (total_count, p50, p99, max)
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
        // 273390965 bytes * 8 = 2187127720 bits ≈ 2.19 Gbps
        let line = "socket.rx:bytes=273390965";
        let result = ParsedMetricsLine::parse(line).format_pretty();
        assert_eq!(result, "socket.rx=2.19Gbps");
    }

    #[test]
    fn format_plain_counter() {
        let line = "rx.data=255470";
        let result = ParsedMetricsLine::parse(line).format_pretty();
        assert_eq!(result, "rx.data=255470");
    }

    #[test]
    fn format_mixed() {
        let line = "q.ack.drain=46005,q.ack.enq=46005,rx.data=255470,socket.tx:bytes=272721617";
        let result = ParsedMetricsLine::parse(line).format_pretty();
        assert_eq!(
            result,
            "q.ack=46005/46005 rx.data=255470 socket.tx=2.18Gbps"
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
}

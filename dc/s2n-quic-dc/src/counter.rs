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
            let line = Some(line)
                .filter(|v| !v.is_empty())
                .map(|v| format_metrics_line(&v))
                .filter(|v| !v.is_empty());

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

/// Parses the flat dc-metrics output and reformats it with grouped queue gauges
/// and human-readable bps values.
///
/// Naming conventions used for parsing:
/// - `name.enq`, `name.drain`, `name.depth` → grouped as `name=enq/drain(depth)`
/// - `name:bytes` → converted to `name=X.XXGbps`
/// - `name=value variant` (nominal) → grouped as `name(variant=value, ...)`
/// - Everything else → passed through as `name=value`
fn format_metrics_line(line: &str) -> String {
    use std::{collections::BTreeMap, fmt::Write};

    // Parse all key=value pairs, collecting nominal (aggregated) entries separately.
    // Raw format: "name=value" or "name=value aggregation"
    // Histograms also use spaces (e.g. "0*4541+1*4552 us") so only treat as nominal
    // when the value portion is a plain integer.
    // Variant histograms have a trailing variant name (e.g. "0*100+1*50 us packet_dispatch.0")
    // and are collected separately to avoid BTreeMap key collision.
    let mut metrics: BTreeMap<&str, &str> = BTreeMap::new();
    let mut nominals: BTreeMap<&str, Vec<(&str, &str)>> = BTreeMap::new();
    let mut variant_histograms: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for part in line.split(',') {
        if let Some((key, value)) = part.split_once('=') {
            if let Some((val, rest)) = value.split_once(' ') {
                if val.bytes().all(|b| b.is_ascii_digit()) {
                    nominals.entry(key).or_default().push((rest, val));
                } else if val.contains('*') && !is_histogram_unit_only(rest) {
                    variant_histograms.entry(key).or_default().push(value);
                } else {
                    metrics.insert(key, value);
                }
            } else {
                metrics.insert(key, value);
            }
        }
    }

    // Collect queue gauge bases (keys that have .enq/.drain/.depth siblings)
    let mut queue_bases: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
    // Collect hit/miss bases (keys that have .hit/.miss siblings)
    let mut hit_miss_bases: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
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

    let mut output = String::new();
    let mut emitted: std::collections::HashSet<&str> = std::collections::HashSet::new();

    for (&key, &value) in &metrics {
        if emitted.contains(key) {
            continue;
        }

        // Check if this key is part of a queue gauge group
        let queue_base = key
            .strip_suffix(".enq")
            .or_else(|| key.strip_suffix(".drain"))
            .or_else(|| key.strip_suffix(".depth"));

        if let Some(base) = queue_base {
            if queue_bases.contains(base) {
                if emitted.contains(base) {
                    continue;
                }
                emitted.insert(base);

                let enq_key = format!("{base}.enq");
                let drain_key = format!("{base}.drain");
                let depth_key = format!("{base}.depth");
                emitted.insert(
                    metrics
                        .keys()
                        .find(|k| **k == enq_key.as_str())
                        .copied()
                        .unwrap_or(""),
                );
                emitted.insert(
                    metrics
                        .keys()
                        .find(|k| **k == drain_key.as_str())
                        .copied()
                        .unwrap_or(""),
                );
                emitted.insert(
                    metrics
                        .keys()
                        .find(|k| **k == depth_key.as_str())
                        .copied()
                        .unwrap_or(""),
                );

                let enq = metrics.get(enq_key.as_str()).unwrap_or(&"0");
                let drain = metrics.get(drain_key.as_str()).unwrap_or(&"0");
                let depth = metrics.get(depth_key.as_str());

                if !output.is_empty() {
                    output.push(' ');
                }
                match depth {
                    Some(d) if *d != "0" => write!(output, "{base}={enq}/{drain}({d})").unwrap(),
                    _ => write!(output, "{base}={enq}/{drain}").unwrap(),
                }
                continue;
            }
        }

        // Check if this key is part of a hit/miss group
        let hm_base = key
            .strip_suffix(".hit")
            .or_else(|| key.strip_suffix(".miss"));

        if let Some(base) = hm_base {
            if hit_miss_bases.contains(base) {
                if emitted.contains(base) {
                    continue;
                }
                emitted.insert(base);

                let hit_key = format!("{base}.hit");
                let miss_key = format!("{base}.miss");
                emitted.insert(
                    metrics
                        .keys()
                        .find(|k| **k == hit_key.as_str())
                        .copied()
                        .unwrap_or(""),
                );
                emitted.insert(
                    metrics
                        .keys()
                        .find(|k| **k == miss_key.as_str())
                        .copied()
                        .unwrap_or(""),
                );

                let hit = metrics.get(hit_key.as_str()).unwrap_or(&"0");
                let miss = metrics.get(miss_key.as_str()).unwrap_or(&"0");

                if !output.is_empty() {
                    output.push(' ');
                }
                write!(output, "{base}={hit}/{miss}").unwrap();
                continue;
            }
        }

        // Check if this is a bytes counter → format as bps
        if let Some(name) = key.strip_suffix(":bytes") {
            emitted.insert(key);
            if let Ok(bytes) = value.parse::<u64>() {
                if bytes == 0 {
                    continue;
                }
                if !output.is_empty() {
                    output.push(' ');
                }
                let mut rate = bytes as f64 * 8.0;
                let prefixes = [("G", 1e9), ("M", 1e6), ("K", 1e3)];
                let mut prefix = "";
                for (p, divisor) in prefixes {
                    if rate >= divisor {
                        rate /= divisor;
                        prefix = p;
                        break;
                    }
                }
                write!(output, "{name}={rate:.2}{prefix}bps").unwrap();
            }
            continue;
        }

        // Check if this looks like a histogram (contains `*` and `+`)
        if value.contains('*') {
            emitted.insert(key);
            if !output.is_empty() {
                output.push(' ');
            }
            let formatted = format_histogram(key, value);
            output.push_str(&formatted);
            continue;
        }

        // Plain counter/gauge
        emitted.insert(key);
        if !output.is_empty() {
            output.push(' ');
        }
        write!(output, "{key}={value}").unwrap();
    }

    // Nominal (variant) counters: group by metric name
    for (key, variants) in &nominals {
        if !output.is_empty() {
            output.push(' ');
        }
        write!(output, "{key}(").unwrap();
        let mut first = true;
        for (variant, value) in variants {
            if !first {
                output.push(' ');
            }
            first = false;
            write!(output, "{variant}={value}").unwrap();
        }
        output.push(')');
    }

    // Variant histograms: group by key, each variant inside parentheses
    for (key, entries) in &variant_histograms {
        if !output.is_empty() {
            output.push(' ');
        }
        write!(output, "{key}(").unwrap();
        let mut first = true;
        for value in entries {
            if !first {
                output.push(' ');
            }
            first = false;
            format_histogram_variant(value, &mut output);
        }
        output.push(')');
    }

    output
}

/// Returns true if `rest` (the part after the first space in a histogram value)
/// is a recognized unit suffix only (e.g. "us", "B"), as opposed to containing a
/// variant name.
fn is_histogram_unit_only(rest: &str) -> bool {
    matches!(rest, "us" | "ms" | "s" | "B" | "KB" | "MB" | "GB")
}

/// Parses a dc-metrics histogram value and formats it as `key(n=N p50=V p99=V max=V unit)`.
fn format_histogram(key: &str, value: &str) -> String {
    use std::fmt::Write;

    let (data, unit, _variant) = parse_histogram_suffix(value);
    let (total_count, p50, p99, max) = compute_histogram_percentiles(data);

    if total_count == 0 {
        return format!("{key}=0");
    }

    let mut out = String::new();
    if unit == "us" {
        write!(
            out,
            "{key}(n={total_count} p50={} p99={} max={})",
            format_duration_us(p50),
            format_duration_us(p99),
            format_duration_us(max),
        )
        .unwrap();
    } else {
        write!(out, "{key}(n={total_count} p50={p50} p99={p99} max={max}").unwrap();
        if !unit.is_empty() {
            write!(out, " {unit}").unwrap();
        }
        out.push(')');
    }
    out
}

/// Formats a single variant histogram entry as `variant=(n=N p50=V p99=V max=V)`
/// appended to the provided output buffer.
fn format_histogram_variant(value: &str, out: &mut String) {
    use std::fmt::Write;

    let (data, unit, variant) = parse_histogram_suffix(value);
    let (total_count, p50, p99, max) = compute_histogram_percentiles(data);

    let label = if variant.is_empty() { "?" } else { variant };

    if total_count == 0 {
        write!(out, "{label}=0").unwrap();
        return;
    }

    if unit == "us" {
        write!(
            out,
            "{label}=(n={total_count} p50={} p99={} max={})",
            format_duration_us(p50),
            format_duration_us(p99),
            format_duration_us(max),
        )
        .unwrap();
    } else {
        write!(
            out,
            "{label}=(n={total_count} p50={p50} p99={p99} max={max}"
        )
        .unwrap();
        if !unit.is_empty() {
            write!(out, " {unit}").unwrap();
        }
        out.push(')');
    }
}

fn compute_histogram_percentiles(data: &str) -> (u64, u64, u64, u64) {
    let mut buckets: Vec<(u64, u64)> = Vec::new();
    let mut total_count: u64 = 0;
    for entry in data.split('+') {
        if let Some((val_str, count_str)) = entry.split_once('*') {
            if let (Ok(val), Ok(count)) = (val_str.parse::<u64>(), count_str.parse::<u64>()) {
                buckets.push((val, count));
                total_count += count;
            }
        }
    }

    if total_count == 0 {
        return (0, 0, 0, 0);
    }

    let p50_target = ((total_count as f64) * 0.5).ceil() as u64;
    let p99_target = ((total_count as f64) * 0.99).ceil() as u64;

    let mut cumulative: u64 = 0;
    let mut p50: u64 = 0;
    let mut p99: u64 = 0;
    let mut max: u64 = 0;

    for &(val, count) in &buckets {
        cumulative += count;
        if p50 == 0 && cumulative >= p50_target {
            p50 = val;
        }
        if p99 == 0 && cumulative >= p99_target {
            p99 = val;
        }
        max = val;
    }

    (total_count, p50, p99, max)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_queue_gauge() {
        let line = "q.packet.drain=45662,q.packet.depth=875,q.packet.enq=46180";
        let result = format_metrics_line(line);
        assert_eq!(result, "q.packet=46180/45662(875)");
    }

    #[test]
    fn format_queue_gauge_no_depth() {
        let line = "q.packet.drain=100,q.packet.enq=100";
        let result = format_metrics_line(line);
        assert_eq!(result, "q.packet=100/100");
    }

    #[test]
    fn format_bytes_as_bps() {
        // 273390965 bytes * 8 = 2187127720 bits ≈ 2.19 Gbps
        let line = "socket.rx:bytes=273390965";
        let result = format_metrics_line(line);
        assert_eq!(result, "socket.rx=2.19Gbps");
    }

    #[test]
    fn format_plain_counter() {
        let line = "rx.data=255470";
        let result = format_metrics_line(line);
        assert_eq!(result, "rx.data=255470");
    }

    #[test]
    fn format_mixed() {
        let line = "q.ack.drain=46005,q.ack.enq=46005,rx.data=255470,socket.tx:bytes=272721617";
        let result = format_metrics_line(line);
        assert_eq!(
            result,
            "q.ack=46005/46005 rx.data=255470 socket.tx=2.18Gbps"
        );
    }

    #[test]
    fn format_hit_miss() {
        let line = "rx.peer_cache.hit=80000,rx.peer_cache.miss=5";
        let result = format_metrics_line(line);
        assert_eq!(result, "rx.peer_cache=80000/5");
    }

    #[test]
    fn format_histogram_us() {
        let line = "rx.decrypt_time=0*4541+1*4552+1*4527+2*4617+5*378+13*45 us";
        let result = format_metrics_line(line);
        // total = 4541+4552+4527+4617+378+45 = 18660
        // p50 target = 9330, cumulative: 4541, 9093, 13620 → p50=1us
        // p99 target = 18474, cumulative: ...18237, 18615, 18660 → p99=5us
        assert_eq!(result, "rx.decrypt_time(n=18660 p50=1us p99=5us max=13us)");
    }

    #[test]
    fn format_histogram_count() {
        let line = "rx.frames_per_packet=4*5036+7*5079+15*6952+25*349";
        let result = format_metrics_line(line);
        // total = 5036+5079+6952+349 = 17416
        // p50 target = 8709 → cumulative 5036, 10115 → p50=7
        // p99 target = 17242 → cumulative 5036, 10115, 17067, 17416 → p99=25
        assert_eq!(result, "rx.frames_per_packet(n=17416 p50=7 p99=25 max=25)");
    }

    #[test]
    fn format_nominal() {
        let line = "rx.ecn=500 ect0,rx.ecn=3 ect1,rx.ecn=2 ce,rx.ecn=0 not_ect";
        let result = format_metrics_line(line);
        assert_eq!(result, "rx.ecn(ect0=500 ect1=3 ce=2 not_ect=0)");
    }

    #[test]
    fn format_variant_histogram_with_unit() {
        let line = "task.time=5*5000+10*3000+50*1500+200*500 us packet_dispatch.0,task.time=3*4000+20*3000+80*2000+300*1000 us packet_dispatch.1";
        let result = format_metrics_line(line);
        assert_eq!(
            result,
            "task.time(packet_dispatch.0=(n=10000 p50=5us p99=200us max=200us) packet_dispatch.1=(n=10000 p50=20us p99=300us max=300us))"
        );
    }

    #[test]
    fn format_variant_histogram_no_unit() {
        let line =
            "task.budget=1*8000+2*1500+4*300+10*200 packet_dispatch.0,task.budget=1*7000+2*2000+5*800+12*200 packet_dispatch.1";
        let result = format_metrics_line(line);
        assert_eq!(
            result,
            "task.budget(packet_dispatch.0=(n=10000 p50=1 p99=10 max=10) packet_dispatch.1=(n=10000 p50=1 p99=12 max=12))"
        );
    }

    #[test]
    fn format_variant_histogram_real_log() {
        let line = "task.budget=2*5321+3*208+4*1586+5*638+6*513+7*404+8*267+9*193+10*157+11*112+12*100+103*562 packet_dispatch.0,task.budget=2*6022+3*192+4*1576+5*544+6*514+7*413+8*309+9*236+10*157+11*138+12*115+147*840 packet_dispatch.1";
        let result = format_metrics_line(line);
        assert!(result.starts_with("task.budget(packet_dispatch.0=(n="));
        assert!(result.contains("packet_dispatch.1=(n="));
        assert!(result.ends_with(')'));
    }
}

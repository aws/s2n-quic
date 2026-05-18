// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

// ── Metrics line formatting ─────────────────────────────────────────────────

#[derive(Debug, Default, PartialEq, Eq)]
pub struct ParsedMetricsLine<'a> {
    pub entries: Vec<MetricEntry<'a>>,
}

impl<'a> ParsedMetricsLine<'a> {
    pub fn parse(line: &'a str) -> Self {
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

    pub fn format_pretty(&self) -> String {
        let mut output = String::new();

        for entry in &self.entries {
            entry.write_to(&mut output);
        }

        output
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn to_json_rows(&self) -> Vec<serde_json::Value> {
        use serde_json::json;

        let mut rows = Vec::new();

        for entry in &self.entries {
            match entry {
                MetricEntry::QueueGauge(m) => {
                    rows.push(json!({
                        "metric": m.name,
                        "type": "queue",
                        "enq": m.enq.parse::<u64>().unwrap_or(0),
                        "drain": m.drain.parse::<u64>().unwrap_or(0),
                        "depth": m.depth.and_then(|d| d.parse::<i64>().ok()).unwrap_or(0),
                    }));
                }
                MetricEntry::HitMiss(m) => {
                    rows.push(json!({
                        "metric": m.name,
                        "type": "hit_miss",
                        "hit": m.hit.parse::<u64>().unwrap_or(0),
                        "miss": m.miss.parse::<u64>().unwrap_or(0),
                    }));
                }
                MetricEntry::Throughput(m) => {
                    rows.push(json!({
                        "metric": m.name,
                        "type": "throughput",
                        "bytes": m.bytes,
                    }));
                }
                MetricEntry::Histogram(m) => {
                    let (count, p50, p99, max) = m.histogram.summarize();
                    rows.push(json!({
                        "metric": m.name,
                        "type": "histogram",
                        "unit": m.histogram.unit,
                        "count": count,
                        "p50": p50,
                        "p99": p99,
                        "max": max,
                    }));
                }
                MetricEntry::Scalar(m) => {
                    rows.push(json!({
                        "metric": m.name,
                        "type": "scalar",
                        "value": m.value.parse::<u64>().unwrap_or(0),
                    }));
                }
                MetricEntry::Nominal(m) => {
                    for v in &m.variants {
                        rows.push(json!({
                            "metric": m.name,
                            "type": "nominal",
                            "variant": v.label,
                            "value": v.value.parse::<u64>().unwrap_or(0),
                        }));
                    }
                }
                MetricEntry::VariantHistograms(m) => {
                    for v in &m.variants {
                        let (count, p50, p99, max) = v.histogram.summarize();
                        rows.push(json!({
                            "metric": m.name,
                            "type": "histogram",
                            "variant": v.label,
                            "unit": v.histogram.unit,
                            "count": count,
                            "p50": p50,
                            "p99": p99,
                            "max": max,
                        }));
                    }
                }
            }
        }

        rows
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum MetricEntry<'a> {
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
            Self::Histogram(metric) => metric.histogram.write_summary(metric.name, output),
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
pub struct QueueGaugeMetric<'a> {
    pub name: &'a str,
    pub enq: &'a str,
    pub drain: &'a str,
    pub depth: Option<&'a str>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct HitMissMetric<'a> {
    pub name: &'a str,
    pub hit: &'a str,
    pub miss: &'a str,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ThroughputMetric<'a> {
    pub name: &'a str,
    pub bytes: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub struct HistogramMetric<'a> {
    pub name: &'a str,
    pub histogram: Histogram<'a>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ScalarMetric<'a> {
    pub name: &'a str,
    pub value: &'a str,
}

#[derive(Debug, PartialEq, Eq)]
pub struct NominalMetric<'a> {
    pub name: &'a str,
    pub variants: Vec<NominalVariant<'a>>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct NominalVariant<'a> {
    pub label: &'a str,
    pub value: &'a str,
}

#[derive(Debug, PartialEq, Eq)]
pub struct VariantHistogramMetric<'a> {
    pub name: &'a str,
    pub variants: Vec<HistogramVariant<'a>>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct HistogramVariant<'a> {
    pub label: &'a str,
    pub histogram: Histogram<'a>,
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
        self.histogram.write_variant(self.label, output);
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Histogram<'a> {
    pub buckets: Vec<HistogramBucket>,
    pub unit: &'a str,
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

    pub fn summarize(&self) -> (u64, u64, u64, u64) {
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
pub struct HistogramBucket {
    pub value: u64,
    pub count: u64,
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

fn parse_byte_value(value: &str) -> Option<u64> {
    let (number, unit) = value.split_once(' ')?;
    if unit.trim() == "B" {
        number.parse().ok()
    } else {
        None
    }
}

pub fn compute_histogram_summary(buckets: &[HistogramBucket]) -> (u64, u64, u64, u64) {
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
pub fn histogram_count_min_max(buckets: &[HistogramBucket]) -> (u64, u64, u64) {
    let count = buckets.iter().map(|bucket| bucket.count).sum();
    let min = buckets.first().map_or(0, |bucket| bucket.value);
    let max = buckets.last().map_or(0, |bucket| bucket.value);
    (count, min, max)
}

/// Returns the value at the requested percentile for histogram buckets.
///
/// `percentile` is expected in the range `0..=100`. Empty buckets return `0`.
pub fn histogram_value_at_percentile(buckets: &[HistogramBucket], percentile: u32) -> u64 {
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

pub fn parse_histogram_buckets(data: &str) -> Vec<HistogramBucket> {
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
pub fn parse_histogram_suffix(value: &str) -> (&str, &str, &str) {
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

pub fn format_duration_us(us: u64) -> String {
    if us >= 1_000_000 {
        format!("{:.2}s", us as f64 / 1_000_000.0)
    } else if us >= 1_000 {
        format!("{:.2}ms", us as f64 / 1_000.0)
    } else {
        format!("{us}us")
    }
}

pub fn format_bits_per_second(bytes: u64) -> (f64, &'static str) {
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
}

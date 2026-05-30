// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use arrow::{
    array::*,
    datatypes::{DataType, Field, Schema},
    record_batch::RecordBatch,
};
use s2n_quic_dc_metrics::format::MetricRow;
use std::sync::{Arc, OnceLock};

pub const METRICS_BATCH_SIZE: usize = 8192;

pub fn metrics_schema() -> Arc<Schema> {
    static SCHEMA: OnceLock<Arc<Schema>> = OnceLock::new();
    SCHEMA
        .get_or_init(|| {
            Arc::new(Schema::new(vec![
                Field::new("ts", DataType::Float64, false),
                Field::new("source", DataType::Utf8, false),
                Field::new("log_group", DataType::Utf8, false),
                Field::new("stream", DataType::Utf8, false),
                Field::new("env", DataType::Utf8, false),
                Field::new("metric", DataType::Utf8, false),
                Field::new("type", DataType::Utf8, false),
                Field::new("variant", DataType::Utf8, true),
                Field::new("unit", DataType::Utf8, true),
                Field::new("value", DataType::Int64, true),
                Field::new("enq", DataType::UInt64, true),
                Field::new("drain", DataType::UInt64, true),
                Field::new("depth", DataType::Int64, true),
                Field::new("hit", DataType::UInt64, true),
                Field::new("miss", DataType::UInt64, true),
                Field::new("bytes", DataType::UInt64, true),
                Field::new("count", DataType::UInt64, true),
                Field::new("p50", DataType::UInt64, true),
                Field::new("p99", DataType::UInt64, true),
                Field::new("max", DataType::UInt64, true),
                Field::new(
                    "buckets",
                    DataType::Map(
                        Arc::new(Field::new(
                            "entries",
                            DataType::Struct(
                                vec![
                                    Arc::new(Field::new("key", DataType::UInt64, false)),
                                    Arc::new(Field::new("value", DataType::UInt64, false)),
                                ]
                                .into(),
                            ),
                            false,
                        )),
                        false,
                    ),
                    true,
                ),
            ]))
        })
        .clone()
}

pub struct MetricsBatchBuilder {
    ts: Float64Builder,
    source: StringBuilder,
    log_group: StringBuilder,
    stream: StringBuilder,
    env: StringBuilder,
    metric: StringBuilder,
    r#type: StringBuilder,
    variant: StringBuilder,
    unit: StringBuilder,
    value: Int64Builder,
    enq: UInt64Builder,
    drain: UInt64Builder,
    depth: Int64Builder,
    hit: UInt64Builder,
    miss: UInt64Builder,
    bytes: UInt64Builder,
    count: UInt64Builder,
    p50: UInt64Builder,
    p99: UInt64Builder,
    max: UInt64Builder,
    buckets: MapBuilder<UInt64Builder, UInt64Builder>,
    pub row_count: usize,
}

impl MetricsBatchBuilder {
    pub fn new() -> Self {
        Self {
            ts: Float64Builder::new(),
            source: StringBuilder::new(),
            log_group: StringBuilder::new(),
            stream: StringBuilder::new(),
            env: StringBuilder::new(),
            metric: StringBuilder::new(),
            r#type: StringBuilder::new(),
            variant: StringBuilder::new(),
            unit: StringBuilder::new(),
            value: Int64Builder::new(),
            enq: UInt64Builder::new(),
            drain: UInt64Builder::new(),
            depth: Int64Builder::new(),
            hit: UInt64Builder::new(),
            miss: UInt64Builder::new(),
            bytes: UInt64Builder::new(),
            count: UInt64Builder::new(),
            p50: UInt64Builder::new(),
            p99: UInt64Builder::new(),
            max: UInt64Builder::new(),
            buckets: MapBuilder::new(None, UInt64Builder::new(), UInt64Builder::new()),
            row_count: 0,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn push(
        &mut self,
        ts: f64,
        source: &str,
        log_group: Option<&str>,
        stream: Option<&str>,
        env: Option<&str>,
        row: &MetricRow,
    ) {
        self.ts.append_value(ts);
        self.source.append_value(source);
        self.log_group.append_value(log_group.unwrap_or_default());
        self.stream.append_value(stream.unwrap_or_default());
        self.env.append_value(env.unwrap_or_default());

        self.metric.append_value(row.metric);
        self.r#type.append_value(row.r#type);

        append_opt_str(&mut self.variant, row.variant);
        append_opt_str(&mut self.unit, row.unit);
        append_opt_i64(&mut self.value, row.value);
        append_opt_u64(&mut self.enq, row.enq);
        append_opt_u64(&mut self.drain, row.drain);
        append_opt_i64(&mut self.depth, row.depth);
        append_opt_u64(&mut self.hit, row.hit);
        append_opt_u64(&mut self.miss, row.miss);
        append_opt_u64(&mut self.bytes, row.bytes);
        append_opt_u64(&mut self.count, row.count);
        append_opt_u64(&mut self.p50, row.p50);
        append_opt_u64(&mut self.p99, row.p99);
        append_opt_u64(&mut self.max, row.max);

        match row.buckets.as_ref() {
            Some(buckets) => {
                for (bucket_value, bucket_count) in buckets {
                    self.buckets.keys().append_value(*bucket_value);
                    self.buckets.values().append_value(*bucket_count);
                }
                self.buckets
                    .append(true)
                    .expect("map builder state inconsistent");
            }
            None => self
                .buckets
                .append(false)
                .expect("map builder state inconsistent"),
        }

        self.row_count += 1;
    }

    pub fn finish(&mut self) -> RecordBatch {
        RecordBatch::try_new(
            metrics_schema(),
            vec![
                Arc::new(self.ts.finish()),
                Arc::new(self.source.finish()),
                Arc::new(self.log_group.finish()),
                Arc::new(self.stream.finish()),
                Arc::new(self.env.finish()),
                Arc::new(self.metric.finish()),
                Arc::new(self.r#type.finish()),
                Arc::new(self.variant.finish()),
                Arc::new(self.unit.finish()),
                Arc::new(self.value.finish()),
                Arc::new(self.enq.finish()),
                Arc::new(self.drain.finish()),
                Arc::new(self.depth.finish()),
                Arc::new(self.hit.finish()),
                Arc::new(self.miss.finish()),
                Arc::new(self.bytes.finish()),
                Arc::new(self.count.finish()),
                Arc::new(self.p50.finish()),
                Arc::new(self.p99.finish()),
                Arc::new(self.max.finish()),
                Arc::new(self.buckets.finish()),
            ],
        )
        .expect("schema mismatch in metrics batch builder")
    }
}

fn append_opt_str(builder: &mut StringBuilder, val: Option<&str>) {
    match val {
        Some(s) => builder.append_value(s),
        None => builder.append_null(),
    }
}

fn append_opt_u64(builder: &mut UInt64Builder, val: Option<u64>) {
    match val {
        Some(n) => builder.append_value(n),
        None => builder.append_null(),
    }
}

fn append_opt_i64(builder: &mut Int64Builder, val: Option<i64>) {
    match val {
        Some(n) => builder.append_value(n),
        None => builder.append_null(),
    }
}

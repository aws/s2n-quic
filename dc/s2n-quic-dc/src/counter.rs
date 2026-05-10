// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::time::Duration;
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicI64, AtomicU64, Ordering},
        Arc, Mutex,
    },
};

#[derive(Clone)]
enum Metric {
    Counter(Arc<AtomicU64>),
    Gauge(Arc<AtomicI64>),
    Queue {
        enqueue: Arc<AtomicU64>,
        drain: Arc<AtomicU64>,
        depth: Arc<AtomicI64>,
    },
}

impl Metric {
    fn format(&self, label: &'static str) -> Option<String> {
        match self {
            Metric::Counter(v) => {
                let count = v.swap(0, Ordering::Relaxed);
                if count == 0 {
                    return None;
                }
                if label.ends_with(":bytes") {
                    let mut rate = count as f64 * 8.0;
                    let prefixes = [("G", 1e9), ("M", 1e6), ("K", 1e3)];
                    let mut prefix = "";
                    for (p, divisor) in prefixes {
                        if rate >= divisor {
                            rate /= divisor;
                            prefix = p;
                            break;
                        }
                    }
                    let label_without_suffix = label.trim_end_matches(":bytes");
                    Some(format!("{}={:.2}{}bps", label_without_suffix, rate, prefix))
                } else {
                    Some(format!("{}={}", label, count))
                }
            }
            Metric::Gauge(v) => {
                let depth = v.load(Ordering::Relaxed);
                if depth == 0 {
                    return None;
                }
                Some(format!("{}={}", label, depth))
            }
            Metric::Queue {
                enqueue,
                drain,
                depth,
            } => {
                let enq = enqueue.swap(0, Ordering::Relaxed);
                let drn = drain.swap(0, Ordering::Relaxed);
                let dep = depth.load(Ordering::Relaxed);
                if enq == 0 && drn == 0 && dep == 0 {
                    return None;
                }
                if dep == 0 {
                    Some(format!("{label}={enq}/{drn}"))
                } else {
                    Some(format!("{label}={enq}/{drn}({dep})"))
                }
            }
        }
    }
}

#[derive(Clone, Default)]
pub struct Registry {
    metrics: Arc<Mutex<HashMap<&'static str, Metric>>>,
}

impl Registry {
    pub fn new() -> Self {
        Self {
            metrics: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn register(&self, label: &'static str) -> Counter {
        let mut metrics = self.metrics.lock().unwrap();
        let inner = match metrics.entry(label) {
            std::collections::hash_map::Entry::Occupied(e) => match e.get() {
                Metric::Counter(v) => v.clone(),
                _ => panic!("label {label:?} already registered as a different metric type"),
            },
            std::collections::hash_map::Entry::Vacant(e) => {
                let v = Arc::new(AtomicU64::new(0));
                e.insert(Metric::Counter(v.clone()));
                v
            }
        };
        Counter::new(inner)
    }

    pub fn register_queue_gauge(&self, label: &'static str) -> QueueGauge {
        let mut metrics = self.metrics.lock().unwrap();
        match metrics.entry(label) {
            std::collections::hash_map::Entry::Occupied(e) => match e.get() {
                Metric::Queue {
                    enqueue,
                    drain,
                    depth,
                } => QueueGauge {
                    throughput: Counter::new(enqueue.clone()),
                    drain: Counter::new(drain.clone()),
                    depth: Gauge(depth.clone()),
                },
                _ => panic!("label {label:?} already registered as a different metric type"),
            },
            std::collections::hash_map::Entry::Vacant(e) => {
                let enqueue = Arc::new(AtomicU64::new(0));
                let drain = Arc::new(AtomicU64::new(0));
                let depth = Arc::new(AtomicI64::new(0));
                e.insert(Metric::Queue {
                    enqueue: enqueue.clone(),
                    drain: drain.clone(),
                    depth: depth.clone(),
                });
                QueueGauge {
                    throughput: Counter::new(enqueue),
                    drain: Counter::new(drain),
                    depth: Gauge(depth),
                }
            }
        }
    }

    pub fn register_gauge(&self, label: &'static str) -> Gauge {
        let mut metrics = self.metrics.lock().unwrap();
        let inner = match metrics.entry(label) {
            std::collections::hash_map::Entry::Occupied(e) => match e.get() {
                Metric::Gauge(v) => v.clone(),
                _ => panic!("label {label:?} already registered as a different metric type"),
            },
            std::collections::hash_map::Entry::Vacant(e) => {
                let v = Arc::new(AtomicI64::new(0));
                e.insert(Metric::Gauge(v.clone()));
                v
            }
        };
        Gauge(inner)
    }

    pub fn spawn_reporter(&self, interval: Duration) {
        let metrics = self.metrics.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(interval);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            loop {
                interval.tick().await;

                let metrics = metrics.lock().unwrap();
                if metrics.is_empty() {
                    continue;
                }

                let mut labels: Vec<&'static str> = metrics.keys().copied().collect();
                labels.sort();

                let parts: Vec<String> = labels
                    .into_iter()
                    .filter_map(|label| metrics[label].format(label))
                    .collect();

                if !parts.is_empty() {
                    tracing::info!("{}", parts.join(" "));
                }
            }
        });
    }
}

#[derive(Clone)]
pub struct Counter(Arc<AtomicU64>);

impl Counter {
    #[inline]
    pub fn new(inner: Arc<AtomicU64>) -> Self {
        Self(inner)
    }

    #[inline]
    pub fn add(&self, v: u64) {
        self.0.fetch_add(v, Ordering::Relaxed);
    }
}

impl core::ops::AddAssign<u64> for Counter {
    #[inline]
    fn add_assign(&mut self, rhs: u64) {
        self.add(rhs);
    }
}

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

    /// Returns the current gauge value.
    #[inline]
    pub fn get(&self) -> i64 {
        self.0.load(Ordering::Relaxed)
    }
}

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

    /// Decrements the queue depth by `count` items and records them as drained.
    ///
    /// Use this instead of calling [`dequeue`] in a loop when receiving a batch
    /// of items at once, so the depth and drain counters stay accurate.
    #[inline]
    pub fn dequeue_n(&self, count: u64) {
        self.drain.add(count);
        self.depth.sub(count as i64);
    }
}

pub struct GaugedQueue<T, R> {
    inner: R,
    queue: crate::intrusive_queue::Queue<T>,
    gauge: QueueGauge,
}

impl<T, R> GaugedQueue<T, R> {
    pub fn new(inner: R, gauge: QueueGauge) -> Self {
        Self {
            inner,
            queue: Default::default(),
            gauge,
        }
    }
}

impl<T, R> crate::socket::channel::Receiver<crate::intrusive_queue::Entry<T>> for GaugedQueue<T, R>
where
    R: crate::socket::channel::Receiver<crate::intrusive_queue::Queue<T>>,
{
    fn poll_recv(
        &mut self,
        cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Option<crate::intrusive_queue::Entry<T>>> {
        loop {
            if let Some(entry) = self.queue.pop_front() {
                self.gauge.dequeue();
                return core::task::Poll::Ready(Some(entry));
            }

            match self.inner.poll_recv(cx) {
                core::task::Poll::Ready(Some(queue)) => {
                    if queue.is_empty() {
                        cx.waker().wake_by_ref();
                        return core::task::Poll::Pending;
                    }
                    self.gauge.enqueue(queue.len() as u64);
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

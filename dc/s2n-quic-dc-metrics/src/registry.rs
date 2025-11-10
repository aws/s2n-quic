// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, Mutex},
};

use crate::{rseq::Channels, BoolCounter, Counter, Summary, Unit};

/// A `Registry` allows registering metrics for emission and can be asked to periodically emit
/// them.
///
/// `Clone` for `Registry` will share the underlying storage. This can make it easier to put
/// recorders into various structures, though callers should prefer to register individual metrics
/// up front (rather than repeatedly doing so).
#[derive(Clone)]
pub struct Registry {
    inner: Arc<Mutex<RegistryInner>>,
}

pub(crate) struct RegistryInner {
    // Use a BTreeMap so that we automatically get consistent ordering of the reported metrics.
    // Consistent ordering makes it easier to analyze them locally visually or with ad-hoc scripts.
    metrics: BTreeMap<MetricKey, MetricValue>,

    counters: Arc<Channels<crate::counter::SharedCounter>>,
    histograms: Arc<Channels<crate::summary::SharedSummary>>,

    task_monitors: HashMap<String, crate::TaskMonitor>,

    is_open: bool,
}

impl RegistryInner {
    pub fn try_take_current_metrics_line(&mut self) -> Option<String> {
        if !self.is_open {
            return None;
        }

        let mut output = String::new();

        // Ensure that all per-CPU data is aggregated.
        self.counters.steal_pages();
        self.histograms.steal_pages();

        let mut first = true;
        for (key, value) in self.metrics.iter_mut() {
            let v = match value {
                MetricValue::Counter(c) => c.take_current(),
                MetricValue::Summary(s) => s.take_current(),
                MetricValue::BoolCounter(b) => b.take_current(),
                MetricValue::ValueList(c) => c.take_current(),
            };

            if let Some(v) = v {
                if !first {
                    output.push(',');
                }
                first = false;

                output.push_str(&key.name);
                output.push('=');
                output.push_str(&v);

                if let Some(agg) = &key.aggregation {
                    output.push(' ');
                    output.push_str(agg);
                }
            }
        }

        Some(output)
    }
}

impl Registry {
    pub fn new() -> Registry {
        Registry {
            inner: Arc::new(Mutex::new(RegistryInner {
                metrics: BTreeMap::new(),
                counters: Arc::new(Channels::new()),
                histograms: Arc::new(Channels::new()),
                task_monitors: HashMap::new(),
                is_open: true,
            })),
        }
    }

    pub fn register_task_monitor(&self, task: &str) -> crate::TaskMonitor {
        let aggregation = format!("Task|{task}");

        let guard = self.inner.lock().unwrap();
        if let Some(monitor) = guard.task_monitors.get(&aggregation) {
            monitor.clone()
        } else {
            drop(guard);
            let monitor = crate::TaskMonitor::new(self, aggregation.clone());
            let mut guard = self.inner.lock().unwrap();
            guard.task_monitors.insert(aggregation, monitor.clone());
            monitor
        }
    }

    /// Registers a given metric (name, aggregation) with the recorder as a `Counter`.
    ///
    /// This will deduplicate calls, but is somewhat expensive, so prefer to call just once and
    /// then reuse the returned type.
    #[track_caller]
    pub fn register_counter(&self, metric: String, aggregation: Option<String>) -> Counter {
        let mut inner = self.inner.lock().unwrap();
        let inner = &mut *inner;

        let entry = inner
            .metrics
            .entry(MetricKey {
                name: metric.clone(),
                aggregation: aggregation.clone(),
            })
            .or_insert_with(|| MetricValue::Counter(Counter::new(inner.counters.clone())));

        if let MetricValue::Counter(c) = &*entry {
            c.clone()
        } else {
            panic!(
                "Non-counter metric name={metric:?}, aggregation={aggregation:?} already registered"
            )
        }
    }

    /// Registers a given metric (name, class, instance) with the recorder as a `Summary`.
    ///
    /// This will deduplicate calls, but is somewhat expensive, so prefer to call just once and
    /// then reuse the returned type.
    #[track_caller]
    pub fn register_summary(
        &self,
        metric: String,
        aggregation: Option<String>,
        display_unit: Unit,
    ) -> Summary {
        let mut inner = self.inner.lock().unwrap();
        let inner = &mut *inner;

        let entry = inner
            .metrics
            .entry(MetricKey {
                name: metric.clone(),
                aggregation: aggregation.clone(),
            })
            .or_insert_with(|| {
                MetricValue::Summary(Summary::new(inner.histograms.clone(), display_unit))
            });

        if let MetricValue::Summary(s) = &*entry {
            s.clone()
        } else {
            panic!(
                "Non-summary metric name={metric:?}, aggregation={aggregation:?} already registered"
            )
        }
    }

    /// Registers a given metric with the recorder as a `BoolCounter`.
    ///
    /// This will deduplicate calls, but is somewhat expensive, so prefer to call just once and
    /// then reuse the returned type.
    #[track_caller]
    pub fn register_bool(&self, metric: String, aggregation: Option<String>) -> BoolCounter {
        let mut inner = self.inner.lock().unwrap();
        let inner = &mut *inner;

        let entry = inner
            .metrics
            .entry(MetricKey {
                name: metric.clone(),
                aggregation: aggregation.clone(),
            })
            .or_insert_with(|| MetricValue::BoolCounter(BoolCounter::new(inner.counters.clone())));

        if let MetricValue::BoolCounter(b) = &*entry {
            b.clone()
        } else {
            panic!(
                "Non-bool metric name={metric:?}, aggregation={aggregation:?} already registered"
            )
        }
    }

    /// Registers a given metric with the recorder, where the value is obtained by calling the
    /// provided function, which should serialize it into the passed string.
    ///
    /// The provided type must match across all calls (we store and confirm this via `Any`).
    ///
    /// On repeat calls with matching metric name and aggregation, `MetricCallback::register_extra`
    /// is called. See its documentation for details.
    #[track_caller]
    pub fn register_list_callback<D, F>(
        &self,
        metric: String,
        aggregation: Option<String>,
        unit: Unit,
        callback: F,
    ) where
        D: std::fmt::Display,
        F: FnMut() -> D + 'static + Send,
    {
        let mut inner = self.inner.lock().unwrap();

        let entry = inner.metrics.entry(MetricKey {
            name: metric.clone(),
            aggregation: aggregation.clone(),
        });

        match entry {
            std::collections::btree_map::Entry::Vacant(v) => {
                v.insert(MetricValue::ValueList(Box::new((vec![callback], unit))));
            }
            std::collections::btree_map::Entry::Occupied(mut o) => {
                if let MetricValue::ValueList(previous) = o.get_mut() {
                    if let Some(previous) = previous.as_any().downcast_mut::<(Vec<F>, Unit)>() {
                        assert_eq!(previous.1, unit);
                        previous.0.push(callback);
                    } else {
                        panic!(
                            "Callback metric name={metric:?}, aggregation={aggregation:?} already registered with different type"
                        );
                    }
                } else {
                    panic!(
                        "Non-callback metric name={metric:?}, aggregation={aggregation:?} already registered"
                    )
                }
            }
        }
    }

    pub fn has_metrics(&self) -> bool {
        !self.inner.lock().unwrap().metrics.is_empty()
    }

    /// Compute and return the latest metrics line.
    ///
    /// This returns the text which should be placed after `Metrics=` into the service log.
    ///
    /// Note that this will reset various counters, so this shouldn't be called unless emitting
    /// into logs.
    ///
    /// # Panics
    ///
    /// * If the registry has been closed
    pub fn take_current_metrics_line(&self) -> String {
        self.try_take_current_metrics_line()
            .expect("cannot take metrics from closed registry")
    }

    /// Compute and return the latest metrics line if the registry is open.
    ///
    /// This returns the text which should be placed after `Metrics=` into the service log.
    ///
    /// Note that this will reset various counters, so this shouldn't be called unless emitting
    /// into logs.
    pub fn try_take_current_metrics_line(&self) -> Option<String> {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .try_take_current_metrics_line()
    }

    /// Returns `true` if the registry is open
    pub fn is_open(&self) -> bool {
        self.inner.lock().is_ok_and(|inner| inner.is_open)
    }

    /// Closes the registry
    ///
    /// This is used as a mechanism to notify and background workers that metrics are no longer being
    /// updated and should shut down.
    pub fn close(&self) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.is_open = false;
        }
    }
}

impl Default for Registry {
    fn default() -> Self {
        Registry::new()
    }
}

/// This represents a single entry in our emitted service log, with optional aggregation along the
/// two class/instance dimensions.
#[derive(PartialEq, Eq, Hash, PartialOrd, Ord)]
struct MetricKey {
    name: String,
    aggregation: Option<String>,
}

/// This represents metric state. Note that a single metric may collect many different values
/// between emissions; so a "value" represents potentially multiple recorded points.
///
/// (FIXME: rename this to something else?)
enum MetricValue {
    Counter(Counter),
    Summary(Summary),
    BoolCounter(BoolCounter),
    ValueList(Box<dyn crate::callback::ValueList + Send>),
}

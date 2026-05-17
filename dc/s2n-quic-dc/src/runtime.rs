// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Runtime abstraction for stream endpoint initialization.
//!
//! This provides a generic interface for spawning tasks and obtaining clocks across
//! different runtimes (busy-poll, tokio, bach) while respecting worker affinity for
//! non-Send types.
//!
//! The key challenge is that busy-poll uses a two-phase spawn pattern:
//! 1. Call `handle.spawn_local(|spawner| { ... })` with a Send closure
//! 2. Inside that closure, use `spawner.spawn(future)` to spawn !Send futures
//!
//! This abstraction needs to support both this pattern and simpler runtimes like tokio.

use crate::counter;
use crate::time::precision;
use core::fmt;
use s2n_quic_core::time;
use std::future::Future;

#[derive(Clone, Debug)]
pub struct MetricRegistration {
    pub label: String,
    pub variant: Option<String>,
    pub kind: counter::MetricKind,
    pub unit: Option<&'static str>,
    pub description: String,
}

impl MetricRegistration {
    #[inline]
    pub fn new(
        name: impl fmt::Display,
        kind: counter::MetricKind,
        description: impl fmt::Display,
    ) -> Self {
        Self {
            label: name.to_string(),
            variant: None,
            kind,
            unit: None,
            description: description.to_string(),
        }
    }

    #[inline]
    pub fn with_variant(mut self, variant: impl fmt::Display) -> Self {
        self.variant = Some(variant.to_string());
        self
    }

    #[inline]
    pub fn with_unit(mut self, unit: &'static str) -> Self {
        self.unit = Some(unit);
        self
    }
}

pub trait Metric {
    fn registrations(&self) -> Vec<MetricRegistration>;
}

impl Metric for MetricRegistration {
    fn registrations(&self) -> Vec<MetricRegistration> {
        vec![self.clone()]
    }
}

impl Metric for counter::Task {
    fn registrations(&self) -> Vec<MetricRegistration> {
        self.metrics()
            .into_iter()
            .map(|metric| {
                let mut registration =
                    MetricRegistration::new(metric.label, metric.kind, metric.description);
                if let Some(variant) = metric.variant {
                    registration = registration.with_variant(variant);
                }
                if let Some(unit) = metric.unit {
                    registration = registration.with_unit(unit);
                }
                registration
            })
            .collect()
    }
}

impl Metric for counter::QueueGauge {
    fn registrations(&self) -> Vec<MetricRegistration> {
        self.metrics()
            .into_iter()
            .map(|metric| {
                let mut registration =
                    MetricRegistration::new(metric.label, metric.kind, metric.description);
                if let Some(variant) = metric.variant {
                    registration = registration.with_variant(variant);
                }
                if let Some(unit) = metric.unit {
                    registration = registration.with_unit(unit);
                }
                registration
            })
            .collect()
    }
}

impl Metric for counter::QueueSender {
    fn registrations(&self) -> Vec<MetricRegistration> {
        self.metrics()
            .into_iter()
            .map(|metric| {
                let mut registration =
                    MetricRegistration::new(metric.label, metric.kind, metric.description);
                if let Some(variant) = metric.variant {
                    registration = registration.with_variant(variant);
                }
                if let Some(unit) = metric.unit {
                    registration = registration.with_unit(unit);
                }
                registration
            })
            .collect()
    }
}

impl Metric for counter::QueueReceiver {
    fn registrations(&self) -> Vec<MetricRegistration> {
        self.metrics()
            .into_iter()
            .map(|metric| {
                let mut registration =
                    MetricRegistration::new(metric.label, metric.kind, metric.description);
                if let Some(variant) = metric.variant {
                    registration = registration.with_variant(variant);
                }
                if let Some(unit) = metric.unit {
                    registration = registration.with_unit(unit);
                }
                registration
            })
            .collect()
    }
}

/// Describes a spawned pipeline task for runtime-level introspection.
#[derive(Clone, Debug)]
pub struct TaskRegistration {
    pub name: String,
    pub description: String,
    pub function: String,
    pub budget: Option<usize>,
    pub metrics: Vec<MetricRegistration>,
}

impl TaskRegistration {
    #[inline]
    pub fn new(
        name: impl fmt::Display,
        description: impl fmt::Display,
        function: impl fmt::Display,
    ) -> Self {
        Self {
            name: name.to_string(),
            description: description.to_string(),
            function: function.to_string(),
            budget: None,
            metrics: Vec::new(),
        }
    }

    #[inline]
    pub fn with_budget(mut self, budget: Option<usize>) -> Self {
        self.budget = budget;
        self
    }

    #[inline]
    pub fn with_metric(mut self, metric: &impl Metric) -> Self {
        for metric in metric.registrations() {
            self.upsert_metric(metric);
        }
        self
    }

    #[inline]
    fn upsert_metric(&mut self, metric: MetricRegistration) {
        if let Some(current) = self
            .metrics
            .iter_mut()
            .find(|m| m.label == metric.label && m.variant == metric.variant)
        {
            *current = metric;
        } else {
            self.metrics.push(metric);
        }
    }
}

/// Describes a channel/queue entity in the runtime pipeline.
#[derive(Clone, Debug)]
pub struct ChannelRegistration {
    pub name: String,
    pub description: String,
    pub function: String,
    pub metrics: Vec<MetricRegistration>,
}

impl ChannelRegistration {
    #[inline]
    pub fn new(
        name: impl fmt::Display,
        description: impl fmt::Display,
        function: impl fmt::Display,
    ) -> Self {
        Self {
            name: name.to_string(),
            description: description.to_string(),
            function: function.to_string(),
            metrics: Vec::new(),
        }
    }

    #[inline]
    pub fn with_metric(mut self, metric: &impl Metric) -> Self {
        for metric in metric.registrations() {
            self.upsert_metric(metric);
        }
        self
    }

    #[inline]
    fn upsert_metric(&mut self, metric: MetricRegistration) {
        if let Some(current) = self
            .metrics
            .iter_mut()
            .find(|m| m.label == metric.label && m.variant == metric.variant)
        {
            *current = metric;
        } else {
            self.metrics.push(metric);
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChannelDirection {
    Sends,
    Receives,
}

impl fmt::Display for ChannelDirection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sends => f.write_str("sends"),
            Self::Receives => f.write_str("receives"),
        }
    }
}

#[derive(Clone, Debug)]
pub struct ChannelBinding {
    pub task_name: String,
    pub channel_name: String,
    pub direction: ChannelDirection,
    pub description: String,
    pub function: String,
}

impl ChannelBinding {
    #[inline]
    pub fn new(
        task_name: impl fmt::Display,
        channel_name: impl fmt::Display,
        direction: ChannelDirection,
        description: impl fmt::Display,
        function: impl fmt::Display,
    ) -> Self {
        Self {
            task_name: task_name.to_string(),
            channel_name: channel_name.to_string(),
            direction,
            description: description.to_string(),
            function: function.to_string(),
        }
    }
}

/// Abstraction over a task runtime and its associated clock.
///
/// Each runtime implementation bundles both spawning capability and the clock
/// appropriate for that execution model (e.g. busy-poll timers that don't use wakers,
/// tokio timers backed by the tokio runtime, bach simulated time).
pub trait Runtime: Clone + Send + 'static {
    /// The clock type associated with this runtime.
    type Clock: time::Clock + precision::Clock + Clone + Send + 'static;

    /// The worker-local spawner type for spawning !Send futures.
    type Spawner<'a>: Spawner;

    /// Number of workers in this runtime.
    fn worker_count(&self) -> usize;

    /// Returns a clone of the runtime's clock.
    fn clock(&self) -> Self::Clock;

    /// Spawn !Send futures on a specific worker.
    ///
    /// The closure receives a spawner handle that can spawn !Send futures.
    /// This matches the busy-poll pattern where you call spawn_local with a Send closure
    /// that receives a Spawner to spawn !Send futures.
    fn spawn_local<F>(&self, worker_id: usize, f: F)
    where
        F: FnOnce(Self::Spawner<'_>) + Send + 'static;
}

/// Handle for spawning !Send futures within a worker-local context.
pub trait Spawner {
    /// Spawn a !Send future on the current worker.
    fn spawn<F>(&mut self, future: F)
    where
        F: Future<Output = ()> + 'static;

    /// Register metadata describing a task spawned on this worker.
    #[inline]
    fn register_task(&mut self, _task: TaskRegistration) {}

    /// Register metadata describing a queue/channel edge on this worker.
    #[inline]
    fn register_channel(&mut self, _channel: ChannelRegistration) {}

    #[inline]
    fn register_queue_channel(&mut self, channel: &counter::QueueGauge) {
        let registration = channel.with_registration_metadata_ref(|name, description, function| {
            ChannelRegistration::new(name, description, function).with_metric(channel)
        });
        self.register_channel(registration);
    }

    /// Register a relationship between a task and a channel entity.
    #[inline]
    fn register_channel_binding(&mut self, _binding: ChannelBinding) {}

    /// Register that a task sends on a channel.
    #[inline]
    fn register_channel_sender(
        &mut self,
        task_name: impl fmt::Display,
        channel_name: impl fmt::Display,
        description: impl fmt::Display,
        function: impl fmt::Display,
    ) {
        self.register_channel_binding(ChannelBinding::new(
            task_name,
            channel_name,
            ChannelDirection::Sends,
            description,
            function,
        ));
    }

    #[inline]
    fn register_queue_sender(&mut self, sender: &counter::QueueSender) {
        let channel_name = sender.channel_metadata(|name, _, _| name.to_string());
        self.register_channel_sender(
            sender.task_name(),
            channel_name,
            sender.description(),
            sender.function(),
        );
    }

    /// Register that a task receives from a channel.
    #[inline]
    fn register_channel_receiver(
        &mut self,
        task_name: impl fmt::Display,
        channel_name: impl fmt::Display,
        description: impl fmt::Display,
        function: impl fmt::Display,
    ) {
        self.register_channel_binding(ChannelBinding::new(
            task_name,
            channel_name,
            ChannelDirection::Receives,
            description,
            function,
        ));
    }

    #[inline]
    fn register_queue_receiver(&mut self, receiver: &counter::QueueReceiver) {
        let channel_name = receiver.channel_metadata(|name, _, _| name.to_string());
        self.register_channel_receiver(
            receiver.task_name(),
            channel_name,
            receiver.description(),
            receiver.function(),
        );
    }

    /// Convenience helper for pipeline tasks with budget and task counters.
    #[inline]
    fn spawn_receiver_task<F>(
        &mut self,
        future: F,
        budget: Option<usize>,
        task_counter: counter::Task,
    ) where
        F: Future<Output = ()> + 'static,
        Self: Sized,
    {
        let task = task_counter.with_registration_metadata_ref(|name, description, function| {
            TaskRegistration::new(name, description, function)
                .with_budget(budget)
                .with_metric(&task_counter)
        });
        self.register_task(task);
        self.spawn(future);
    }
}

// ── BusyPoll Implementation ────────────────────────────────────────────────

/// Implementations for busy_poll runtime
pub mod busy_poll {
    use super::{Runtime, Spawner};
    use crate::busy_poll::clock;
    use std::future::Future;

    /// Busy-poll runtime: a pool of polling workers with a wall-clock timer.
    #[derive(Clone)]
    pub struct Handle {
        pool: crate::busy_poll::Pool,
        clock: clock::Clock,
    }

    impl Handle {
        pub fn new(pool: crate::busy_poll::Pool) -> Self {
            Self {
                pool,
                clock: clock::Clock::new(),
            }
        }
    }

    impl Runtime for Handle {
        type Clock = clock::Timer;
        type Spawner<'a> = crate::busy_poll::Spawner<'a>;

        fn worker_count(&self) -> usize {
            self.pool.len()
        }

        fn clock(&self) -> Self::Clock {
            use crate::time::precision::Clock as _;
            self.clock.timer()
        }

        fn spawn_local<F>(&self, worker_id: usize, f: F)
        where
            F: FnOnce(Self::Spawner<'_>) + Send + 'static,
        {
            self.pool[worker_id].spawn_local(f);
        }
    }

    impl Spawner for crate::busy_poll::Spawner<'_> {
        fn spawn<F>(&mut self, future: F)
        where
            F: Future<Output = ()> + 'static,
        {
            crate::busy_poll::Spawner::spawn(self, future);
        }
    }
}

// ── Bach Implementation ────────────────────────────────────────────────────

/// Bach runtime for deterministic testing
#[cfg(any(test, feature = "testing"))]
pub mod bach {
    use super::{Runtime, Spawner};
    use std::future::Future;

    /// Bach runtime for deterministic testing.
    ///
    /// Bach is single-threaded but we emulate multiple workers for testing worker affinity logic.
    #[derive(Clone)]
    pub struct Handle {
        worker_count: usize,
        clock: crate::time::bach::Clock,
    }

    impl Handle {
        pub fn new(worker_count: usize) -> Self {
            Self {
                worker_count,
                clock: crate::time::bach::Clock::default(),
            }
        }
    }

    /// Bach local spawner
    pub struct Local;

    /// Wrapper to make !Send futures Send for bach's API
    struct SendWrapper<F>(F);

    // SAFETY: Bach is single-threaded and never executes concurrently
    unsafe impl<F> Send for SendWrapper<F> {}
    unsafe impl<F> Sync for SendWrapper<F> {}

    impl<F> Future for SendWrapper<F>
    where
        F: Future,
    {
        type Output = F::Output;

        fn poll(
            self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Self::Output> {
            unsafe {
                std::future::Future::poll(
                    std::pin::Pin::new_unchecked(&mut self.get_unchecked_mut().0),
                    cx,
                )
            }
        }
    }

    impl Spawner for Local {
        fn spawn<F>(&mut self, future: F)
        where
            F: Future<Output = ()> + 'static,
        {
            ::bach::spawn(SendWrapper(future));
        }
    }

    impl Runtime for Handle {
        type Clock = crate::time::bach::Clock;
        type Spawner<'a> = Local;

        fn worker_count(&self) -> usize {
            self.worker_count
        }

        fn clock(&self) -> Self::Clock {
            self.clock.clone()
        }

        fn spawn_local<F>(&self, _worker_id: usize, f: F)
        where
            F: FnOnce(Self::Spawner<'_>) + Send + 'static,
        {
            let local = Local;
            f(local);
        }
    }
}

// ── Tokio Implementation ───────────────────────────────────────────────────

/// Tokio runtime with single-threaded runtimes per worker
pub mod tokio {
    use super::{Runtime, Spawner};
    use std::future::Future;

    /// Tokio runtime with single-threaded runtimes per worker.
    ///
    /// Each worker is a LocalSet that can run !Send futures.
    #[derive(Clone)]
    pub struct Handle {
        workers: std::sync::Arc<Vec<WorkerHandle>>,
        clock: crate::time::tokio::Clock,
    }

    struct WorkerHandle {
        sender: tokio::sync::mpsc::UnboundedSender<WorkItem>,
    }

    type WorkItem = Box<dyn FnOnce(Local) + Send>;

    impl Handle {
        /// Create a new tokio runtime with the specified number of workers.
        pub fn new(worker_count: usize) -> Self {
            let workers: Vec<_> = (0..worker_count)
                .map(|_| {
                    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<WorkItem>();

                    std::thread::spawn(move || {
                        let rt = tokio::runtime::Builder::new_current_thread()
                            .enable_all()
                            .build()
                            .expect("failed to build tokio runtime");

                        let local = tokio::task::LocalSet::new();

                        local.block_on(&rt, async move {
                            while let Some(work) = rx.recv().await {
                                let spawner = Local;
                                work(spawner);
                            }
                        });
                    });

                    WorkerHandle { sender: tx }
                })
                .collect();

            Self {
                workers: std::sync::Arc::new(workers),
                clock: crate::time::tokio::Clock::default(),
            }
        }
    }

    /// Tokio local spawner that uses spawn_local within a LocalSet.
    pub struct Local;

    impl Spawner for Local {
        fn spawn<F>(&mut self, future: F)
        where
            F: Future<Output = ()> + 'static,
        {
            tokio::task::spawn_local(future);
        }
    }

    impl Runtime for Handle {
        type Clock = crate::time::tokio::Clock;
        type Spawner<'a> = Local;

        fn worker_count(&self) -> usize {
            self.workers.len()
        }

        fn clock(&self) -> Self::Clock {
            self.clock.clone()
        }

        fn spawn_local<F>(&self, worker_id: usize, f: F)
        where
            F: FnOnce(Self::Spawner<'_>) + Send + 'static,
        {
            let work: WorkItem = Box::new(f);
            self.workers[worker_id]
                .sender
                .send(work)
                .expect("worker thread died");
        }
    }
}

/// Wrapper runtime that records per-worker task/channel registrations and can emit DOT graphs.
pub mod inspector {
    use super::{
        ChannelBinding, ChannelDirection, ChannelRegistration, Runtime, Spawner, TaskRegistration,
    };
    use std::{
        collections::BTreeMap,
        sync::{Arc, Mutex},
    };

    #[derive(Clone)]
    pub struct Handle<R: Runtime> {
        inner: R,
        state: Arc<Mutex<State>>,
    }

    #[derive(Default)]
    struct State {
        tasks: Vec<RegisteredTask>,
        channels: Vec<RegisteredChannel>,
        channel_bindings: Vec<RegisteredChannelBinding>,
    }

    #[derive(Clone)]
    struct RegisteredTask {
        worker_id: usize,
        task: TaskRegistration,
    }

    #[derive(Clone)]
    struct RegisteredChannel {
        worker_id: usize,
        channel: ChannelRegistration,
    }

    #[derive(Clone)]
    struct RegisteredChannelBinding {
        worker_id: usize,
        binding: ChannelBinding,
    }

    impl<R: Runtime> Handle<R> {
        pub fn new(inner: R) -> Self {
            Self {
                inner,
                state: Arc::new(Mutex::new(State::default())),
            }
        }

        pub fn to_dot(&self) -> String {
            let state = self.state.lock().expect("inspector lock poisoned");
            let mut out = String::from("digraph pipeline {\n  rankdir=LR;\n  compound=true;\n");

            let mut by_worker: BTreeMap<usize, Vec<_>> = BTreeMap::new();
            for task in &state.tasks {
                by_worker
                    .entry(task.worker_id)
                    .or_default()
                    .push(task.task.clone());
            }

            let mut task_node_ids = std::collections::HashMap::new();
            let mut channel_node_ids = std::collections::HashMap::new();
            for worker_id in 0..self.inner.worker_count() {
                out.push_str(&format!("  subgraph cluster_worker_{worker_id} {{\n"));
                out.push_str(&format!("    label=\"worker {worker_id}\";\n"));
                if let Some(tasks) = by_worker.get(&worker_id) {
                    for (idx, task) in tasks.iter().enumerate() {
                        let node_id = format!("w{worker_id}_t{idx}");
                        task_node_ids.insert((worker_id, task.name.clone()), node_id.clone());
                        let budget = task
                            .budget
                            .map(|v| v.to_string())
                            .unwrap_or_else(|| "none".to_string());
                        let metrics = metric_summary(&task.metrics);
                        let label = format!(
                            "{}\\nfn: {}\\nbudget: {}\\nmetrics: {}\\n{}",
                            task.name, task.function, budget, metrics, task.description
                        );
                        out.push_str(&format!(
                            "    {node_id} [shape=box,label=\"{}\"];\n",
                            escape_dot(&label)
                        ));
                    }
                }
                for (idx, channel) in state
                    .channels
                    .iter()
                    .filter(|channel| channel.worker_id == worker_id)
                    .enumerate()
                {
                    let node_id = format!("w{worker_id}_c{idx}");
                    channel_node_ids
                        .insert((worker_id, channel.channel.name.clone()), node_id.clone());
                    let metrics = metric_summary(&channel.channel.metrics);
                    let label = format!(
                        "{}\\nfn: {}\\nmetrics: {}\\n{}",
                        channel.channel.name,
                        channel.channel.function,
                        metrics,
                        channel.channel.description
                    );
                    out.push_str(&format!(
                        "    {node_id} [shape=ellipse,label=\"{}\"];\n",
                        escape_dot(&label)
                    ));
                }
                out.push_str("  }\n");
            }

            for binding in &state.channel_bindings {
                let worker_id = binding.worker_id;
                let Some(task_node) =
                    task_node_ids.get(&(worker_id, binding.binding.task_name.clone()))
                else {
                    out.push_str(&format!(
                        "  // unresolved binding on worker {worker_id}: missing task '{}'\n",
                        binding.binding.task_name
                    ));
                    continue;
                };
                let Some(channel_node) =
                    channel_node_ids.get(&(worker_id, binding.binding.channel_name.clone()))
                else {
                    out.push_str(&format!(
                        "  // unresolved binding on worker {worker_id}: missing channel '{}'\n",
                        binding.binding.channel_name
                    ));
                    continue;
                };

                let (from, to) = if binding.binding.direction == ChannelDirection::Sends {
                    (task_node, channel_node)
                } else {
                    (channel_node, task_node)
                };
                let label = format!(
                    "{}\\nfn: {}\\n{}",
                    binding.binding.direction,
                    binding.binding.function,
                    binding.binding.description
                );
                out.push_str(&format!(
                    "  {from} -> {to} [label=\"{}\"];\n",
                    escape_dot(&label)
                ));
            }

            out.push_str("}\n");
            out
        }

        pub fn channel_bindings(&self) -> Vec<String> {
            let state = self.state.lock().expect("inspector lock poisoned");
            let mut bindings: Vec<_> = state.channel_bindings.iter().collect();
            bindings.sort_by(|a, b| {
                (
                    a.worker_id,
                    &a.binding.task_name,
                    a.binding.direction as u8,
                    &a.binding.channel_name,
                )
                    .cmp(&(
                        b.worker_id,
                        &b.binding.task_name,
                        b.binding.direction as u8,
                        &b.binding.channel_name,
                    ))
            });
            bindings
                .into_iter()
                .map(|entry| {
                    format!(
                        "worker {}: task '{}' {} on channel '{}'",
                        entry.worker_id,
                        entry.binding.task_name,
                        entry.binding.direction,
                        entry.binding.channel_name
                    )
                })
                .collect()
        }
    }

    fn escape_dot(input: &str) -> String {
        input
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
    }

    fn metric_summary(metrics: &[super::MetricRegistration]) -> String {
        if metrics.is_empty() {
            "none".to_string()
        } else {
            metrics
                .iter()
                .map(|metric| {
                    let variant = metric
                        .variant
                        .as_ref()
                        .map(|variant| format!(" variant={variant}"))
                        .unwrap_or_default();
                    let unit = metric
                        .unit
                        .map(|unit| format!(" unit={unit}"))
                        .unwrap_or_default();
                    format!(
                        "{} [{}{}{}]: {}",
                        metric.label, metric.kind, variant, unit, metric.description
                    )
                })
                .collect::<Vec<_>>()
                .join("\\n")
        }
    }

    impl<R: Runtime> Runtime for Handle<R> {
        type Clock = R::Clock;
        type Spawner<'a> = Local<R::Spawner<'a>>;

        fn worker_count(&self) -> usize {
            self.inner.worker_count()
        }

        fn clock(&self) -> Self::Clock {
            self.inner.clock()
        }

        fn spawn_local<F>(&self, worker_id: usize, f: F)
        where
            F: FnOnce(Self::Spawner<'_>) + Send + 'static,
        {
            let state = self.state.clone();
            self.inner.spawn_local(worker_id, move |spawner| {
                f(Local {
                    inner: spawner,
                    worker_id,
                    state,
                });
            });
        }
    }

    pub struct Local<S> {
        inner: S,
        worker_id: usize,
        state: Arc<Mutex<State>>,
    }

    impl<S: Spawner> Spawner for Local<S> {
        fn spawn<F>(&mut self, future: F)
        where
            F: std::future::Future<Output = ()> + 'static,
        {
            self.inner.spawn(future);
        }

        fn register_task(&mut self, task: TaskRegistration) {
            let mut state = self.state.lock().expect("inspector lock poisoned");
            if let Some(existing) = state.tasks.iter_mut().find(|existing| {
                existing.worker_id == self.worker_id && existing.task.name == task.name
            }) {
                existing.task = task.clone();
            } else {
                state.tasks.push(RegisteredTask {
                    worker_id: self.worker_id,
                    task: task.clone(),
                });
            }
            self.inner.register_task(task);
        }

        fn register_channel(&mut self, channel: ChannelRegistration) {
            let mut state = self.state.lock().expect("inspector lock poisoned");
            if let Some(existing) = state.channels.iter_mut().find(|existing| {
                existing.worker_id == self.worker_id && existing.channel.name == channel.name
            }) {
                existing.channel = channel.clone();
            } else {
                state.channels.push(RegisteredChannel {
                    worker_id: self.worker_id,
                    channel: channel.clone(),
                });
            }
            self.inner.register_channel(channel);
        }

        fn register_channel_binding(&mut self, binding: ChannelBinding) {
            self.state
                .lock()
                .expect("inspector lock poisoned")
                .channel_bindings
                .push(RegisteredChannelBinding {
                    worker_id: self.worker_id,
                    binding: binding.clone(),
                });
            self.inner.register_channel_binding(binding);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokio_runtime_worker_count() {
        let rt = tokio::Handle::new(4);
        assert_eq!(rt.worker_count(), 4);
    }

    #[test]
    fn bach_runtime_worker_count() {
        let rt = bach::Handle::new(4);
        assert_eq!(rt.worker_count(), 4);
    }

    #[test]
    fn inspector_runtime_emits_dot_with_worker_task_and_channel_metadata() {
        let rt = inspector::Handle::new(bach::Handle::new(2));
        let counter_registry = crate::counter::Registry::new();
        let counter_registry_for_worker = counter_registry.clone();

        rt.spawn_local(1, move |mut local| {
            let producer_counter = counter_registry_for_worker.register_task("task.producer");
            local.register_task(
                TaskRegistration::new(
                    "task.producer",
                    "Produces work for downstream pipeline stages",
                    "tests::producer",
                )
                .with_budget(Some(32))
                .with_metric(&producer_counter),
            );

            let consumer_counter = counter_registry_for_worker.register_task("task.consumer");
            local.register_task(
                TaskRegistration::new(
                    "task.consumer",
                    "Consumes work from producer",
                    "tests::consumer",
                )
                .with_metric(&consumer_counter),
            );

            local.register_channel(
                ChannelRegistration::new(
                    "ch.producer_to_consumer",
                    "Unsync queue carrying producer items",
                    "tests::channel",
                )
                .with_metric(
                    &counter_registry_for_worker.register_queue_gauge("q.producer_to_consumer"),
                ),
            );
            local.register_channel_sender(
                "task.producer",
                "ch.producer_to_consumer",
                "Producer enqueues messages",
                "tests::producer",
            );
            local.register_channel_receiver(
                "task.consumer",
                "ch.producer_to_consumer",
                "Consumer dequeues messages",
                "tests::consumer",
            );
        });

        insta::assert_snapshot!(rt.to_dot());
        insta::assert_snapshot!(rt.channel_bindings().join("\n"));
    }
}

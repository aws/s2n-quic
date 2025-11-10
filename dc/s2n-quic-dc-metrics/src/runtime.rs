// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(tokio_unstable), allow(unused))]

pub use crate::task::TaskMonitor;
use crate::{Registry, Unit};
use std::time::Duration;

#[track_caller]
fn delta(
    mut callback: impl FnMut() -> u64 + Send + 'static,
) -> impl FnMut() -> u64 + Send + 'static {
    let mut prev = callback();
    move || {
        let delta = callback() - prev;
        prev += delta;
        delta
    }
}

impl Registry {
    #[cfg(not(tokio_unstable))]
    pub fn instrument_runtime(
        &self,
        _name: &str,
        _runtime: &tokio::runtime::Handle,
        _interval_duration: std::time::Duration,
    ) {
        // no-op -- without tokio_unstable we can't access the runtime metrics
    }

    /// This should only be called once per runtime, as duplicate calls will instrument that
    /// runtime twice (presenting metrics with duplicate values).
    #[cfg(tokio_unstable)]
    pub fn instrument_runtime(
        &self,
        name: &str,
        runtime: &tokio::runtime::Handle,
        interval_duration: std::time::Duration,
    ) {
        let metrics = runtime.metrics();
        let aggregation = Some(format!("Runtime|{name}"));

        {
            let registry = self.clone();
            let aggregation = aggregation.clone();
            let metrics = runtime.metrics();
            runtime.spawn(async move {
                let mut interval = tokio::time::interval(interval_duration);

                // If we can't keep up for some reason, just skip sampling - these metrics don't
                // matter that much.
                interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

                let global_queue_depth = registry.register_summary(
                    "TokioRuntime:GlobalQueueDepth".into(),
                    aggregation.clone(),
                    Unit::Count,
                );
                let blocking_queue_depth = registry.register_summary(
                    "TokioRuntime:BlockingQueueDepth".into(),
                    aggregation.clone(),
                    Unit::Count,
                );
                let worker_local_queue_depth = registry.register_summary(
                    "TokioRuntime:WorkerLocalQueueDepth".into(),
                    aggregation.clone(),
                    Unit::Count,
                );

                loop {
                    global_queue_depth.record_value(metrics.global_queue_depth() as u64);
                    blocking_queue_depth.record_value(metrics.blocking_queue_depth() as u64);

                    for worker in 0..metrics.num_workers() {
                        worker_local_queue_depth
                            .record_value(metrics.worker_local_queue_depth(worker) as u64);
                    }

                    interval.tick().await;
                }
            });
        }

        self.register_list_callback(
            "TokioRuntime:NumWorkers".into(),
            aggregation.clone(),
            Unit::Count,
            {
                let metrics = metrics.clone();
                move || metrics.num_workers()
            },
        );
        self.register_list_callback(
            "TokioRuntime:NumAliveTasks".into(),
            aggregation.clone(),
            Unit::Count,
            {
                let metrics = metrics.clone();
                move || metrics.num_alive_tasks()
            },
        );
        self.register_list_callback(
            "TokioRuntime:NumBlockingThreads".into(),
            aggregation.clone(),
            Unit::Count,
            {
                let metrics = metrics.clone();
                move || metrics.num_blocking_threads()
            },
        );
        self.register_list_callback(
            "TokioRuntime:NumIdleBlockingThreads".into(),
            aggregation.clone(),
            Unit::Count,
            {
                let metrics = metrics.clone();
                move || metrics.num_idle_blocking_threads()
            },
        );
        self.register_list_callback(
            "TokioRuntime:RemoteScheduleCount".into(),
            aggregation.clone(),
            Unit::Count,
            delta({
                let metrics = metrics.clone();
                move || metrics.remote_schedule_count()
            }),
        );
        self.register_list_callback(
            "TokioRuntime:BudgetForcedYieldCount".into(),
            aggregation.clone(),
            Unit::Count,
            delta({
                let metrics = metrics.clone();
                move || metrics.budget_forced_yield_count()
            }),
        );
        self.register_list_callback(
            "TokioRuntime:RegisteredFileDescriptors".into(),
            aggregation.clone(),
            Unit::Count,
            {
                let metrics = metrics.clone();
                move || {
                    metrics.io_driver_fd_registered_count()
                        - metrics.io_driver_fd_deregistered_count()
                }
            },
        );
        self.register_list_callback(
            "TokioRuntime:FileDescriptorRegistrations".into(),
            aggregation.clone(),
            Unit::Count,
            delta({
                let metrics = metrics.clone();
                move || metrics.io_driver_fd_registered_count()
            }),
        );
        self.register_list_callback(
            "TokioRuntime:ReadyEvents".into(),
            aggregation.clone(),
            Unit::Count,
            delta({
                let metrics = metrics.clone();
                move || metrics.io_driver_ready_count()
            }),
        );

        self.register_list_callback(
            "TokioRuntime:IdleWorkerCount".into(),
            aggregation.clone(),
            Unit::Count,
            {
                let metrics = metrics.clone();
                let mut previous = vec![Duration::ZERO; metrics.num_workers()];
                move || {
                    let mut idle = 0;
                    for (idx, prev) in previous.iter_mut().enumerate() {
                        let current = metrics.worker_total_busy_duration(idx);
                        if current
                            .checked_sub(*prev)
                            .is_some_and(|d| d <= Duration::from_millis(1))
                        {
                            idle += 1;
                        }
                        *prev = current;
                    }
                    idle
                }
            },
        );

        for worker in 0..metrics.num_workers() {
            self.register_list_callback(
                "TokioRuntime:WorkerParkCount".into(),
                aggregation.clone(),
                Unit::Count,
                delta({
                    let metrics = metrics.clone();
                    move || metrics.worker_park_count(worker)
                }),
            );
            self.register_list_callback(
                "TokioRuntime:WorkerUnparkCount".into(),
                aggregation.clone(),
                Unit::Count,
                delta({
                    let metrics = metrics.clone();
                    move || metrics.worker_park_count(worker)
                }),
            );
            self.register_list_callback(
                "TokioRuntime:WorkerNoopCount".into(),
                aggregation.clone(),
                Unit::Count,
                delta({
                    let metrics = metrics.clone();
                    move || metrics.worker_noop_count(worker)
                }),
            );
            self.register_list_callback(
                "TokioRuntime:WorkerStealCount".into(),
                aggregation.clone(),
                Unit::Count,
                delta({
                    let metrics = metrics.clone();
                    move || metrics.worker_steal_count(worker)
                }),
            );
            self.register_list_callback(
                "TokioRuntime:WorkerStealOperations".into(),
                aggregation.clone(),
                Unit::Count,
                delta({
                    let metrics = metrics.clone();
                    move || metrics.worker_steal_operations(worker)
                }),
            );
            self.register_list_callback(
                "TokioRuntime:WorkerPollCount".into(),
                aggregation.clone(),
                Unit::Count,
                delta({
                    let metrics = metrics.clone();
                    move || metrics.worker_poll_count(worker)
                }),
            );
            self.register_list_callback(
                "TokioRuntime:WorkerTotalBusyDuration".into(),
                aggregation.clone(),
                Unit::Microsecond,
                delta({
                    let metrics = metrics.clone();
                    move || metrics.worker_total_busy_duration(worker).as_micros() as u64
                }),
            );
            self.register_list_callback(
                "TokioRuntime:WorkerBusyPercent".into(),
                aggregation.clone(),
                Unit::Percent,
                {
                    let metrics = metrics.clone();
                    let callback = move || metrics.worker_total_busy_duration(worker);

                    let mut at = std::time::Instant::now();
                    let mut prev = callback();
                    move || {
                        let new = callback();
                        let delta = new.saturating_sub(prev);
                        let elapsed = at.elapsed();
                        prev = new;
                        at = std::time::Instant::now();

                        // What % of the time since we were last polled did this worker spend busy?
                        delta.div_duration_f32(elapsed) * 100.0
                    }
                },
            );
            self.register_list_callback(
                "TokioRuntime:WorkerLocalScheduleCount".into(),
                aggregation.clone(),
                Unit::Count,
                delta({
                    let metrics = metrics.clone();
                    move || metrics.worker_local_schedule_count(worker)
                }),
            );
            self.register_list_callback(
                "TokioRuntime:WorkerOverflowCount".into(),
                aggregation.clone(),
                Unit::Count,
                delta({
                    let metrics = metrics.clone();
                    move || metrics.worker_overflow_count(worker)
                }),
            );

            self.register_list_callback(
                "TokioRuntime:WorkerMeanPollTime".into(),
                aggregation.clone(),
                Unit::Microsecond,
                {
                    let metrics = metrics.clone();
                    move || metrics.worker_mean_poll_time(worker).as_micros() as u64
                },
            );
        }
    }
}

#[cfg(all(test, tokio_unstable))]
mod test {
    use super::*;

    #[test]
    fn metrics_reported() {
        // The interval at which we sample metrics from the runtime. This is
        // intentionally very long to ensure the underlying sampling loop always
        // runs at most once.
        let runtime_metrics_sample_interval = std::time::Duration::from_secs(3_600);
        let registry = Registry::new();
        let a = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        let b = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();

        registry.instrument_runtime("A", a.handle(), runtime_metrics_sample_interval);
        registry.instrument_runtime("A", b.handle(), runtime_metrics_sample_interval);

        a.block_on(async move {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        });
        b.block_on(async move {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        });

        let line = registry.take_current_metrics_line();

        // We passed `true` for how long to run for, so we'll only run once guaranteed.

        let got = line
            .split(',')
            .map(replace_digits)
            .map(|l| {
                if l.contains("BlockingQueueDepth") {
                    // Sometimes there's a blocking task that sneaks into one of the runtimes. Not
                    // clear what this is, but this prevents the test from failing in that case.
                    l.replace("#*#+#*#", "#*#")
                } else {
                    l.to_owned()
                }
            })
            .collect::<Vec<_>>();
        let expected = [
            "TokioRuntime:BlockingQueueDepth=#*# Runtime|A",
            "TokioRuntime:BudgetForcedYieldCount=#+# Runtime|A",
            "TokioRuntime:FileDescriptorRegistrations=#+# Runtime|A",
            "TokioRuntime:GlobalQueueDepth=#*# Runtime|A",
            "TokioRuntime:IdleWorkerCount=#+# Runtime|A",
            "TokioRuntime:NumAliveTasks=#+# Runtime|A",
            "TokioRuntime:NumBlockingThreads=#+# Runtime|A",
            "TokioRuntime:NumIdleBlockingThreads=#+# Runtime|A",
            "TokioRuntime:NumWorkers=#+# Runtime|A",
            "TokioRuntime:ReadyEvents=#+# Runtime|A",
            "TokioRuntime:RegisteredFileDescriptors=#+# Runtime|A",
            "TokioRuntime:RemoteScheduleCount=#+# Runtime|A",
            "TokioRuntime:WorkerBusyPercent=#+#+#+# % Runtime|A",
            "TokioRuntime:WorkerLocalQueueDepth=#*# Runtime|A",
            "TokioRuntime:WorkerLocalScheduleCount=#+#+#+# Runtime|A",
            "TokioRuntime:WorkerMeanPollTime=#+#+#+# us Runtime|A",
            "TokioRuntime:WorkerNoopCount=#+#+#+# Runtime|A",
            "TokioRuntime:WorkerOverflowCount=#+#+#+# Runtime|A",
            "TokioRuntime:WorkerParkCount=#+#+#+# Runtime|A",
            "TokioRuntime:WorkerPollCount=#+#+#+# Runtime|A",
            "TokioRuntime:WorkerStealCount=#+#+#+# Runtime|A",
            "TokioRuntime:WorkerStealOperations=#+#+#+# Runtime|A",
            "TokioRuntime:WorkerTotalBusyDuration=#+#+#+# us Runtime|A",
            "TokioRuntime:WorkerUnparkCount=#+#+#+# Runtime|A",
        ];

        assert_eq!(
            got, expected,
            "original: {line:?},{got:#?} != {expected:#?}",
        );
    }

    #[test]
    fn task_monitor() {
        let registry = Registry::new();
        let a = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();

        let monitor = registry.register_task_monitor("A");

        assert_eq!(
            registry
                .take_current_metrics_line()
                .split(',')
                .collect::<Vec<_>>(),
            [
                "TokioTask:DroppedTasks=0 Task|A",
                "TokioTask:FirstPollDelay=0 us Task|A",
                "TokioTask:IdleDuration=0 us Task|A",
                "TokioTask:InstrumentedTasks=0 Task|A",
                "TokioTask:PollDuration=0 us Task|A",
                "TokioTask:ScheduledDuration=0 us Task|A"
            ]
        );

        a.block_on(monitor.instrument(async move {
            // Pretend we're a very busy task, avoid yielding back for a while.
            std::thread::sleep(std::time::Duration::from_millis(100));
        }));

        assert_eq!(
            registry
                .take_current_metrics_line()
                .split(',')
                .map(replace_digits)
                .collect::<Vec<_>>(),
            [
                "TokioTask:DroppedTasks=# Task|A",
                "TokioTask:FirstPollDelay=#*# us Task|A",
                "TokioTask:IdleDuration=# us Task|A",
                "TokioTask:InstrumentedTasks=# Task|A",
                "TokioTask:PollDuration=#*# us Task|A",
                "TokioTask:ScheduledDuration=# us Task|A"
            ]
        );
    }

    fn replace_digits(s: &str) -> String {
        let mut replacement = String::with_capacity(s.len());
        for ch in s.chars() {
            if ch.is_ascii_digit() || ch == '.' {
                // Only push # once.
                if replacement.ends_with('#') {
                    continue;
                }
                replacement.push('#');
            } else {
                replacement.push(ch);
            }
        }
        replacement
    }
}

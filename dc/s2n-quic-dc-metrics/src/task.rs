// Copyright (c) 2022 Tokio Contributors
//
// Permission is hereby granted, free of charge, to any
// person obtaining a copy of this software and associated
// documentation files (the "Software"), to deal in the
// Software without restriction, including without
// limitation the rights to use, copy, modify, merge,
// publish, distribute, sublicense, and/or sell copies of
// the Software, and to permit persons to whom the Software
// is furnished to do so, subject to the following
// conditions:
//
// The above copyright notice and this permission notice
// shall be included in all copies or substantial portions
// of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF
// ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED
// TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A
// PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT
// SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
// CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
// OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR
// IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
// DEALINGS IN THE SOFTWARE.
//
// This file is modified from the upstream copy at
// https://github.com/tokio-rs/tokio-metrics/blob/265851a87631a94f571375eca4d5ed6fbeda9f16/src/task.rs
//
// Modifications change the various metrics (especially durations) collected to be reported into
// the s2n-quic-dc-metrics registry as `Summary`s, which ensure that we preserve a more complete
// picture of the values rather than only an average. This is difficult to upstream as our Summary
// type is internal and poorly fits with tokio-metrics lightweight (essentially std-only) public
// API, so for now we've essentially forked this code. It's not expected that this code will
// significantly change over time so this seems OK.

use futures_util::task::{ArcWake, AtomicWaker};
use pin_project_lite::pin_project;
use std::{
    future::Future,
    pin::Pin,
    sync::{
        atomic::{AtomicU64, Ordering::SeqCst},
        Arc,
    },
    task::{Context, Poll},
    time::{Duration, Instant},
};

#[derive(Clone)]
pub struct TaskMonitor {
    metrics: Arc<RawMetrics>,
}

pin_project! {
    /// An async task that has been instrumented with [`TaskMonitor::instrument`].
    pub struct Instrumented<T> {
        // The task being instrumented
        #[pin]
        task: T,

        // True when the task is polled for the first time
        did_poll_once: bool,

        // The instant, tracked as nanoseconds since `instrumented_at`, at which the future finished
        // its last poll.
        idled_at: u64,

        // State shared between the task and its instrumented waker.
        state: Arc<State>,
    }

    impl<T> PinnedDrop for Instrumented<T> {
        fn drop(this: Pin<&mut Self>) {
            this.state.metrics.dropped_count.increment(1);
        }
    }
}

/// Tracks the metrics, shared across the various types.
struct RawMetrics {
    instrumented_count: crate::Counter,
    dropped_count: crate::Counter,
    first_poll_delay: crate::Summary,
    idle_duration: crate::Summary,
    scheduled_duration: crate::Summary,
    poll_duration: crate::Summary,
}

struct State {
    /// Where metrics should be recorded
    metrics: Arc<RawMetrics>,

    /// Instant at which the task was instrumented. This is used to track the time to first poll.
    instrumented_at: Instant,

    /// The instant, tracked as nanoseconds since `instrumented_at`, at which the future
    /// was last woken.
    woke_at: AtomicU64,

    /// Waker to forward notifications to.
    waker: AtomicWaker,
}

impl TaskMonitor {
    pub(crate) fn new(registry: &crate::Registry, aggregation: String) -> TaskMonitor {
        TaskMonitor {
            metrics: Arc::new(RawMetrics {
                instrumented_count: registry.register_counter(
                    "TokioTask:InstrumentedTasks".into(),
                    Some(aggregation.clone()),
                ),
                dropped_count: registry
                    .register_counter("TokioTask:DroppedTasks".into(), Some(aggregation.clone())),
                first_poll_delay: registry.register_summary(
                    "TokioTask:FirstPollDelay".into(),
                    Some(aggregation.clone()),
                    crate::Unit::Microsecond,
                ),
                idle_duration: registry.register_summary(
                    "TokioTask:IdleDuration".into(),
                    Some(aggregation.clone()),
                    crate::Unit::Microsecond,
                ),
                scheduled_duration: registry.register_summary(
                    "TokioTask:ScheduledDuration".into(),
                    Some(aggregation.clone()),
                    crate::Unit::Microsecond,
                ),
                poll_duration: registry.register_summary(
                    "TokioTask:PollDuration".into(),
                    Some(aggregation),
                    crate::Unit::Microsecond,
                ),
            }),
        }
    }

    pub fn instrument<F>(&self, task: F) -> Instrumented<F> {
        self.metrics.instrumented_count.increment(1);
        Instrumented {
            task,
            did_poll_once: false,
            idled_at: 0,
            state: Arc::new(State {
                metrics: self.metrics.clone(),
                instrumented_at: Instant::now(),
                woke_at: AtomicU64::new(0),
                waker: AtomicWaker::new(),
            }),
        }
    }
}

impl<T: Future> Future for Instrumented<T> {
    type Output = T::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        instrument_poll(cx, self, Future::poll)
    }
}

fn instrument_poll<T, Out>(
    cx: &mut Context,
    instrumented: Pin<&mut Instrumented<T>>,
    poll_fn: impl FnOnce(Pin<&mut T>, &mut Context) -> Poll<Out>,
) -> Poll<Out> {
    let poll_start = Instant::now();
    let this = instrumented.project();
    let idled_at = this.idled_at;
    let state = this.state;
    let instrumented_at = state.instrumented_at;
    let metrics = &state.metrics;
    // accounting for time-to-first-poll and tasks-count
    // is this the first time this task has been polled?
    if !*this.did_poll_once {
        // if so, we need to do three things:
        // 1. note that this task *has* been polled
        *this.did_poll_once = true;

        // 2. account for the time-to-first-poll of this task
        let elapsed = poll_start - instrumented_at;
        metrics.first_poll_delay.record_duration(elapsed);
    }
    // accounting for time-idled and time-scheduled
    // 1. note (and reset) the instant this task was last awoke
    let woke_at = state.woke_at.swap(0, SeqCst);
    // The state of a future is *idling* in the interim between the instant
    // it completes a `poll`, and the instant it is next awoken.
    if *idled_at < woke_at {
        // compute the duration of the idle
        let idle_ns = woke_at - *idled_at;

        metrics
            .idle_duration
            .record_duration(Duration::from_nanos(idle_ns));
    }
    // if this task spent any time in the scheduled state after instrumentation,
    // and after first poll, `woke_at` will be greater than 0.
    if woke_at > 0 {
        // recall that the `woke_at` field is internally represented as
        // nanoseconds-since-instrumentation. here, for accounting purposes,
        // we need to instead represent it as a proper `Instant`.
        let woke_instant = instrumented_at + Duration::from_nanos(woke_at);

        // the duration this task spent scheduled is time time elapsed between
        // when this task was awoke, and when it was polled.
        let scheduled = poll_start - woke_instant;

        metrics.scheduled_duration.record_duration(scheduled);
    }
    // Register the waker
    state.waker.register(cx.waker());
    // Get the instrumented waker
    let waker_ref = futures_util::task::waker_ref(state);
    let mut cx = Context::from_waker(&waker_ref);
    // Poll the task
    let inner_poll_start = Instant::now();
    let ret = poll_fn(this.task, &mut cx);
    let inner_poll_end = Instant::now();
    // idle time starts now
    *idled_at = (inner_poll_end - instrumented_at)
        .as_nanos()
        .try_into()
        .unwrap_or(u64::MAX);
    metrics
        .poll_duration
        .record_duration(inner_poll_end - inner_poll_start);
    ret
}

impl State {
    fn on_wake(&self) {
        let woke_at: u64 = match self.instrumented_at.elapsed().as_nanos().try_into() {
            Ok(woke_at) => woke_at,
            // This is highly unlikely as it would mean the task ran for over
            // 500 years. If you ran your service for 500 years. If you are
            // reading this 500 years in the future, I'm sorry.
            Err(_) => return,
        };

        // We don't actually care about the result
        let _ = self.woke_at.compare_exchange(0, woke_at, SeqCst, SeqCst);
    }
}

impl ArcWake for State {
    fn wake_by_ref(arc_self: &Arc<State>) {
        arc_self.on_wake();
        arc_self.waker.wake();
    }

    fn wake(self: Arc<State>) {
        self.on_wake();
        self.waker.wake();
    }
}

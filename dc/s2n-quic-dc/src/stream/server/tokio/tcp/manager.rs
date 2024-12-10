// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    event::{self, EndpointPublisher},
    task::waker,
};
use core::{
    ops::ControlFlow,
    task::{self, Poll, Waker},
    time::Duration,
};
use s2n_quic_core::{
    inet::SocketAddress,
    packet::number::PacketNumberSpace,
    recovery::RttEstimator,
    time::{Clock, Timestamp},
};
use std::{collections::VecDeque, io};
use tracing::trace;

pub struct Manager<W>
where
    W: Worker,
{
    inner: Inner<W>,
    waker_set: waker::Set,
    root_waker: Option<Waker>,
}

/// Split the tasks from the waker set to avoid ownership issues
struct Inner<W>
where
    W: Worker,
{
    /// A set of worker entries which process newly-accepted streams
    workers: Box<[(W, Waker)]>,
    /// FIFO queue for tracking free [`Worker`] entries
    ///
    /// None of the indices in this queue have associated sockets and are waiting to be assigned
    /// for work.
    free: VecDeque<usize>,
    /// A list of [`Worker`] entries that are in order of sojourn time, starting with the oldest.
    ///
    /// The front will be the first to be reclaimed in the case of overload.
    by_sojourn_time: VecDeque<usize>,
    /// Tracks the [sojourn time](https://en.wikipedia.org/wiki/Mean_sojourn_time) of processing
    /// streams in worker entries.
    sojourn_time: RttEstimator,
    /// The number of `by_sojourn_time` list entries that have completed but haven't yet
    /// moved to the `free` list
    gc_count: usize,
}

impl<W> Manager<W>
where
    W: Worker,
{
    #[inline]
    pub fn new(workers: impl IntoIterator<Item = W>) -> Self {
        let mut waker_set = waker::Set::default();
        let workers: Box<[_]> = workers
            .into_iter()
            .enumerate()
            .map(|(idx, worker)| (worker, waker_set.waker(idx)))
            .collect();
        let capacity = workers.len();
        let mut free = VecDeque::with_capacity(capacity);
        free.extend(0..capacity);
        let by_sojourn_time = VecDeque::with_capacity(capacity);

        let inner = Inner {
            workers,
            free,
            by_sojourn_time,
            // set the initial estimate high to avoid backlog churn before we get stable samples
            sojourn_time: RttEstimator::new(Duration::from_secs(30)),
            gc_count: 0,
        };

        Self {
            inner,
            waker_set,
            root_waker: None,
        }
    }

    #[inline]
    pub fn active_slots(&self) -> usize {
        // don't include the pending GC streams
        self.inner.by_sojourn_time.len() - self.inner.gc_count
    }

    #[inline]
    pub fn free_slots(&self) -> usize {
        self.inner.free.len() + self.inner.gc_count
    }

    #[inline]
    pub fn capacity(&self) -> usize {
        self.inner.workers.len()
    }

    #[inline]
    pub fn max_sojourn_time(&self) -> Duration {
        self.inner.max_sojourn_time()
    }

    /// Must be called before polling any workers
    #[inline]
    pub fn update_task_context(&mut self, cx: &mut task::Context) {
        let new_waker = cx.waker();

        let root_task_requires_update = if let Some(waker) = self.root_waker.as_ref() {
            !waker.will_wake(new_waker)
        } else {
            true
        };

        if root_task_requires_update {
            self.waker_set.update_root(new_waker);
            self.root_waker = Some(new_waker.clone());
        }
    }

    #[inline]
    pub fn insert<Pub, C>(
        &mut self,
        remote_address: SocketAddress,
        stream: W::Stream,
        cx: &mut W::Context,
        connection_context: W::ConnectionContext,
        publisher: &Pub,
        clock: &C,
    ) -> bool
    where
        Pub: EndpointPublisher,
        C: Clock,
    {
        let Some(idx) = self.inner.next_worker(clock) else {
            // NOTE: we do not apply back pressure on the listener's `accept` since the aim is to
            // keep that queue as short as possible so we can control the behavior in userspace.
            //
            // TODO: we need to investigate how this interacts with SYN cookies/retries and fast
            // failure modes in kernel space.
            publisher.on_acceptor_tcp_stream_dropped(event::builder::AcceptorTcpStreamDropped {
                remote_address: &remote_address,
                reason: event::builder::AcceptorTcpStreamDropReason::SlotsAtCapacity,
            });
            drop(stream);
            return false;
        };

        self.inner.workers[idx].0.replace(
            remote_address,
            stream,
            connection_context,
            publisher,
            clock,
        );
        self.inner.by_sojourn_time.push_back(idx);

        // kick off the initial poll to register wakers with the socket
        self.inner.poll_worker(idx, cx, publisher, clock);

        true
    }

    #[inline]
    pub fn poll<Pub, C>(
        &mut self,
        cx: &mut W::Context,
        publisher: &Pub,
        clock: &C,
    ) -> ControlFlow<()>
    where
        Pub: EndpointPublisher,
        C: Clock,
    {
        // poll any workers that are ready
        for idx in self.waker_set.drain() {
            if self.inner.poll_worker(idx, cx, publisher, clock).is_break() {
                return ControlFlow::Break(());
            }
        }

        self.inner.invariants();

        ControlFlow::Continue(())
    }
}

impl<W> Inner<W>
where
    W: Worker,
{
    #[inline]
    pub fn max_sojourn_time(&self) -> Duration {
        // if we're double the smoothed sojourn time then the latency is already quite high on the
        // stream - better to accept a new stream at this point
        //
        // FIXME: This currently hardcodes the min/max to try to avoid issues with very fast or
        // very slow clients skewing our behavior too much, but it's not clear what the goal is.
        (self.sojourn_time.smoothed_rtt() * 2).clamp(Duration::from_secs(1), Duration::from_secs(5))
    }

    #[inline]
    fn poll_worker<Pub, C>(
        &mut self,
        idx: usize,
        cx: &mut W::Context,
        publisher: &Pub,
        clock: &C,
    ) -> ControlFlow<()>
    where
        Pub: EndpointPublisher,
        C: Clock,
    {
        let mut cf = ControlFlow::Continue(());

        let (worker, waker) = &mut self.workers[idx];
        let mut task_cx = task::Context::from_waker(waker);
        let Poll::Ready(res) = worker.poll(&mut task_cx, cx, publisher, clock) else {
            return cf;
        };

        match res {
            Ok(ControlFlow::Continue(())) => {
                let now = clock.get_time();
                // update the accept_time estimate
                self.sojourn_time.update_rtt(
                    Duration::ZERO,
                    worker.sojourn_time(&now),
                    now,
                    true,
                    PacketNumberSpace::ApplicationData,
                );
            }
            Ok(ControlFlow::Break(())) => {
                cf = ControlFlow::Break(());
            }
            Err(Some(err)) => publisher
                .on_acceptor_tcp_io_error(event::builder::AcceptorTcpIoError { error: &err }),
            Err(None) => {}
        }

        // the worker is all done so indicate we have another free slot
        self.gc_count += 1;

        cf
    }

    #[inline]
    fn next_worker<C>(&mut self, clock: &C) -> Option<usize>
    where
        C: Clock,
    {
        // if we're out of free workers and GC has been requested, then do a scan
        if self.free.is_empty() && self.gc_count > 0 {
            self.by_sojourn_time.retain(|idx| {
                let idx = *idx;
                let (worker, _waker) = &self.workers[idx];

                // check if the worker is active
                let is_active = worker.is_active();

                // if the worker isn't active it means it's ready to move to the free list
                if !is_active {
                    self.free.push_back(idx);
                }

                is_active
            });
            // we did a full scan so reset the value
            self.gc_count = 0;
        }

        // if we have a free worker then use that
        if let Some(idx) = self.free.pop_front() {
            trace!(op = %"next_worker", free = idx);
            return Some(idx);
        }

        let idx = *self.by_sojourn_time.front().unwrap();
        let sojourn = self.workers[idx].0.sojourn_time(clock);

        // if the worker's sojourn time exceeds the maximum, then reclaim it
        if sojourn >= self.max_sojourn_time() {
            trace!(op = %"next_worker", injected = idx, ?sojourn);
            return self.by_sojourn_time.pop_front();
        }

        trace!(op = %"next_worker", ?sojourn, max_sojourn_time = ?self.max_sojourn_time());

        None
    }

    #[cfg(not(debug_assertions))]
    fn invariants(&self) {}

    #[cfg(debug_assertions)]
    fn invariants(&self) {
        for idx in 0..self.workers.len() {
            let in_ready = self.free.contains(&idx);
            let in_working = self.by_sojourn_time.contains(&idx);
            assert!(
                in_working ^ in_ready,
                "worker should either be in ready ({in_ready}) or working ({in_working}) list"
            );
        }

        for idx in self.free.iter().copied() {
            let (worker, _waker) = &self.workers[idx];
            assert!(!worker.is_active());
        }

        let mut expected_gc_count = 0;

        let mut prev_queue_time = None;
        for idx in self.by_sojourn_time.iter().copied() {
            let (worker, _waker) = &self.workers[idx];

            // if the worker doesn't have a stream then it should be marked for GC
            if !worker.is_active() {
                expected_gc_count += 1;
                continue;
            }

            let queue_time = worker.queue_time();
            if let Some(prev) = prev_queue_time {
                assert!(
                    prev <= queue_time,
                    "front should be oldest; prev={prev:?}, queue_time={queue_time:?}"
                );
            }
            prev_queue_time = Some(queue_time);
        }

        assert_eq!(self.gc_count, expected_gc_count);
    }
}

pub trait Worker {
    type Context;
    type ConnectionContext;
    type Stream;

    fn replace<Pub, C>(
        &mut self,
        remote_address: SocketAddress,
        stream: Self::Stream,
        connection_context: Self::ConnectionContext,
        publisher: &Pub,
        clock: &C,
    ) where
        Pub: EndpointPublisher,
        C: Clock;

    fn poll<Pub, C>(
        &mut self,
        task_cx: &mut task::Context,
        cx: &mut Self::Context,
        publisher: &Pub,
        clock: &C,
    ) -> Poll<Result<ControlFlow<()>, Option<io::Error>>>
    where
        Pub: EndpointPublisher,
        C: Clock;

    #[inline]
    fn sojourn_time<C>(&self, c: &C) -> Duration
    where
        C: Clock,
    {
        c.get_time().saturating_duration_since(self.queue_time())
    }

    fn queue_time(&self) -> Timestamp;

    fn is_active(&self) -> bool;
}

#[cfg(test)]
mod tests {
    use super::{Worker as _, *};
    use crate::event::{self, IntoEvent};
    use bolero::{check, TypeGenerator};
    use core::time::Duration;
    use std::io;

    const WORKER_COUNT: usize = 4;

    #[derive(Clone, Copy, Debug, TypeGenerator)]
    enum Op {
        Insert,
        Wake {
            #[generator(0..WORKER_COUNT)]
            idx: usize,
        },
        Ready {
            #[generator(0..WORKER_COUNT)]
            idx: usize,
            error: bool,
        },
        Advance {
            #[generator(1..=10)]
            millis: u8,
        },
    }

    enum State {
        Idle,
        Active,
        Ready,
        Error(io::ErrorKind),
    }

    struct Worker {
        queue_time: Timestamp,
        state: State,
        epoch: u64,
        poll_count: u64,
    }

    impl Worker {
        fn new<C>(clock: &C) -> Self
        where
            C: Clock,
        {
            Self {
                queue_time: clock.get_time(),
                state: State::Idle,
                epoch: 0,
                poll_count: 0,
            }
        }
    }

    impl super::Worker for Worker {
        type Context = ();
        type ConnectionContext = ();
        type Stream = ();

        fn replace<Pub, C>(
            &mut self,
            _remote_address: SocketAddress,
            _stream: Self::Stream,
            _connection_context: Self::ConnectionContext,
            _publisher: &Pub,
            clock: &C,
        ) where
            Pub: EndpointPublisher,
            C: Clock,
        {
            self.queue_time = clock.get_time();
            self.state = State::Active;
            self.epoch += 1;
            self.poll_count = 0;
        }

        fn poll<Pub, C>(
            &mut self,
            _task_cx: &mut task::Context,
            _cx: &mut Self::Context,
            _publisher: &Pub,
            _clock: &C,
        ) -> Poll<Result<ControlFlow<()>, Option<io::Error>>>
        where
            Pub: EndpointPublisher,
            C: Clock,
        {
            self.poll_count += 1;
            match self.state {
                State::Idle => {
                    unreachable!("shouldn't be polled when idle")
                }
                State::Active => Poll::Pending,
                State::Ready => {
                    self.state = State::Idle;
                    Poll::Ready(Ok(ControlFlow::Continue(())))
                }
                State::Error(err) => {
                    self.state = State::Idle;
                    Poll::Ready(Err(Some(err.into())))
                }
            }
        }

        fn queue_time(&self) -> Timestamp {
            self.queue_time
        }

        fn is_active(&self) -> bool {
            matches!(self.state, State::Active | State::Ready | State::Error(_))
        }
    }

    struct Harness {
        manager: Manager<Worker>,
        clock: Timestamp,
        subscriber: event::tracing::Subscriber,
    }

    impl core::ops::Deref for Harness {
        type Target = Manager<Worker>;

        fn deref(&self) -> &Self::Target {
            &self.manager
        }
    }

    impl core::ops::DerefMut for Harness {
        fn deref_mut(&mut self) -> &mut Self::Target {
            &mut self.manager
        }
    }

    impl Default for Harness {
        fn default() -> Self {
            let clock = unsafe { Timestamp::from_duration(Duration::from_secs(1)) };
            let manager = Manager::<Worker>::new((0..WORKER_COUNT).map(|_| Worker::new(&clock)));
            let subscriber = event::tracing::Subscriber::default();
            Self {
                manager,
                clock,
                subscriber,
            }
        }
    }

    impl Harness {
        pub fn poll(&mut self) {
            self.manager.poll(
                &mut (),
                &publisher(&self.subscriber, &self.clock),
                &self.clock,
            );
        }

        pub fn insert(&mut self) -> bool {
            self.manager.insert(
                SocketAddress::default(),
                (),
                &mut (),
                (),
                &publisher(&self.subscriber, &self.clock),
                &self.clock,
            )
        }

        pub fn wake(&mut self, idx: usize) -> bool {
            let (worker, waker) = &mut self.manager.inner.workers[idx];
            let is_active = worker.is_active();

            if is_active {
                waker.wake_by_ref();
            }

            is_active
        }

        pub fn ready(&mut self, idx: usize) -> bool {
            let (worker, waker) = &mut self.manager.inner.workers[idx];
            let is_active = worker.is_active();

            if is_active {
                worker.state = State::Ready;
                waker.wake_by_ref();
            }

            is_active
        }

        pub fn error(&mut self, idx: usize, error: io::ErrorKind) -> bool {
            let (worker, waker) = &mut self.manager.inner.workers[idx];
            let is_active = worker.is_active();

            if is_active {
                worker.state = State::Error(error);
                waker.wake_by_ref();
            }

            is_active
        }

        pub fn advance(&mut self, time: Duration) {
            self.clock += time;
        }

        #[track_caller]
        pub fn assert_epoch(&self, idx: usize, expected: u64) {
            let (worker, _waker) = &self.manager.inner.workers[idx];
            assert_eq!(worker.epoch, expected);
        }

        #[track_caller]
        pub fn assert_poll_count(&self, idx: usize, expected: u64) {
            let (worker, _waker) = &self.manager.inner.workers[idx];
            assert_eq!(worker.poll_count, expected);
        }
    }

    fn publisher<'a>(
        subscriber: &'a event::tracing::Subscriber,
        clock: &Timestamp,
    ) -> event::EndpointPublisherSubscriber<'a, event::tracing::Subscriber> {
        event::EndpointPublisherSubscriber::new(
            crate::event::builder::EndpointMeta {
                timestamp: clock.into_event(),
            },
            None,
            subscriber,
        )
    }

    #[test]
    fn invariants_test() {
        check!().with_type::<Vec<Op>>().for_each(|ops| {
            let mut harness = Harness::default();

            for op in ops {
                match op {
                    Op::Insert => {
                        harness.insert();
                    }
                    Op::Wake { idx } => {
                        harness.wake(*idx);
                    }
                    Op::Ready { idx, error } => {
                        if *error {
                            harness.error(*idx, io::ErrorKind::ConnectionReset);
                        } else {
                            harness.ready(*idx);
                        }
                    }
                    Op::Advance { millis } => {
                        harness.advance(Duration::from_millis(*millis as u64));
                        harness.poll();
                    }
                }
            }

            harness.poll();
        });
    }

    #[test]
    fn replace_test() {
        let mut harness = Harness::default();
        assert_eq!(harness.active_slots(), 0);
        assert_eq!(harness.capacity(), WORKER_COUNT);

        for idx in 0..4 {
            assert!(harness.insert());
            assert_eq!(harness.active_slots(), 1 + idx);
            harness.assert_epoch(idx, 1);
        }

        // manager should not replace a slot if sojourn_time hasn't passed
        assert!(!harness.insert());

        // advance the clock by max_sojourn_time
        harness.advance(harness.max_sojourn_time());
        harness.poll();
        assert_eq!(harness.active_slots(), WORKER_COUNT);

        for idx in 0..4 {
            assert!(harness.insert());
            assert_eq!(harness.active_slots(), WORKER_COUNT);
            harness.assert_epoch(idx, 2);
        }
    }

    #[test]
    fn wake_test() {
        let mut harness = Harness::default();
        assert!(harness.insert());
        // workers should be polled on insertion
        harness.assert_poll_count(0, 1);
        // workers should not be polled until woken
        harness.poll();
        harness.assert_poll_count(0, 1);

        harness.wake(0);
        harness.assert_poll_count(0, 1);
        harness.poll();
        harness.assert_poll_count(0, 2);
    }

    #[test]
    fn ready_test() {
        let mut harness = Harness::default();

        assert_eq!(harness.active_slots(), 0);
        assert!(harness.insert());
        assert_eq!(harness.active_slots(), 1);
        harness.ready(0);
        assert_eq!(harness.active_slots(), 1);
        harness.poll();
        assert_eq!(harness.active_slots(), 0);
    }
}

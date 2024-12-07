// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::event::{self, EndpointPublisher};
use core::{ops::ControlFlow, task::Poll, time::Duration};
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
    /// A set of worker entries which process newly-accepted streams
    workers: Box<[W]>,
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
    pub fn new(workers: Box<[W]>) -> Self {
        let capacity = workers.len();
        let mut free = VecDeque::with_capacity(capacity);
        free.extend(0..capacity);
        let by_sojourn_time = VecDeque::with_capacity(capacity);

        Self {
            workers,
            free,
            by_sojourn_time,
            // set the initial estimate high to avoid backlog churn before we get stable samples
            sojourn_time: RttEstimator::new(Duration::from_secs(30)),
            gc_count: 0,
        }
    }

    #[inline]
    pub fn active_slots(&self) -> usize {
        // don't include the pending GC streams
        self.by_sojourn_time.len() - self.gc_count
    }

    #[inline]
    pub fn free_slots(&self) -> usize {
        self.free.len() + self.gc_count
    }

    #[inline]
    pub fn capacity(&self) -> usize {
        self.workers.len()
    }

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
    pub fn insert<Pub, C>(
        &mut self,
        remote_address: SocketAddress,
        stream: W::Stream,
        worker_cx: &mut W::Context,
        connection_context: W::ConnectionContext,
        publisher: &Pub,
        clock: &C,
    ) where
        Pub: EndpointPublisher,
        C: Clock,
    {
        let Some(idx) = self.next_worker(clock) else {
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
            return;
        };
        self.workers[idx].replace(remote_address, stream, connection_context, publisher, clock);
        self.by_sojourn_time.push_back(idx);

        // kick off the initial poll to register wakers with the socket
        self.poll_worker(idx, worker_cx, publisher, clock);
    }

    #[inline]
    pub fn poll<T, Pub, C>(
        &mut self,
        tasks: T,
        cx: &mut W::Context,
        publisher: &Pub,
        clock: &C,
    ) -> ControlFlow<()>
    where
        T: IntoIterator<Item = usize>,
        Pub: EndpointPublisher,
        C: Clock,
    {
        // poll any workers that are ready
        for idx in tasks {
            if self.poll_worker(idx, cx, publisher, clock).is_break() {
                return ControlFlow::Break(());
            }
        }

        self.invariants();

        ControlFlow::Continue(())
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

        let worker = &mut self.workers[idx];
        let Poll::Ready(res) = worker.poll(cx, publisher, clock) else {
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
                let worker = &self.workers[idx];

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
        let sojourn = self.workers[idx].sojourn_time(clock);

        // if the worker's sojourn time exceeds the maximum, then reclaim it
        if sojourn > self.max_sojourn_time() {
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
            let worker = &self.workers[idx];
            assert!(!worker.is_active());
        }

        let mut expected_gc_count = 0;

        let mut prev_queue_time = None;
        for idx in self.by_sojourn_time.iter().copied() {
            let worker = &self.workers[idx];

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

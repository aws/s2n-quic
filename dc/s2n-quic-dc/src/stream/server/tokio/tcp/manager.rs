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
use std::io;
use tracing::trace;

mod list;
#[cfg(test)]
mod tests;

use list::List;

pub struct Manager<W>
where
    W: Worker,
{
    inner: Inner<W>,
    waker_set: waker::Set,
}

/// Split the tasks from the waker set to avoid ownership issues
struct Inner<W>
where
    W: Worker,
{
    /// A set of worker entries which process newly-accepted streams
    workers: Box<[Entry<W>]>,
    /// FIFO queue for tracking free [`Worker`] entries
    ///
    /// None of the indices in this queue have associated sockets and are waiting to be assigned
    /// for work.
    free: List,
    /// A list of [`Worker`] entries that are in order of sojourn time, starting with the oldest.
    ///
    /// The front will be the first to be reclaimed in the case of overload.
    by_sojourn_time: List,
    /// Tracks the [sojourn time](https://en.wikipedia.org/wiki/Mean_sojourn_time) of processing
    /// streams in worker entries.
    sojourn_time: RttEstimator,
}

struct Entry<W>
where
    W: Worker,
{
    worker: W,
    waker: Waker,
    link: list::Link,
}

impl<W> AsRef<list::Link> for Entry<W>
where
    W: Worker,
{
    #[inline]
    fn as_ref(&self) -> &list::Link {
        &self.link
    }
}

impl<W> AsMut<list::Link> for Entry<W>
where
    W: Worker,
{
    #[inline]
    fn as_mut(&mut self) -> &mut list::Link {
        &mut self.link
    }
}

impl<W> Manager<W>
where
    W: Worker,
{
    #[inline]
    pub fn new(workers: impl IntoIterator<Item = W>) -> Self {
        let mut waker_set = waker::Set::default();
        let mut workers: Box<[_]> = workers
            .into_iter()
            .enumerate()
            .map(|(idx, worker)| {
                let waker = waker_set.waker(idx);
                let link = list::Link::default();
                Entry {
                    worker,
                    waker,
                    link,
                }
            })
            .collect();
        let capacity = workers.len();
        let mut free = List::default();
        for idx in 0..capacity {
            free.push(&mut workers, idx);
        }

        let by_sojourn_time = List::default();

        let inner = Inner {
            workers,
            free,
            by_sojourn_time,
            // set the initial estimate high to avoid backlog churn before we get stable samples
            sojourn_time: RttEstimator::new(Duration::from_secs(30)),
        };

        Self { inner, waker_set }
    }

    #[inline]
    pub fn active_slots(&self) -> usize {
        self.inner.by_sojourn_time.len()
    }

    #[inline]
    pub fn free_slots(&self) -> usize {
        self.inner.free.len()
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
    pub fn poll_start(&mut self, cx: &mut task::Context) {
        self.waker_set.poll_start(cx);
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

        self.inner.workers[idx].worker.replace(
            remote_address,
            stream,
            connection_context,
            publisher,
            clock,
        );

        self.inner
            .by_sojourn_time
            .push(&mut self.inner.workers, idx);

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
        let ready = self.waker_set.drain();

        // no need to actually poll any workers if none are active
        if self.inner.by_sojourn_time.is_empty() {
            return ControlFlow::Continue(());
        }

        // poll any workers that are ready
        for idx in ready {
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

        let entry = &mut self.workers[idx];
        let mut task_cx = task::Context::from_waker(&entry.waker);
        let Poll::Ready(res) = entry.worker.poll(&mut task_cx, cx, publisher, clock) else {
            debug_assert!(entry.worker.is_active());
            return cf;
        };

        match res {
            Ok(ControlFlow::Continue(())) => {
                let now = clock.get_time();
                // update the accept_time estimate
                self.sojourn_time.update_rtt(
                    Duration::ZERO,
                    entry.worker.sojourn_time(&now),
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
        self.by_sojourn_time.remove(&mut self.workers, idx);
        self.free.push(&mut self.workers, idx);

        cf
    }

    #[inline]
    fn next_worker<C>(&mut self, clock: &C) -> Option<usize>
    where
        C: Clock,
    {
        // if we have a free worker then use that
        if let Some(idx) = self.free.pop(&mut self.workers) {
            trace!(op = %"next_worker", free = idx);
            return Some(idx);
        }

        let idx = self.by_sojourn_time.front().unwrap();
        let sojourn = self.workers[idx].worker.sojourn_time(clock);

        // if the worker's sojourn time exceeds the maximum, then reclaim it
        if sojourn >= self.max_sojourn_time() {
            trace!(op = %"next_worker", injected = idx, ?sojourn);
            return self.by_sojourn_time.pop(&mut self.workers);
        }

        trace!(op = %"next_worker", ?sojourn, max_sojourn_time = ?self.max_sojourn_time());

        None
    }

    #[cfg(not(debug_assertions))]
    fn invariants(&self) {}

    #[cfg(debug_assertions)]
    fn invariants(&self) {
        for idx in 0..self.workers.len() {
            assert!(
                self.free
                    .iter(&self.workers)
                    .chain(self.by_sojourn_time.iter(&self.workers))
                    .filter(|v| *v == idx)
                    .count()
                    == 1,
                "worker {idx} should be linked at all times\n{:?}",
                self.workers[idx].link,
            );
        }

        let mut expected_free_len = 0usize;
        for idx in self.free.iter(&self.workers) {
            let entry = &self.workers[idx];
            assert!(!entry.worker.is_active());
            expected_free_len += 1;
        }
        assert_eq!(self.free.len(), expected_free_len, "{:?}", self.free);

        let mut prev_queue_time = None;
        let mut active_len = 0usize;
        for idx in self.by_sojourn_time.iter(&self.workers) {
            let entry = &self.workers[idx];

            assert!(entry.worker.is_active());
            active_len += 1;

            let queue_time = entry.worker.queue_time();
            if let Some(prev) = prev_queue_time {
                assert!(
                    prev <= queue_time,
                    "front should be oldest; prev={prev:?}, queue_time={queue_time:?}"
                );
            }
            prev_queue_time = Some(queue_time);
        }

        assert_eq!(
            active_len,
            self.by_sojourn_time.len(),
            "{:?}",
            self.by_sojourn_time
        );
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

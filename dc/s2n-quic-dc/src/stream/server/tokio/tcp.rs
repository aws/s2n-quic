// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::accept;
use crate::{
    msg,
    path::secret,
    stream::{
        endpoint,
        environment::{
            tokio::{self as env, Environment},
            Environment as _,
        },
        server,
        socket::Socket,
    },
};
use core::{
    future::poll_fn,
    ops::ControlFlow,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};
use s2n_quic_core::{
    ensure,
    packet::number::PacketNumberSpace,
    ready,
    recovery::RttEstimator,
    time::{Clock, Timestamp},
};
use std::{collections::VecDeque, io};
use tokio::{
    io::AsyncWrite as _,
    net::{TcpListener, TcpStream},
};
use tracing::{debug, error, trace};

pub struct Acceptor {
    sender: accept::Sender,
    socket: TcpListener,
    env: Environment,
    secrets: secret::Map,
    backlog: usize,
    accept_flavor: accept::Flavor,
}

impl Acceptor {
    #[inline]
    pub fn new(
        socket: TcpListener,
        sender: &accept::Sender,
        env: &Environment,
        secrets: &secret::Map,
        backlog: usize,
        accept_flavor: accept::Flavor,
    ) -> Self {
        Self {
            sender: sender.clone(),
            socket,
            env: env.clone(),
            secrets: secrets.clone(),
            backlog,
            accept_flavor,
        }
    }

    pub async fn run(self) {
        let drop_guard = DropLog;
        let mut fresh = FreshQueue::new(&self);
        let mut workers = WorkerSet::new(&self);
        let mut context = WorkerContext::new(&self);

        poll_fn(move |cx| {
            let now = self.env.clock().get_time();

            fresh.fill(cx, &self.socket);
            trace!(accepted_backlog = fresh.len());

            for socket in fresh.drain() {
                workers.push(socket, now);
            }

            trace!(pre_worker_count = workers.working.len());
            let res = workers.poll(cx, &mut context, now);
            trace!(post_worker_count = workers.working.len());

            workers.invariants();

            if res.is_continue() {
                Poll::Pending
            } else {
                Poll::Ready(())
            }
        })
        .await;

        drop(drop_guard);
    }
}

/// Converts the kernel's TCP FIFO accept queue to LIFO
///
/// This should produce overall better latencies in the case of overloaded queues.
struct FreshQueue {
    queue: VecDeque<TcpStream>,
}

impl FreshQueue {
    fn new(acceptor: &Acceptor) -> Self {
        Self {
            queue: VecDeque::with_capacity(acceptor.backlog),
        }
    }

    fn fill(&mut self, cx: &mut Context, listener: &TcpListener) {
        // Allow draining the queue twice the capacity
        //
        // The idea here is to try and reduce the number of connections in the kernel's queue while
        // bounding the amount of work we do in userspace.
        //
        // TODO: investigate getting the current length and dropping the front of the queue rather
        // than pop/push with the userspace queue
        let mut remaining = self.queue.capacity() * 2;

        while let Poll::Ready(res) = listener.poll_accept(cx) {
            match res {
                Ok((socket, _remote_addr)) => {
                    if self.queue.len() == self.queue.capacity() {
                        let _ = self.queue.pop_back();
                        trace!("fresh backlog too full; dropping stream");
                    }
                    // most recent streams go to the front of the line, since they're the most
                    // likely to be successfully processed
                    self.queue.push_front(socket);
                }
                Err(err) => {
                    // TODO submit to a separate error channel that the application can subscribe
                    // to
                    error!(listener_error = %err);
                }
            }

            remaining -= 1;

            if remaining == 0 {
                return;
            }
        }
    }

    fn len(&self) -> usize {
        self.queue.len()
    }

    fn drain(&mut self) -> impl Iterator<Item = TcpStream> + '_ {
        self.queue.drain(..)
    }
}

struct WorkerSet {
    /// A set of worker entries which process newly-accepted streams
    workers: Box<[Worker]>,
    /// FIFO queue for tracking free [`Worker`] entries
    ///
    /// None of the indices in this queue have associated sockets and are waiting to be assigned
    /// for work.
    free: VecDeque<usize>,
    /// A list of [`Worker`] entries that are currently processing a socket
    ///
    /// This list is ordered by sojourn time, where the front of the list is the oldest. The front
    /// will be the first to be reclaimed in the case of overload.
    working: VecDeque<usize>,
    /// Tracks the [sojourn time](https://en.wikipedia.org/wiki/Mean_sojourn_time) of processing
    /// streams in worker entries.
    sojourn_time: RttEstimator,
}

impl WorkerSet {
    #[inline]
    pub fn new(acceptor: &Acceptor) -> Self {
        let backlog = acceptor.backlog;
        let mut workers = Vec::with_capacity(backlog);
        let mut free = VecDeque::with_capacity(backlog);
        let now = acceptor.env.clock().get_time();
        for idx in 0..backlog {
            workers.push(Worker::new(now));
            free.push_back(idx);
        }
        Self {
            workers: workers.into(),
            free,
            working: VecDeque::with_capacity(backlog),
            // set the initial estimate high to avoid backlog churn before we get stable samples
            sojourn_time: RttEstimator::new(Duration::from_secs(30)),
        }
    }

    #[inline]
    pub fn push(&mut self, stream: TcpStream, now: Timestamp) {
        let Some(idx) = self.next_worker(now) else {
            // NOTE: we do not apply back pressure on the listener's `accept` since the aim is to
            // keep that queue as short as possible so we can control the behavior in userspace.
            //
            // TODO: we need to investigate how this interacts with SYN cookies/retries and fast
            // failure modes in kernel space.
            trace!(
                "could not find an available worker; dropping stream {:?}",
                stream
            );
            drop(stream);
            return;
        };
        self.workers[idx].push(stream, now);
        self.working.push_back(idx);
    }

    #[inline]
    pub fn poll(
        &mut self,
        cx: &mut Context,
        worker_cx: &mut WorkerContext,
        now: Timestamp,
    ) -> ControlFlow<()> {
        let mut cf = ControlFlow::Continue(());

        self.working.retain(|&idx| {
            let worker = &mut self.workers[idx];
            let Poll::Ready(res) = worker.poll(cx, worker_cx, now) else {
                // keep processing it
                return true;
            };

            match res {
                Ok(ControlFlow::Continue(())) => {
                    // update the accept_time estimate
                    let sample = worker.sojourn(now);
                    trace!(sojourn_sample = ?sample);

                    self.sojourn_time.update_rtt(
                        Duration::ZERO,
                        worker.sojourn(now),
                        now,
                        true,
                        PacketNumberSpace::ApplicationData,
                    );
                }
                Ok(ControlFlow::Break(())) => {
                    cf = ControlFlow::Break(());
                }
                Err(err) => {
                    debug!(accept_stream_error = %err);
                }
            }

            // the worker is done so remove it from the working queue
            self.free.push_back(idx);
            false
        });

        cf
    }

    #[inline]
    fn next_worker(&mut self, now: Timestamp) -> Option<usize> {
        // if we have a free worker then use that
        if let Some(idx) = self.free.pop_front() {
            trace!(op = %"next_worker", free = idx);
            return Some(idx);
        }

        let idx = *self.working.front().unwrap();
        let sojourn = self.workers[idx].sojourn(now);

        // if the worker's sojourn time exceeds the maximum, then reclaim it
        if sojourn > self.max_sojourn_time() {
            trace!(op = %"next_worker", injected = idx, ?sojourn);
            return self.working.pop_front();
        }

        trace!(op = %"next_worker", ?sojourn, max_sojourn_time = ?self.max_sojourn_time());

        None
    }

    #[inline]
    fn max_sojourn_time(&self) -> Duration {
        // if we're double the smoothed sojourn time then the latency is already quite high on the
        // stream - better to accept a new stream at this point
        //
        // FIXME: This currently hardcodes the min/max to try to avoid issues with very fast or
        // very slow clients skewing our behavior too much, but it's not clear what the goal is.
        (self.sojourn_time.smoothed_rtt() * 2).clamp(Duration::from_secs(1), Duration::from_secs(5))
    }

    #[cfg(not(debug_assertions))]
    fn invariants(&self) {}

    #[cfg(debug_assertions)]
    fn invariants(&self) {
        for idx in 0..self.workers.len() {
            let in_ready = self.free.contains(&idx);
            let in_working = self.working.contains(&idx);
            assert!(
                in_working ^ in_ready,
                "worker should either be in ready ({in_ready}) or working ({in_working}) list"
            );
        }

        for idx in self.free.iter().copied() {
            let worker = &self.workers[idx];
            assert!(worker.stream.is_none());
            assert!(
                matches!(worker.state, WorkerState::Init),
                "actual={:?}",
                worker.state
            );
        }

        let mut prev_queue_time = None;
        for idx in self.working.iter().copied() {
            let worker = &self.workers[idx];
            assert!(worker.stream.is_some());
            let queue_time = worker.queue_time;
            if let Some(prev) = prev_queue_time {
                assert!(
                    prev <= queue_time,
                    "front should be oldest; prev={prev:?}, queue_time={queue_time:?}"
                );
            }
            prev_queue_time = Some(queue_time);
        }
    }
}

struct WorkerContext {
    recv_buffer: msg::recv::Message,
    sender: accept::Sender,
    env: Environment,
    secrets: secret::Map,
    accept_flavor: accept::Flavor,
}

impl WorkerContext {
    fn new(acceptor: &Acceptor) -> Self {
        Self {
            recv_buffer: msg::recv::Message::new(u16::MAX),
            sender: acceptor.sender.clone(),
            env: acceptor.env.clone(),
            secrets: acceptor.secrets.clone(),
            accept_flavor: acceptor.accept_flavor,
        }
    }
}

struct Worker {
    queue_time: Timestamp,
    stream: Option<TcpStream>,
    state: WorkerState,
}

impl Worker {
    pub fn new(now: Timestamp) -> Self {
        Self {
            queue_time: now,
            stream: None,
            state: WorkerState::Init,
        }
    }

    #[inline]
    pub fn push(&mut self, stream: TcpStream, now: Timestamp) {
        self.queue_time = now;
        let prev_state = core::mem::replace(&mut self.state, WorkerState::Init);
        let prev = core::mem::replace(&mut self.stream, Some(stream));

        if prev.is_some() {
            trace!(worker_prev_state = ?prev_state);
        }
    }

    #[inline]
    pub fn poll(
        &mut self,
        cx: &mut Context,
        context: &mut WorkerContext,
        now: Timestamp,
    ) -> Poll<io::Result<ControlFlow<()>>> {
        // if we don't have a stream then it's a bug in the worker impl - in production just return
        // `Ready`, which will correct the state
        if self.stream.is_none() {
            debug_assert!(
                false,
                "Worker::poll should only be called with an active socket"
            );
            return Poll::Ready(Ok(ControlFlow::Continue(())));
        }

        // make sure another worker didn't leave around a buffer
        context.recv_buffer.clear();

        let res = ready!(self
            .state
            .poll(cx, context, &mut self.stream, self.queue_time, now));

        // if we're ready then reset the worker
        self.state = WorkerState::Init;
        self.stream = None;

        Poll::Ready(res)
    }

    /// Returns the duration that the worker has been processing a stream
    #[inline]
    pub fn sojourn(&self, now: Timestamp) -> Duration {
        now.saturating_duration_since(self.queue_time)
    }
}

#[derive(Debug)]
enum WorkerState {
    /// Worker is waiting for a packet
    Init,
    /// Worker received a partial packet and is waiting on more data
    Buffering(msg::recv::Message),
    /// Worker encountered an error and is trying to send a response
    Erroring {
        offset: usize,
        buffer: Vec<u8>,
        error: io::Error,
    },
}

impl WorkerState {
    fn poll(
        &mut self,
        cx: &mut Context,
        context: &mut WorkerContext,
        stream: &mut Option<TcpStream>,
        queue_time: Timestamp,
        now: Timestamp,
    ) -> Poll<io::Result<ControlFlow<()>>> {
        loop {
            // figure out where to put the received bytes
            let (recv_buffer, recv_buffer_owned) = match self {
                // borrow the context's recv buffer initially
                WorkerState::Init => (&mut context.recv_buffer, false),
                // we have our own recv buffer to use
                WorkerState::Buffering(recv_buffer) => (recv_buffer, true),
                // we encountered an error so try and send it back
                WorkerState::Erroring { offset, buffer, .. } => {
                    let stream = Pin::new(stream.as_mut().unwrap());
                    let len = ready!(stream.poll_write(cx, &buffer[*offset..]))?;

                    *offset += len;

                    // if we still need to send part of the buffer then loop back around
                    if *offset < buffer.len() {
                        continue;
                    }

                    // io::Error doesn't implement clone so we have to take the error to return it
                    let WorkerState::Erroring { error, .. } = core::mem::replace(self, Self::Init)
                    else {
                        unreachable!()
                    };

                    return Err(error).into();
                }
            };

            // try to read an initial packet from the socket
            let res = Self::poll_initial_packet(cx, stream.as_mut().unwrap(), recv_buffer);

            let Poll::Ready(res) = res else {
                // if we got `Pending` but we don't own the recv buffer then we need to copy it
                // into the worker so we can resume where we left off last time
                if !recv_buffer_owned {
                    *self = WorkerState::Buffering(recv_buffer.take());
                };

                return Poll::Pending;
            };

            let initial_packet = res?;

            debug!(?initial_packet);

            let stream_builder = match endpoint::accept_stream(
                &context.env,
                env::TcpReregistered(stream.take().unwrap()),
                &initial_packet,
                None,
                Some(recv_buffer),
                &context.secrets,
                None,
            ) {
                Ok(stream) => stream,
                Err(error) => {
                    if let Some(env::TcpReregistered(socket)) = error.peer {
                        if !error.secret_control.is_empty() {
                            // if we need to send an error then update the state and loop back
                            // around
                            *stream = Some(socket);
                            *self = WorkerState::Erroring {
                                offset: 0,
                                buffer: error.secret_control,
                                error: error.error,
                            };
                            continue;
                        }
                    }
                    return Err(error.error).into();
                }
            };

            trace!(
                enqueue_stream = ?stream_builder.shared.remote_ip(),
                sojourn_time = ?now.saturating_duration_since(queue_time),
            );

            let item = (stream_builder, queue_time);
            let res = match context.accept_flavor {
                accept::Flavor::Fifo => context.sender.send_back(item),
                accept::Flavor::Lifo => context.sender.send_front(item),
            };

            return Poll::Ready(Ok(match res {
                Ok(prev) => {
                    if let Some((stream, queue_time)) = prev {
                        debug!(
                            event = "accept::prune",
                            credentials = ?stream.shared.credentials(),
                            queue_duration = ?now.saturating_duration_since(queue_time),
                        );
                    }
                    ControlFlow::Continue(())
                }
                Err(_err) => {
                    debug!("application accept queue dropped; shutting down");
                    ControlFlow::Break(())
                }
            }));
        }
    }

    #[inline]
    fn poll_initial_packet(
        cx: &mut Context,
        stream: &mut TcpStream,
        recv_buffer: &mut msg::recv::Message,
    ) -> Poll<io::Result<server::InitialPacket>> {
        loop {
            ensure!(
                recv_buffer.payload_len() < 10_000,
                Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "prelude did not come in the first 10k bytes"
                ))
                .into()
            );

            let res = ready!(stream.poll_recv_buffer(cx, recv_buffer))?;

            match server::InitialPacket::peek(recv_buffer, 16) {
                Ok(packet) => {
                    return Ok(packet).into();
                }
                Err(s2n_codec::DecoderError::UnexpectedEof(_)) => {
                    // If at end of the stream, we're not going to succeed. End early.
                    if res == 0 {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "insufficient data in prelude before EOF",
                        ))
                        .into();
                    }
                    // we don't have enough bytes buffered so try reading more
                    continue;
                }
                Err(err) => {
                    return Err(io::Error::new(io::ErrorKind::InvalidData, err.to_string())).into();
                }
            }
        }
    }
}

struct DropLog;

impl Drop for DropLog {
    #[inline]
    fn drop(&mut self) {
        debug!("acceptor task has been dropped");
    }
}

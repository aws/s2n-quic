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

use crate::{counter, time::precision};
use s2n_quic_core::time;
use std::future::Future;

/// Abstraction over a task runtime and its associated clock.
///
/// Each runtime implementation bundles both spawning capability and the clock
/// appropriate for that execution model (e.g. busy-poll timers that don't use wakers,
/// tokio timers backed by the tokio runtime, bach simulated time).
pub trait Runtime: Clone + 'static {
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

    /// Spawn a named !Send future on the current worker.
    ///
    /// This allows runtimes to attach a debug name to the spawned task.
    /// Default implementation ignores the name and calls spawn().
    #[inline]
    fn spawn_named<F>(&mut self, _name: &str, future: F)
    where
        F: Future<Output = ()> + 'static,
    {
        self.spawn(future);
    }

    /// Returns the worker ID for this spawner.
    ///
    /// This is used for runtime introspection and topology tracking.
    fn worker_id(&self) -> usize;

    /// Convenience helper for pipeline tasks with budget and task counters.
    ///
    /// Calls on_spawn with the budget and worker ID (which registers the task in topology),
    /// then spawns the task with its name.
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
        let worker_id = self.worker_id();
        task_counter.on_spawn(budget, worker_id);
        let task_name =
            task_counter.with_registration_metadata_ref(|name, _, _, _| name.to_string());
        self.spawn_named(&task_name, future);
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

        fn spawn_named<F>(&mut self, name: &str, future: F)
        where
            F: Future<Output = ()> + 'static,
        {
            self.spawn_with_priority_and_name(future, None, Some(name.to_string()));
        }

        fn worker_id(&self) -> usize {
            self.worker_id
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
    pub struct Local {
        worker_id: usize,
    }

    impl Local {
        pub fn new(worker_id: usize) -> Self {
            Self { worker_id }
        }
    }

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

        fn spawn_named<F>(&mut self, name: &str, future: F)
        where
            F: Future<Output = ()> + 'static,
        {
            ::bach::task::spawn_named(SendWrapper(future), name);
        }

        fn worker_id(&self) -> usize {
            self.worker_id
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

        fn spawn_local<F>(&self, worker_id: usize, f: F)
        where
            F: FnOnce(Self::Spawner<'_>) + Send + 'static,
        {
            let local = Local { worker_id };
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
                .map(|worker_id| {
                    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<WorkItem>();

                    std::thread::spawn(move || {
                        let rt = tokio::runtime::Builder::new_current_thread()
                            .enable_all()
                            .build()
                            .expect("failed to build tokio runtime");

                        let local = tokio::task::LocalSet::new();

                        local.block_on(&rt, async move {
                            while let Some(work) = rx.recv().await {
                                let spawner = Local { worker_id };
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
    pub struct Local {
        worker_id: usize,
    }

    impl Spawner for Local {
        fn spawn<F>(&mut self, future: F)
        where
            F: Future<Output = ()> + 'static,
        {
            tokio::task::spawn_local(future);
        }

        fn spawn_named<F>(&mut self, name: &str, future: F)
        where
            F: Future<Output = ()> + 'static,
        {
            // Tokio spawn_local doesn't support names directly.
            // We could log the name or use it for debugging.
            let _ = name;
            self.spawn(future);
        }

        fn worker_id(&self) -> usize {
            self.worker_id
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

/// Wrapper runtime that holds spawned futures to keep them alive.
///
/// This is a simple container that prevents futures from being dropped.
/// Use the counter::Registry::topology() function to get the graph of the pipeline
/// after using the inspector runtime.
pub mod inspector {
    use crate::{
        endpoint::{self, Config, WorkerLayout},
        time::precision,
    };
    use core::pin::Pin;
    use std::{
        future::Future,
        net::{IpAddr, Ipv4Addr, SocketAddr},
        sync::{Arc, Mutex},
    };

    /// Inspector runtime for topology introspection.
    ///
    /// Spawned tasks are recorded without being executed.
    #[derive(Clone)]
    pub struct Handle {
        worker_count: usize,
        clock: Clock,
        futures: Arc<Mutex<Vec<Pin<Box<dyn Future<Output = ()> + 'static>>>>>,
    }

    /// Simple clock for inspector runtime.
    #[derive(Clone, Debug, Default)]
    pub struct Clock;

    /// Simple timer for inspector runtime.
    #[derive(Clone, Debug, Default)]
    pub struct Timer {
        armed: bool,
    }

    pub struct Local {
        worker_id: usize,
        futures: Arc<Mutex<Vec<Pin<Box<dyn Future<Output = ()> + 'static>>>>>,
    }

    #[derive(Clone, Copy)]
    struct PanicSendSocket {
        addr: SocketAddr,
    }

    #[derive(Clone, Copy)]
    struct PanicRecvSocket {
        addr: SocketAddr,
    }

    impl Handle {
        pub fn new(worker_count: usize) -> Self {
            Self {
                worker_count: worker_count.max(1),
                clock: Clock,
                futures: Arc::new(Mutex::new(Vec::new())),
            }
        }

        /// Spawn a future and hold it to prevent it from being dropped.
        pub fn spawn<F>(&self, future: F)
        where
            F: Future<Output = ()> + 'static,
        {
            self.futures
                .lock()
                .expect("inspector future lock poisoned")
                .push(Box::pin(future));
        }

        /// Get the number of futures currently held.
        pub fn future_count(&self) -> usize {
            self.futures
                .lock()
                .expect("inspector future lock poisoned")
                .len()
        }
    }

    /// Builds an endpoint with panic-only sockets and returns the resulting topology.
    ///
    /// This is intended for topology introspection without creating real sockets or executing
    /// runtime tasks.
    pub fn endpoint_topology(
        config: Config,
        send_socket_count: usize,
        recv_socket_count: usize,
    ) -> crate::counter::Topology {
        let worker_count = required_worker_count(&config.layout);
        let runtime = Handle::new(worker_count);
        let send_sockets = (0..send_socket_count)
            .map(|idx| PanicSendSocket {
                addr: socket_addr(10_000, idx),
            })
            .collect();
        let recv_sockets = (0..recv_socket_count)
            .map(|idx| PanicRecvSocket {
                addr: socket_addr(20_000, idx),
            })
            .collect();
        endpoint::setup_endpoint(runtime, config, send_sockets, recv_sockets)
            .counters
            .topology()
    }

    impl Default for Handle {
        fn default() -> Self {
            Self::new(1)
        }
    }

    impl super::Spawner for Local {
        fn spawn<F>(&mut self, future: F)
        where
            F: Future<Output = ()> + 'static,
        {
            self.futures
                .lock()
                .expect("inspector future lock poisoned")
                .push(Box::pin(future));
        }

        fn worker_id(&self) -> usize {
            self.worker_id
        }
    }

    impl super::Runtime for Handle {
        type Clock = Clock;
        type Spawner<'a> = Local;

        fn worker_count(&self) -> usize {
            self.worker_count
        }

        fn clock(&self) -> Self::Clock {
            self.clock.clone()
        }

        fn spawn_local<F>(&self, worker_id: usize, f: F)
        where
            F: FnOnce(Self::Spawner<'_>) + Send + 'static,
        {
            f(Local {
                worker_id,
                futures: self.futures.clone(),
            });
        }
    }

    impl s2n_quic_core::time::Clock for Clock {
        fn get_time(&self) -> s2n_quic_core::time::Timestamp {
            // SAFETY: Duration::ZERO is always a valid non-negative timestamp origin.
            unsafe { s2n_quic_core::time::Timestamp::from_duration(core::time::Duration::ZERO) }
        }
    }

    impl precision::Clock for Clock {
        type Timer = Timer;

        fn now(&self) -> precision::Timestamp {
            precision::Timestamp { nanos: 0 }
        }

        fn timer(&self) -> Self::Timer {
            Timer { armed: false }
        }
    }

    impl precision::Timer for Timer {
        fn now(&self) -> precision::Timestamp {
            precision::Timestamp { nanos: 0 }
        }

        async fn sleep_until(&mut self, _target: precision::Timestamp) {}

        fn poll_ready(&mut self, _cx: &mut core::task::Context) -> core::task::Poll<()> {
            core::task::Poll::Pending
        }

        fn update(&mut self, _target: precision::Timestamp) {
            self.armed = true;
        }

        fn cancel(&mut self) {
            self.armed = false;
        }

        fn is_armed(&self) -> bool {
            self.armed
        }
    }

    impl crate::socket::send::Socket for PanicSendSocket {
        fn send_msg(
            &self,
            _addr: &crate::msg::addr::Addr,
            _payload: &[std::io::IoSlice],
            _segment_size: u16,
            _ecn: s2n_quic_core::inet::ExplicitCongestionNotification,
        ) -> std::io::Result<usize> {
            panic!("send_msg should not be called during topology snapshot");
        }

        fn local_addr(&self) -> std::io::Result<SocketAddr> {
            Ok(self.addr)
        }
    }

    impl crate::socket::recv::Socket for PanicRecvSocket {
        fn poll_recv(
            &self,
            _cx: &mut core::task::Context,
            _addr: &mut crate::msg::addr::Addr,
            _cmsg: &mut crate::msg::cmsg::Receiver,
            _buffer: &mut [std::io::IoSliceMut],
        ) -> core::task::Poll<std::io::Result<usize>> {
            panic!("poll_recv should not be called during topology snapshot");
        }

        fn local_addr(&self) -> std::io::Result<SocketAddr> {
            Ok(self.addr)
        }
    }

    fn required_worker_count(layout: &WorkerLayout) -> usize {
        let max = layout
            .send
            .iter()
            .chain(layout.recv_io.iter())
            .chain(layout.recv_dispatch.iter())
            .chain(layout.waker_drain.iter())
            .chain(core::iter::once(&layout.frame_dispatch))
            .chain(core::iter::once(&layout.background))
            .copied()
            .max()
            .expect("worker layout should not be empty");
        max + 1
    }

    fn socket_addr(base_port: u16, idx: usize) -> SocketAddr {
        let offset = u16::try_from(idx).expect("socket index exceeds u16::MAX (65535)");
        let port = base_port
            .checked_add(offset)
            .expect("port calculation overflows u16::MAX (base_port + index exceeds 65535)");
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::{
        pin::Pin,
        task::{Context, Poll},
    };
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

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
    fn inspector_holds_futures() {
        let inspector = inspector::Handle::new(1);
        assert_eq!(inspector.future_count(), 0);

        // Spawn some futures
        inspector.spawn(async {});
        inspector.spawn(async {});
        inspector.spawn(async {});

        assert_eq!(inspector.future_count(), 3);
    }

    #[test]
    fn inspector_spawn_does_not_drop_future() {
        let inspector = inspector::Handle::new(1);
        let drop_count = Arc::new(AtomicUsize::new(0));
        inspector.spawn(DropOnDropFuture::new(drop_count.clone()));

        assert_eq!(inspector.future_count(), 1);
        assert_eq!(drop_count.load(Ordering::Relaxed), 0);
    }

    struct DropOnDropFuture {
        drop_count: Arc<AtomicUsize>,
    }

    impl DropOnDropFuture {
        fn new(drop_count: Arc<AtomicUsize>) -> Self {
            Self { drop_count }
        }
    }

    impl Future for DropOnDropFuture {
        type Output = ();

        fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
            Poll::Pending
        }
    }

    impl Drop for DropOnDropFuture {
        fn drop(&mut self) {
            self.drop_count.fetch_add(1, Ordering::Relaxed);
        }
    }
}

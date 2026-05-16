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

use crate::time::precision;
use s2n_quic_core::time;
use std::future::Future;

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
}

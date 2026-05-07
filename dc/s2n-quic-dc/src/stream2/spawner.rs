// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Runtime spawner abstraction for stream2
//!
//! This provides a generic interface for spawning tasks across different runtimes
//! (busy-poll, tokio, bach) while respecting worker affinity for non-Send types.
//!
//! The key challenge is that busy-poll uses a two-phase spawn pattern:
//! 1. Call `handle.spawn_local(|spawner| { ... })` with a Send closure
//! 2. Inside that closure, use `spawner.spawn(future)` to spawn !Send futures
//!
//! This abstraction needs to support both this pattern and simpler runtimes like tokio.

use std::future::Future;

/// Abstraction for spawning tasks on a runtime
///
/// Supports both Send futures (can run anywhere) and !Send futures (need worker affinity).
pub trait Spawner: Clone + Send + 'static {
    /// The worker-local spawner type for spawning !Send futures
    type Local<'a>: LocalSpawner;

    /// Number of workers in this runtime
    fn worker_count(&self) -> usize;

    /// Spawn !Send futures on a specific worker
    ///
    /// The closure receives a spawner handle that can spawn !Send futures.
    /// This matches the busy-poll pattern where you call spawn_local with a Send closure
    /// that receives a Spawner to spawn !Send futures.
    fn spawn_local<F>(&self, worker_id: usize, f: F)
    where
        F: FnOnce(Self::Local<'_>) + Send + 'static;
}

/// Handle for spawning !Send futures within a worker-local context
pub trait LocalSpawner {
    /// Spawn a !Send future on the current worker
    fn spawn<F>(&mut self, future: F)
    where
        F: Future<Output = ()> + 'static;
}

// ── BusyPoll Implementation ────────────────────────────────────────────────

/// Implementations for busy_poll runtime
pub mod busy_poll {
    use super::{LocalSpawner, Spawner};
    use std::future::Future;

    impl Spawner for crate::busy_poll::Pool {
        type Local<'a> = crate::busy_poll::Spawner<'a>;

        fn worker_count(&self) -> usize {
            self.len()
        }

        fn spawn_local<F>(&self, worker_id: usize, f: F)
        where
            F: FnOnce(Self::Local<'_>) + Send + 'static,
        {
            self[worker_id].spawn_local(f);
        }
    }

    impl LocalSpawner for crate::busy_poll::Spawner<'_> {
        fn spawn<F>(&mut self, future: F)
        where
            F: Future<Output = ()> + 'static,
        {
            crate::busy_poll::Spawner::spawn(self, future);
        }
    }
}

// ── Bach Implementation ────────────────────────────────────────────────────

/// Bach spawner for deterministic testing
#[cfg(any(test, feature = "testing"))]
pub mod bach {
    use super::{LocalSpawner, Spawner};
    use std::future::Future;

    /// Bach spawner for deterministic testing
    ///
    /// Bach is single-threaded but we emulate multiple workers for testing worker affinity logic.
    #[derive(Clone)]
    pub struct Runtime {
        worker_count: usize,
    }

    impl Runtime {
        pub fn new(worker_count: usize) -> Self {
            Self { worker_count }
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

    impl LocalSpawner for Local {
        fn spawn<F>(&mut self, future: F)
        where
            F: Future<Output = ()> + 'static,
        {
            // Wrap the !Send future to make it Send for bach's API
            bach::spawn(SendWrapper(future));
        }
    }

    impl Spawner for Runtime {
        type Local<'a> = Local;

        fn worker_count(&self) -> usize {
            self.worker_count
        }

        fn spawn_local<F>(&self, _worker_id: usize, f: F)
        where
            F: FnOnce(Self::Local<'_>) + Send + 'static,
        {
            // Bach is single-threaded, so worker_id doesn't matter
            // Just invoke the closure immediately with a local spawner
            let local = Local;
            f(local);
        }
    }
}

// ── Tokio Implementation ───────────────────────────────────────────────────

/// Tokio spawner with single-threaded runtimes per worker
pub mod tokio {
    use super::{LocalSpawner, Spawner};
    use std::future::Future;

    /// Tokio spawner with single-threaded runtimes per worker
    ///
    /// Each worker is a LocalSet that can run !Send futures.
    #[derive(Clone)]
    pub struct Runtime {
        workers: std::sync::Arc<Vec<WorkerHandle>>,
    }

    struct WorkerHandle {
        sender: tokio::sync::mpsc::UnboundedSender<WorkItem>,
    }

    type WorkItem = Box<dyn FnOnce(Local) + Send>;

    impl Runtime {
        /// Create a new tokio spawner with the specified number of workers
        pub fn new(worker_count: usize) -> Self {
            let workers: Vec<_> = (0..worker_count)
                .map(|_| {
                    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<WorkItem>();

                    // Spawn a thread for this worker with a LocalSet
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
            }
        }
    }

    /// Tokio local spawner that uses spawn_local within a LocalSet
    pub struct Local;

    impl LocalSpawner for Local {
        fn spawn<F>(&mut self, future: F)
        where
            F: Future<Output = ()> + 'static,
        {
            tokio::task::spawn_local(future);
        }
    }

    impl Spawner for Runtime {
        type Local<'a> = Local;

        fn worker_count(&self) -> usize {
            self.workers.len()
        }

        fn spawn_local<F>(&self, worker_id: usize, f: F)
        where
            F: FnOnce(Self::Local<'_>) + Send + 'static,
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
    fn tokio_spawner_worker_count() {
        let spawner = tokio::Runtime::new(4);
        assert_eq!(spawner.worker_count(), 4);
    }

    #[test]
    fn bach_spawner_worker_count() {
        let spawner = bach::Runtime::new(4);
        assert_eq!(spawner.worker_count(), 4);
    }
}

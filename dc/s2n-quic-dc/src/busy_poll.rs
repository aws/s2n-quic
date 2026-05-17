// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use parking_lot::Mutex;
use std::{
    fmt,
    future::Future,
    ops,
    panic::Location,
    pin::Pin,
    sync::{
        atomic::{AtomicI64, AtomicU64, Ordering},
        Arc, Weak,
    },
    task::Context,
};

pub mod clock;

#[derive(Clone)]
pub struct Pool {
    handles: Arc<[Handle]>,
}

impl fmt::Debug for Pool {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Pool").finish_non_exhaustive()
    }
}

/// Per-worker heartbeat state monitored by the watchdog thread.
pub struct Heartbeat {
    /// Bumped after each full task-list iteration. Watchdog compares snapshots to detect stalls.
    counter: AtomicU64,
    /// Index of the task currently being polled (-1 = between tasks).
    current_task: AtomicI64,
}

impl Heartbeat {
    fn new() -> Self {
        Self {
            counter: AtomicU64::new(0),
            current_task: AtomicI64::new(-1),
        }
    }
}

impl Pool {
    pub fn new(handles: Arc<[Handle]>) -> Self {
        Self { handles }
    }

    /// Spawns a watchdog thread that monitors all workers for stalls.
    ///
    /// If any worker's heartbeat counter doesn't advance within `timeout`,
    /// the watchdog prints which worker and task index is stuck, then aborts.
    pub fn spawn_watchdog(&self, timeout: std::time::Duration) {
        let heartbeats: Vec<Arc<Heartbeat>> =
            self.handles.iter().map(|h| h.heartbeat.clone()).collect();
        std::thread::Builder::new()
            .name("busy_poll_watchdog".into())
            .spawn(move || {
                let mut prev: Vec<u64> = vec![0; heartbeats.len()];
                loop {
                    std::thread::sleep(timeout);
                    for (worker_id, hb) in heartbeats.iter().enumerate() {
                        let current = hb.counter.load(Ordering::Relaxed);
                        if current == prev[worker_id] && current > 0 {
                            let task_idx = hb.current_task.load(Ordering::Relaxed);
                            eprintln!(
                                "[watchdog] worker {worker_id} stuck in task {task_idx} \
                                 (heartbeat={current}, no progress in {timeout:?})"
                            );
                            eprintln!(
                                "[watchdog] process alive for debugger attach: pid={}",
                                std::process::id()
                            );
                            std::thread::sleep(std::time::Duration::from_secs(5));
                            std::process::abort();
                        }
                        prev[worker_id] = current;
                    }
                }
            })
            .expect("failed to spawn watchdog thread");
    }
}

impl<T> From<T> for Pool
where
    Arc<[Handle]>: From<T>,
{
    fn from(handles: T) -> Self {
        Self::new(Arc::from(handles))
    }
}

impl ops::Deref for Pool {
    type Target = [Handle];

    fn deref(&self) -> &Self::Target {
        &self.handles
    }
}

#[derive(Clone)]
pub struct Handle {
    state: Arc<Mutex<State>>,
    heartbeat: Arc<Heartbeat>,
    worker_id: usize,
}

impl Handle {
    pub fn new(worker_id: usize) -> (Self, Runner) {
        let state = Arc::new(Mutex::new(State {
            spawns: Vec::with_capacity(16),
        }));
        let heartbeat = Arc::new(Heartbeat::new());
        let handle = Self {
            state: state.clone(),
            heartbeat: heartbeat.clone(),
            worker_id,
        };
        let runner = Runner {
            state: Arc::downgrade(&state),
            heartbeat,
            worker_id,
        };
        (handle, runner)
    }

    #[track_caller]
    pub fn spawn<F>(&self, task: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.spawn_with_priority(task, None);
    }

    #[track_caller]
    pub fn spawn_with_priority<F>(&self, task: F, priority: Option<u8>)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.spawn_local(move |mut spawner| {
            spawner.spawn_with_priority(task, priority);
        });
    }

    /// Spawns a non-Send future by passing a Send function that will be called
    /// on the runner's thread to produce the future.
    ///
    /// This allows creating !Send futures (e.g. using `Rc`-based channels)
    /// that are polled entirely on the busy-poll thread.
    pub fn spawn_local<F>(&self, f: F)
    where
        F: FnOnce(Spawner) + Send + 'static,
    {
        self.state.lock().spawns.push(Spawn::new(f));
    }
}

pub struct Spawner<'a> {
    tasks: &'a mut Tasks,
    pub(crate) worker_id: usize,
}

impl<'a> Spawner<'a> {
    #[track_caller]
    pub fn spawn<F>(&mut self, future: F)
    where
        F: Future<Output = ()> + 'static,
    {
        self.spawn_with_priority(future, None);
    }

    #[track_caller]
    pub fn spawn_with_priority<F>(&mut self, future: F, priority: Option<u8>)
    where
        F: Future<Output = ()> + 'static,
    {
        self.spawn_with_priority_and_name(future, priority, None);
    }

    #[track_caller]
    pub fn spawn_with_priority_and_name<F>(
        &mut self,
        future: F,
        priority: Option<u8>,
        name: Option<String>,
    ) where
        F: Future<Output = ()> + 'static,
    {
        let priority = priority.unwrap_or(128);
        let task = Task {
            task: Box::pin(future),
            priority,
            location: Location::caller(),
            name,
        };

        self.tasks.spawn(task);
    }
}

struct State {
    spawns: Vec<Spawn>,
}

/// A `Send` factory that produces a (possibly `!Send`) `Task` on the runner thread.
struct Spawn {
    factory: Box<dyn FnOnce(Spawner) + Send>,
}

impl Spawn {
    fn new(f: impl FnOnce(Spawner) + Send + 'static) -> Self {
        Self {
            factory: Box::new(f),
        }
    }

    fn into_tasks(self, tasks: &mut Tasks, worker_id: usize) {
        (self.factory)(Spawner { tasks, worker_id })
    }
}

/// A task that lives exclusively on the runner thread. May be `!Send`.
struct Task {
    task: Pin<Box<dyn Future<Output = ()> + 'static>>,
    priority: u8,
    #[allow(dead_code)]
    location: &'static Location<'static>,
    #[allow(dead_code)]
    name: Option<String>,
}

#[must_use]
pub struct Runner {
    state: Weak<Mutex<State>>,
    heartbeat: Arc<Heartbeat>,
    worker_id: usize,
}

impl Runner {
    pub fn run(self) {
        let state = self.state;
        let heartbeat = self.heartbeat;
        let worker_id = self.worker_id;
        let waker = s2n_quic_core::task::waker::noop();
        let mut cx = Context::from_waker(&waker);
        let mut tasks = Tasks::new();
        let mut spawns = Vec::with_capacity(16);

        struct AbortOnPanic;

        impl Drop for AbortOnPanic {
            fn drop(&mut self) {
                if std::thread::panicking() {
                    std::process::abort();
                }
            }
        }

        let _guard = AbortOnPanic;

        loop {
            const ITERATIONS: usize = if cfg!(debug_assertions) {
                10
            } else {
                1_000_000
            };

            for _ in 0..ITERATIONS {
                tasks.poll(&mut cx, &*heartbeat);
            }

            // Yield to allow other threads (especially SCHED_OTHER threads like Tokio runtime)
            // to make progress when running with RT scheduling
            #[cfg(target_os = "linux")]
            unsafe {
                libc::sched_yield();
            }

            if let Some(state) = state.upgrade() {
                if let Some(mut guard) = state.try_lock() {
                    core::mem::swap(&mut spawns, &mut guard.spawns);
                }
            } else {
                return;
            }

            if spawns.is_empty() {
                continue;
            }

            for spawn in spawns.drain(..) {
                spawn.into_tasks(&mut tasks, worker_id);
            }

            tasks.after_spawn();
        }
    }
}

struct Tasks {
    slots: Vec<Option<Task>>,
    free: Vec<usize>,
}

impl Tasks {
    const fn new() -> Self {
        Self {
            slots: Vec::new(),
            free: Vec::new(),
        }
    }

    fn spawn(&mut self, task: Task) {
        if let Some(idx) = self.free.pop() {
            self.slots[idx] = Some(task);
        } else {
            self.slots.push(Some(task));
        }
    }

    fn after_spawn(&mut self) {
        self.free.clear();

        self.slots.sort_by(|a, b| {
            match (a, b) {
                // priority 0 is highest
                (Some(a), Some(b)) => a.priority.cmp(&b.priority),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => std::cmp::Ordering::Equal,
            }
        });

        // clear out the free slots
        while self.slots.last().map_or(false, Option::is_none) {
            let slot = self.slots.pop().unwrap();
            debug_assert!(slot.is_none());
        }
    }

    fn poll(&mut self, cx: &mut Context, heartbeat: &Heartbeat) {
        for (idx, slot) in self.slots.iter_mut().enumerate() {
            if let Some(task) = slot {
                heartbeat.current_task.store(idx as i64, Ordering::Relaxed);
                if task.task.as_mut().poll(cx).is_ready() {
                    eprintln!("task {idx} done ({})", task.location);
                    *slot = None;
                    self.free.push(idx);
                }
            }
        }
        heartbeat.current_task.store(-1, Ordering::Relaxed);
        heartbeat.counter.fetch_add(1, Ordering::Relaxed);
    }
}

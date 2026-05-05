// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use parking_lot::Mutex;
use std::{
    fmt,
    future::Future,
    ops,
    panic::Location,
    pin::Pin,
    sync::{Arc, Weak},
    task::Context,
    time::Instant,
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

impl Pool {
    pub fn new(handles: Arc<[Handle]>) -> Self {
        Self { handles }
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
}

impl Handle {
    pub fn new() -> (Self, Runner) {
        let state = Arc::new(Mutex::new(State {
            spawns: Vec::with_capacity(16),
        }));
        let handle = Self {
            state: state.clone(),
        };
        let runner = Runner(Arc::downgrade(&state));
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
        let priority = priority.unwrap_or(128);
        let task = Task {
            task: Box::pin(future),
            priority,
            location: Location::caller(),
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

    fn into_tasks(self, tasks: &mut Tasks) {
        (self.factory)(Spawner { tasks })
    }
}

/// A task that lives exclusively on the runner thread. May be `!Send`.
struct Task {
    task: Pin<Box<dyn Future<Output = ()> + 'static>>,
    priority: u8,
    location: &'static Location<'static>,
}

#[must_use]
pub struct Runner(Weak<Mutex<State>>);

impl Runner {
    pub fn run(self) {
        let state = self.0;
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
                tasks.poll(&mut cx);
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
                spawn.into_tasks(&mut tasks);
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

    fn poll(&mut self, cx: &mut Context) {
        const SLOW_POLL_THRESHOLD: std::time::Duration = std::time::Duration::from_millis(2);

        for (idx, slot) in self.slots.iter_mut().enumerate() {
            if let Some(task) = slot {
                // let start = Instant::now();
                if task.task.as_mut().poll(cx).is_ready() {
                    eprintln!("task {idx} done");
                    *slot = None;
                    self.free.push(idx);
                } else {
                    // let duration = start.elapsed();
                    // if duration > SLOW_POLL_THRESHOLD {
                    // tracing::warn!(
                    // task_idx = idx,
                    // ?duration,
                    // location = %task.location,
                    // "slow task poll detected"
                    // );
                    // }
                }
            }
        }
    }
}

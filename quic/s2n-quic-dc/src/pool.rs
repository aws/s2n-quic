use core::ops;
use crossbeam_channel as mpmc;
use std::{
    mem::ManuallyDrop,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};
use tracing::{info, trace};

pub struct Pool<T: 'static + Send> {
    release: mpmc::Sender<T>,
    acquire: mpmc::Receiver<T>,
    stats: Option<Arc<Stats>>,
}

impl<T: 'static + Send> Clone for Pool<T> {
    #[inline]
    fn clone(&self) -> Self {
        Self {
            release: self.release.clone(),
            acquire: self.acquire.clone(),
            stats: self.stats.clone(),
        }
    }
}

impl<T: 'static + Send> Default for Pool<T> {
    #[inline]
    fn default() -> Self {
        Self::new(2000)
    }
}

impl<T: 'static + Send> Pool<T> {
    #[inline]
    pub fn new(max_entries: usize) -> Self {
        let (release, acquire) = mpmc::bounded(max_entries);
        let mut pool = Self {
            release,
            acquire,
            stats: None,
        };

        if std::env::var("DC_QUIC_POOL_METRICS").is_ok() {
            let stats = Arc::new(Stats::default());
            pool.stats = Some(stats.clone());
            std::thread::spawn(move || loop {
                std::thread::sleep(core::time::Duration::from_secs(1));
                let hits = stats.hits.load(Ordering::Relaxed);
                let misses = stats.misses.load(Ordering::Relaxed);
                let errors = stats.errors.load(Ordering::Relaxed);
                let hit_ratio = hits as f64 / (hits + misses) as f64 * 100.0;
                info!(hits, misses, errors, hit_ratio);
            });
        }

        pool
    }

    #[inline]
    pub fn get(&self) -> Option<Entry<T>> {
        let entry = self.acquire.try_recv().ok()?;
        let entry = Entry::new(entry, self.release.clone());
        Some(entry)
    }

    #[inline]
    pub fn get_or_init<F, E>(&self, f: F) -> Result<Entry<T>, E>
    where
        F: FnOnce() -> Result<T, E>,
    {
        if let Some(entry) = self.get() {
            if let Some(stats) = self.stats.as_ref() {
                stats.hits.fetch_add(1, Ordering::Relaxed);
            }
            trace!("hit");
            Ok(entry)
        } else {
            let entry = f();

            if entry.is_err() {
                if let Some(stats) = self.stats.as_ref() {
                    stats.errors.fetch_add(1, Ordering::Relaxed);
                }
            }

            let entry = entry?;

            let entry = Entry::new(entry, self.release.clone());
            if let Some(stats) = self.stats.as_ref() {
                stats.misses.fetch_add(1, Ordering::Relaxed);
            }
            trace!("miss");
            Ok(entry)
        }
    }
}

#[derive(Default)]
struct Stats {
    hits: AtomicUsize,
    misses: AtomicUsize,
    errors: AtomicUsize,
}

pub struct Entry<T: 'static + Send> {
    entry: ManuallyDrop<T>,
    pool: mpmc::Sender<T>,
}

impl<T: Send> Entry<T> {
    #[inline]
    fn new(entry: T, pool: mpmc::Sender<T>) -> Self {
        let entry = ManuallyDrop::new(entry);
        Self { entry, pool }
    }
}

impl<T: Send> ops::Deref for Entry<T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.entry
    }
}

impl<T: Send> ops::DerefMut for Entry<T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.entry
    }
}

impl<T: Send> Drop for Entry<T> {
    #[inline]
    fn drop(&mut self) {
        let socket = unsafe { ManuallyDrop::take(&mut self.entry) };
        trace!("release");
        let _ = self.pool.try_send(socket);
    }
}

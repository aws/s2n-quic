// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Handshake completion takes the eviction queue mutex the cleaner holds while walking, which is
//! why collection attaches to that walk instead of making a second pass, and why everything here
//! is shaped to keep work off the lock. Not a general purpose extension point -- it exists to
//! serve on-disk persistence of the map.

use super::Entry;
use std::{fmt, sync::Arc, time::Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CollectRequest {
    Nothing,

    Snapshot,

    /// `since` is an `Instant` rather than a duration because the cleaner's cadence is jittered:
    /// "everything new since I last asked" needs the instant of the last ask. Take it from
    /// [`Collected::resume_from`].
    Journal {
        since: Instant,
    },
}

impl CollectRequest {
    #[inline]
    pub fn wants_entries(&self) -> bool {
        !matches!(self, CollectRequest::Nothing)
    }

    #[inline]
    pub(super) fn since(&self) -> Option<Instant> {
        match self {
            CollectRequest::Journal { since } => Some(*since),
            CollectRequest::Nothing | CollectRequest::Snapshot => None,
        }
    }
}

pub struct Collected {
    request: CollectRequest,
    entries: Vec<Arc<Entry>>,
    resume_from: Instant,
    skipped: usize,
}

impl fmt::Debug for Collected {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Collected")
            .field("request", &self.request)
            .field("entries", &self.entries.len())
            .field("skipped", &self.skipped)
            .field("truncated", &self.is_truncated())
            .finish()
    }
}

impl Collected {
    #[cfg(any(test, feature = "testing"))]
    pub fn builder() -> CollectedBuilder {
        CollectedBuilder::default()
    }

    #[inline]
    pub fn request(&self) -> CollectRequest {
        self.request
    }

    #[inline]
    pub fn entries(&self) -> &[Arc<Entry>] {
        &self.entries
    }

    #[inline]
    pub fn into_entries(self) -> Vec<Arc<Entry>> {
        self.entries
    }

    /// Pass this as the next [`CollectRequest::Journal`]'s `since`, for truncated and complete
    /// batches alike. Ignoring it silently loses entries.
    ///
    /// The earliest creation time this batch did *not* account for, which is deliberately not the
    /// latest one it did: re-collecting an entry a previous batch carried is harmless, losing one
    /// is not, so the rule errs toward the duplicate.
    #[inline]
    pub fn resume_from(&self) -> Instant {
        self.resume_from
    }

    /// Zero in normal operation, since the buffer is sized from the entry count plus
    /// `CAPACITY_SLACK`. Non-zero means the map grew by more than that slack during one pass, and
    /// is worth reporting. Nothing is lost -- [`resume_from`](Self::resume_from) still covers the
    /// omitted entries -- but a consumer writing one file per cycle produces a short one.
    #[inline]
    pub fn skipped(&self) -> usize {
        self.skipped
    }

    #[inline]
    pub fn is_truncated(&self) -> bool {
        self.skipped > 0
    }
}

#[cfg(any(test, feature = "testing"))]
#[derive(Debug, Default)]
pub struct CollectedBuilder {
    request: Option<CollectRequest>,
    entries: Vec<Arc<Entry>>,
    resume_from: Option<Instant>,
    skipped: usize,
}

#[cfg(any(test, feature = "testing"))]
impl CollectedBuilder {
    pub fn with_request(mut self, request: CollectRequest) -> Self {
        self.request = Some(request);
        self
    }

    pub fn with_entries(mut self, entries: Vec<Arc<Entry>>) -> Self {
        self.entries = entries;
        self
    }

    pub fn with_resume_from(mut self, resume_from: Instant) -> Self {
        self.resume_from = Some(resume_from);
        self
    }

    pub fn with_skipped(mut self, skipped: usize) -> Self {
        self.skipped = skipped;
        self
    }

    pub fn build(self) -> Collected {
        let resume_from = self.resume_from.unwrap_or_else(|| {
            self.entries
                .iter()
                .map(|e| e.creation_time())
                .max()
                .map(|latest| latest + std::time::Duration::from_nanos(1))
                .unwrap_or_else(Instant::now)
        });

        Collected {
            request: self.request.unwrap_or(CollectRequest::Snapshot),
            entries: self.entries,
            resume_from,
            skipped: self.skipped,
        }
    }
}

/// At most one consumer per map.
///
/// Entries arrive in insertion order, and that is a guarantee rather than an implementation
/// detail: two entries for one peer address can both be live (a re-handshake retires the old one),
/// and re-inserting in collection order is what makes the newer win the address index, so
/// reordering would silently associate a peer with a superseded secret.
///
/// Both methods run on the cleaner thread, `consume` last in the cycle and outside the queue
/// mutex, so an implementation may encode and write here; what it delays is the next cycle,
/// jittered around 60 seconds. Holding a batch beyond that pins entries the map has evicted.
pub trait EntryConsumer: 'static + Send + Sync {
    fn request(&self) -> CollectRequest;

    /// Called once per cycle in which `request` asked for something, even when the batch is empty,
    /// so "nothing matched" is distinguishable from "not asked".
    fn consume(&self, collected: Collected);
}

/// Over-allocate rather than reallocate under the queue mutex: the buffer is sized from
/// `ids.len()` before the mutex is taken, so the count can grow before the walk begins.
const CAPACITY_SLACK: usize = 1024;

pub(super) struct Collector {
    request: CollectRequest,
    since: Option<Instant>,
    entries: Vec<Arc<Entry>>,

    latest_collected: Option<Instant>,

    // a minimum rather than the first one skipped, because queue order does not strictly follow
    // creation time: `Entry` timestamps itself before taking the queue lock
    earliest_skipped: Option<Instant>,

    // resume point for a batch that collected nothing. Using the walk start rather than carrying
    // the previous cutoff forward means an idle cycle still advances, so a consumer does not
    // re-read the same window indefinitely.
    started: Instant,

    skipped: usize,
}

impl Collector {
    pub(super) fn new(request: CollectRequest, live_entries: usize) -> Option<Self> {
        if !request.wants_entries() {
            return None;
        }

        Some(Self {
            request,
            since: request.since(),
            // sized from the live count for a journal too: one resuming after a truncated or
            // failed cycle can approach the whole map, and a buffer sized for the common case
            // would reallocate under the mutex exactly when the map is already under strain
            entries: Vec::with_capacity(live_entries.saturating_add(CAPACITY_SLACK)),
            latest_collected: None,
            earliest_skipped: None,
            started: Instant::now(),
            skipped: 0,
        })
    }

    #[inline]
    pub(super) fn push(&mut self, entry: Arc<Entry>) {
        // A retired entry's marker is epoch-based and cannot be meaningfully persisted, so a
        // consumer that restored it would resurrect a dead secret that could then win the peer
        // index over its own replacement.
        if entry.retired_at().is_some() {
            return;
        }

        let created = entry.creation_time();

        // this mechanism can skip entries permanently, and we are ok with that: a handshake that
        // stalled between `Entry::new` and the queue lock arrives with `created` behind a `since`
        // some earlier walk already published, and no later journal asks for less. Only a
        // snapshot, which carries no `since`, picks it back up.
        if self.since.is_some_and(|since| created < since) {
            return;
        }

        // full: count it and track its time rather than growing the buffer, which would
        // reallocate while the queue mutex is held
        if self.entries.len() == self.entries.capacity() {
            self.skipped += 1;
            self.earliest_skipped = Some(match self.earliest_skipped {
                Some(earliest) if earliest <= created => earliest,
                _ => created,
            });
            return;
        }

        self.latest_collected = Some(match self.latest_collected {
            Some(latest) if latest >= created => latest,
            _ => created,
        });
        self.entries.push(entry);
    }

    pub(super) fn finish(self) -> Collected {
        // see `Collected::resume_from`. A skipped entry always wins: it is the earliest thing this
        // batch failed to account for, and is necessarily no newer than something we did collect.
        let resume_from = match (self.earliest_skipped, self.latest_collected) {
            (Some(earliest_skipped), _) => earliest_skipped,
            (None, Some(latest)) => latest + std::time::Duration::from_nanos(1),
            (None, None) => self.started,
        };

        Collected {
            request: self.request,
            entries: self.entries,
            resume_from,
            skipped: self.skipped,
        }
    }
}

#[cfg(test)]
mod tests;

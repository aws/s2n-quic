// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Hands live map entries to an embedding application once per cleaner cycle.
//!
//! The cleaner already walks every entry in the eviction queue roughly once a minute, holding the
//! queue mutex while it does so. Handshake completion takes that same mutex, so any additional work
//! performed under it delays handshakes. This module lets an embedder receive entries by attaching to
//! that existing walk rather than performing a second one.
//!
//! The cleaner consults the consumer twice per cycle:
//!
//! 1. [`EntryConsumer::request`], *before* the queue mutex is taken, to ask what is wanted.
//!
//! 2. [`EntryConsumer::consume`], *after* the mutex is released, to hand over the batch.
//!
//! Under the mutex the per-entry cost is a comparison and an 8-byte move into a pre-allocated
//! buffer. The reference count was already incremented by the cleaner's own `Weak::upgrade`, so
//! moving the `Arc` out costs no atomic operation.
//!
//! This is deliberately not a general purpose extension point. It exists to serve on-disk
//! persistence of the map, and its shape is chosen to keep work off the mutex rather than to be
//! broadly reusable.

use super::Entry;
use std::{fmt, sync::Arc, time::Instant};

/// What the cleaner should collect on this cycle.
///
/// Returned by [`EntryConsumer::request`] and handed back to [`EntryConsumer::consume`] so the
/// consumer knows which kind of batch it is receiving without tracking that itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CollectRequest {
    /// Collect nothing. The cleaner does no extra work and [`EntryConsumer::consume`] is not
    /// called.
    Nothing,

    /// Collect every live entry.
    Snapshot,

    /// Collect only entries created at or after `since`.
    ///
    /// `since` is an [`Instant`] rather than a duration because the cleaner's cadence is jittered:
    /// a caller that wants "everything new since I last asked" must supply the instant it last
    /// asked, or entries created in the jitter gap are missed entirely.
    ///
    /// If a previous cycle was truncated, this must be no later than that cycle's
    /// [`Collected::resume_from`], or the entries it could not fit are never collected. See
    /// [`Collected::resume_from`].
    Journal { since: Instant },
}

impl CollectRequest {
    /// Whether this request asks for anything at all.
    #[inline]
    pub fn wants_entries(&self) -> bool {
        !matches!(self, CollectRequest::Nothing)
    }

    /// The creation-time cutoff, if this request has one.
    #[inline]
    pub(super) fn since(&self) -> Option<Instant> {
        match self {
            CollectRequest::Journal { since } => Some(*since),
            CollectRequest::Nothing | CollectRequest::Snapshot => None,
        }
    }
}

/// One batch of entries, handed to [`EntryConsumer::consume`].
///
/// Construct nothing here; the cleaner produces these.
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

impl Default for Collected {
    /// An empty batch, as produced by a cycle that found nothing to collect.
    ///
    /// No entries, nothing skipped, and resuming from now -- matching what the cleaner produces for
    /// an empty walk. The request is reported as [`CollectRequest::Snapshot`], since
    /// [`CollectRequest::Nothing`] never yields a batch at all.
    ///
    /// Chiefly useful for exercising the accessors. To build a batch with entries in it, see
    /// [`Collected::builder`].
    ///
    /// # Examples
    ///
    /// ```
    /// use amzn_salty_s2n_quic_dc::path::secret::map::Collected;
    ///
    /// let batch = Collected::default();
    ///
    /// assert!(batch.entries().is_empty());
    /// assert_eq!(batch.skipped(), 0);
    /// assert!(!batch.is_truncated());
    /// ```
    fn default() -> Self {
        Self {
            request: CollectRequest::Snapshot,
            entries: Vec::new(),
            resume_from: Instant::now(),
            skipped: 0,
        }
    }
}

impl Collected {
    /// Begins building a batch, for testing an [`EntryConsumer`] implementation.
    ///
    /// Only available with the `testing` feature. In production the cleaner is the only producer of
    /// a `Collected`. See [`CollectedBuilder`] for examples.
    #[cfg(any(test, feature = "testing"))]
    pub fn builder() -> CollectedBuilder {
        CollectedBuilder::default()
    }

    /// The request that produced this batch.
    #[inline]
    pub fn request(&self) -> CollectRequest {
        self.request
    }

    /// The collected entries, in insertion order.
    ///
    /// See the [ordering guarantee](EntryConsumer#ordering).
    #[inline]
    pub fn entries(&self) -> &[Arc<Entry>] {
        &self.entries
    }

    /// Takes the collected entries, in insertion order.
    ///
    /// Dropping them promptly releases the strong references this batch holds; see the
    /// [cost note](EntryConsumer#cost).
    ///
    /// # Examples
    ///
    /// Use [`entries`](Self::entries) to read a batch while keeping the other accessors available,
    /// and `into_entries` to hand the entries onward -- to a writer thread, say -- when the rest of
    /// the batch is no longer needed:
    ///
    /// ```
    /// use amzn_salty_s2n_quic_dc::path::secret::map::Collected;
    ///
    /// let batch = Collected::default();
    ///
    /// // Borrowing leaves the batch intact.
    /// let count = batch.entries().len();
    /// let resume = batch.resume_from();
    ///
    /// // Read anything still needed from the batch *before* consuming it.
    /// let entries = batch.into_entries();
    /// assert_eq!(entries.len(), count);
    ///
    /// // `resume` is still usable; `batch` is gone.
    /// let _ = resume;
    /// ```
    #[inline]
    pub fn into_entries(self) -> Vec<Arc<Entry>> {
        self.entries
    }

    /// The cutoff a subsequent [`CollectRequest::Journal`] should use to continue from this batch.
    ///
    /// Always meaningful, for truncated and complete batches alike, so a consumer streaming
    /// snapshots and journals has one rule: pass the previous batch's `resume_from` as the next
    /// request's `since`.
    ///
    /// This is **not** the creation time of the last entry in [`entries`](Self::entries), nor the
    /// latest creation time in it. `Entry` timestamps itself before taking the queue lock, so two
    /// handshakes completing concurrently can be enqueued in the opposite order to their creation
    /// times. If such a pair straddles the point where the buffer fills, the older of the two is
    /// the one skipped, and resuming from either the last or the highest collected time would place
    /// the cutoff after it -- excluding it from this batch and from every batch that follows.
    ///
    /// So the value is the earliest creation time this batch did *not* account for:
    ///
    /// - Truncated: the earliest creation time among the skipped entries.
    /// - Complete: one nanosecond past the latest creation time collected, so the next journal
    ///   excludes what this batch already has.
    /// - Empty: when the walk began, since nothing older can become eligible later.
    ///
    /// Re-collecting an entry the previous batch already carried is expected and harmless; losing
    /// one is not. The rule errs toward the duplicate.
    ///
    /// # Examples
    ///
    /// The whole of the chaining rule, in a collector that alternates snapshots and journals:
    ///
    /// ```
    /// use amzn_salty_s2n_quic_dc::path::secret::map::{
    ///     CollectRequest, Collected, EntryConsumer,
    /// };
    /// use std::{sync::Mutex, time::Instant};
    ///
    /// struct Persistence {
    ///     /// Where the next journal should start. `None` until the first snapshot lands.
    ///     cutoff: Mutex<Option<Instant>>,
    /// }
    ///
    /// impl EntryConsumer for Persistence {
    ///     fn request(&self) -> CollectRequest {
    ///         match *self.cutoff.lock().unwrap() {
    ///             // Nothing written yet: take everything.
    ///             None => CollectRequest::Snapshot,
    ///             Some(since) => CollectRequest::Journal { since },
    ///         }
    ///     }
    ///
    ///     fn consume(&self, collected: Collected) {
    ///         // ... write `collected.entries()` to disk ...
    ///
    ///         // Advance only on success. Advancing after a failed write would leave the entries
    ///         // this batch carried in no file at all.
    ///         *self.cutoff.lock().unwrap() = Some(collected.resume_from());
    ///     }
    /// }
    ///
    /// let persistence = Persistence { cutoff: Mutex::new(None) };
    /// assert_eq!(persistence.request(), CollectRequest::Snapshot);
    ///
    /// persistence.consume(Collected::default());
    /// assert!(matches!(persistence.request(), CollectRequest::Journal { .. }));
    /// ```
    #[inline]
    pub fn resume_from(&self) -> Instant {
        self.resume_from
    }

    /// How many entries the batch omitted because the buffer was full.
    ///
    /// Zero in normal operation: the buffer is sized from the map's entry count plus slack. A
    /// non-zero value means the map grew by more than that slack during a single cleaner pass, and
    /// is worth reporting. Nothing is lost, because [`resume_from`](Self::resume_from) still covers
    /// the omitted entries, but a consumer writing one file per cycle will produce a short one.
    #[inline]
    pub fn skipped(&self) -> usize {
        self.skipped
    }

    /// Whether the buffer was too small and entries were left uncollected.
    ///
    /// Equivalent to `skipped() > 0`. The omitted entries are not lost: they are covered by
    /// [`resume_from`](Self::resume_from).
    ///
    /// # Examples
    ///
    /// Truncation is a signal to report, not an error to handle: the batch is still valid and the
    /// omitted entries arrive on the next cycle.
    ///
    /// ```
    /// use amzn_salty_s2n_quic_dc::path::secret::map::Collected;
    ///
    /// fn write(collected: &Collected) {
    ///     if collected.is_truncated() {
    ///         // Emit a metric: the map grew faster than the buffer's slack during one cycle.
    ///         // Nothing is lost, but this file is short.
    ///         eprintln!("collection truncated, {} entries deferred", collected.skipped());
    ///     }
    ///
    ///     // Write the batch either way.
    ///     for _entry in collected.entries() {
    ///         // ... encode ...
    ///     }
    /// }
    ///
    /// write(&Collected::default());
    /// ```
    #[inline]
    pub fn is_truncated(&self) -> bool {
        self.skipped > 0
    }
}

/// Builds a [`Collected`] for testing an [`EntryConsumer`] implementation.
///
/// Only available with the `testing` feature. The cleaner is the only thing that produces a
/// [`Collected`] in production; this exists so that a consumer can exercise its own
/// [`EntryConsumer::consume`] without running a map and waiting for a cleaner cycle.
///
/// [`resume_from`](Collected::resume_from) defaults to the value the cleaner would have produced
/// for the given entries, so a batch built without setting it behaves like a complete one.
///
/// # Examples
///
/// Testing a collector that records what it is handed:
///
/// ```
/// use amzn_salty_s2n_quic_dc::path::secret::map::{
///     CollectRequest, Collected, Entry, EntryConsumer,
/// };
/// use std::sync::{Arc, Mutex};
///
/// #[derive(Default)]
/// struct Recorder {
///     seen: Mutex<Vec<Collected>>,
/// }
///
/// impl EntryConsumer for Recorder {
///     fn request(&self) -> CollectRequest {
///         CollectRequest::Snapshot
///     }
///
///     fn consume(&self, collected: Collected) {
///         self.seen.lock().unwrap().push(collected);
///     }
/// }
///
/// let peer = "127.0.0.1:4433".parse().unwrap();
/// let batch = Collected::builder()
///     .with_request(CollectRequest::Snapshot)
///     .with_entries(vec![Entry::fake(peer, None)])
///     .build();
///
/// let recorder = Recorder::default();
/// recorder.consume(batch);
///
/// let seen = recorder.seen.lock().unwrap();
/// assert_eq!(seen[0].entries().len(), 1);
/// assert!(!seen[0].is_truncated());
/// ```
///
/// Exercising the truncation path, which is otherwise hard to reach:
///
/// ```
/// use amzn_salty_s2n_quic_dc::path::secret::map::{CollectRequest, Collected, Entry};
///
/// let peer = "127.0.0.1:4433".parse().unwrap();
/// let batch = Collected::builder()
///     .with_entries(vec![Entry::fake(peer, None)])
///     .with_skipped(7)
///     .build();
///
/// assert!(batch.is_truncated());
/// assert_eq!(batch.skipped(), 7);
/// ```
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
    /// Sets the request this batch is a response to. Defaults to [`CollectRequest::Snapshot`].
    pub fn with_request(mut self, request: CollectRequest) -> Self {
        self.request = Some(request);
        self
    }

    /// Sets the entries, which are taken to be in insertion order.
    pub fn with_entries(mut self, entries: Vec<Arc<Entry>>) -> Self {
        self.entries = entries;
        self
    }

    /// Overrides the resume point.
    ///
    /// Defaults to what the cleaner would have produced: just past the newest entry, or the current
    /// instant for an empty batch. Set this to test a consumer's handling of a specific cutoff.
    pub fn with_resume_from(mut self, resume_from: Instant) -> Self {
        self.resume_from = Some(resume_from);
        self
    }

    /// Marks the batch as truncated, having omitted `skipped` entries.
    pub fn with_skipped(mut self, skipped: usize) -> Self {
        self.skipped = skipped;
        self
    }

    /// Builds the [`Collected`].
    pub fn build(self) -> Collected {
        let resume_from = self.resume_from.unwrap_or_else(|| {
            // Mirror `Collector::finish` for a complete batch: just past the newest entry, so a
            // journal chained from this batch excludes what it already carried.
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

/// Consumes batches of live map entries gathered by the cleaner.
///
/// The cleaner does the gathering, during a walk it performs anyway; an implementation of this trait
/// states what it wants and then receives it. Registered via `Builder::with_entry_consumer`. At most
/// one consumer per map.
///
/// # Ordering
///
/// **The `entries` handed to [`consume`](EntryConsumer::consume) are in eviction queue order,
/// which is the order in which they were inserted into the map.** The eviction queue is a
/// `VecDeque` that is pushed to at the back on insertion and popped from the front on eviction, and
/// the cleaner walks it front to back, so collection preserves that order.
///
/// This is a guarantee rather than an implementation detail, because a consumer that persists and
/// later restores entries depends on it. Two entries for the same peer address can both be present
/// -- a re-handshake creates a new entry and retires the old one -- and re-inserting them in
/// collection order is what causes the newer one to win the address index. Reordering collection
/// would silently associate a peer with a superseded secret.
///
/// # Cost
///
/// Both methods are called on the cleaner thread. `request` runs before the queue mutex is taken and
/// `consume` last of all, after the mutex is released and after eviction, re-handshake selection and
/// the cleaner's own metrics. Neither blocks handshake completion, `consume` delays no other cleaner
/// work, and time spent in `consume` is not attributed to the cleaner -- a consumer times itself.
///
/// A consumer may therefore encode and write on this thread. What it delays is the next cleaner
/// cycle, which is jittered around a 60 second cadence. A consumer that cannot bound its work below
/// that should hand the batch to a thread of its own.
///
/// The batch holds a strong reference to each entry, keeping it alive until the consumer drops it.
/// This costs no extra memory while the entries are still in the map, which is the normal case, but
/// retaining the batch indefinitely would pin evicted entries.
///
/// # Truncation
///
/// The buffer is never grown during the walk, so a batch can be incomplete. A consumer must honour
/// [`Collected::resume_from`] or it will silently lose entries; see that method.
pub trait EntryConsumer: 'static + Send + Sync {
    /// Asks what to collect on this cycle.
    ///
    /// Called once per cleaner cycle, before the eviction queue mutex is taken. Returning
    /// [`CollectRequest::Nothing`] costs the cycle a single branch.
    fn request(&self) -> CollectRequest;

    /// Receives the entries the cleaner gathered.
    ///
    /// Called once per cycle in which `request` returned anything other than
    /// [`CollectRequest::Nothing`], as the last step of that cycle.
    ///
    /// Called even when the batch is empty, so that a cycle which collected nothing is
    /// distinguishable from a cycle that was never asked.
    fn consume(&self, collected: Collected);
}

/// Extra capacity beyond the current entry count when allocating a collection buffer.
///
/// The buffer is sized from `ids.len()` before the queue mutex is taken, so the count can grow
/// slightly before the walk begins. Over-allocating by a small margin avoids a reallocation under
/// the mutex, which is the cost this whole module exists to avoid.
const CAPACITY_SLACK: usize = 1024;

/// Accumulates collected entries during the `retain` walk.
///
/// Constructed before the eviction queue mutex is taken so that the allocation happens outside it.
pub(super) struct Collector {
    request: CollectRequest,
    since: Option<Instant>,
    entries: Vec<Arc<Entry>>,

    /// The latest creation time collected, used to derive the resume point for a complete batch.
    latest_collected: Option<Instant>,

    /// The earliest creation time skipped for want of room, which is the resume point for a
    /// truncated batch. Tracked as a minimum rather than taking the first, because queue order does
    /// not strictly follow creation time.
    earliest_skipped: Option<Instant>,

    /// When the walk began.
    ///
    /// The resume point for a batch that collected nothing at all: no entry created before the walk
    /// started is eligible, and there is no collected time to derive one from. Using `started` here
    /// rather than carrying the previous cutoff forward means an idle cycle still advances, which
    /// keeps a consumer from re-reading the same window indefinitely.
    started: Instant,

    skipped: usize,
}

impl Collector {
    /// Prepares collection for `request`, or `None` if nothing is wanted.
    ///
    /// The buffer is sized from `live_entries` for every request kind, including a journal. A
    /// journal is normally a small fraction of the map, but one resuming after a truncated or failed
    /// cycle can approach the whole of it, and a buffer sized for the common case would reallocate
    /// under the mutex in exactly the situation where the map is already under strain.
    pub(super) fn new(request: CollectRequest, live_entries: usize) -> Option<Self> {
        if !request.wants_entries() {
            return None;
        }

        Some(Self {
            request,
            since: request.since(),
            entries: Vec::with_capacity(live_entries.saturating_add(CAPACITY_SLACK)),
            latest_collected: None,
            earliest_skipped: None,
            started: Instant::now(),
            skipped: 0,
        })
    }

    /// Adds `entry` to the batch if it is wanted and there is room, and drops it otherwise.
    ///
    /// The buffer is never grown, because doing so would reallocate while the eviction queue mutex
    /// is held. Once full, further entries are counted and their earliest creation time tracked, so
    /// that [`Collected::resume_from`] still covers them.
    #[inline]
    pub(super) fn push(&mut self, entry: Arc<Entry>) {
        // Retired entries are excluded. A retired entry has been superseded by a newer one for the
        // same peer, and its retired marker is epoch-based and so cannot be meaningfully persisted:
        // a consumer that restored it would resurrect a dead secret that could then win the peer
        // index over its own replacement.
        if entry.retired_at().is_some() {
            return;
        }

        let created = entry.creation_time();

        if self.since.is_some_and(|since| created < since) {
            return;
        }

        // Full. Track the earliest creation time we could not take, rather than reallocating under
        // the mutex. A minimum rather than the first one seen, because queue order does not strictly
        // follow creation time: `Entry` timestamps itself before taking the queue lock.
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

    /// Finishes the batch for handing to [`EntryConsumer::consume`].
    pub(super) fn finish(self) -> Collected {
        // A skipped entry always wins: it is the earliest thing this batch failed to account for, and
        // it is necessarily older than or equal to some entry we did collect.
        let resume_from = match (self.earliest_skipped, self.latest_collected) {
            (Some(earliest_skipped), _) => earliest_skipped,
            // Complete: resume just past the newest entry collected, so the next journal excludes
            // what this batch already carried. `Instant` has nanosecond resolution and two entries
            // cannot share a creation instant without one of them being observable here anyway.
            (None, Some(latest)) => latest + std::time::Duration::from_nanos(1),
            // Collected nothing: nothing older than the walk can become eligible later.
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

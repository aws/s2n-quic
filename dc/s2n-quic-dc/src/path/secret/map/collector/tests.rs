// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::{
    event::tracing,
    path::secret::{
        map::{state::State, store::Store, Epoch},
        stateless_reset,
    },
};
use s2n_quic_core::time::NoopClock as Clock;
use std::{
    net::{Ipv4Addr, SocketAddr},
    sync::Mutex,
};

/// An [`EntryConsumer`] that records what it is asked and what it receives.
///
/// `request` is scripted by the test: each call pops the next queued response, defaulting to
/// `Nothing` once exhausted, so one test can drive several cleaner passes with different requests.
#[derive(Default)]
struct Recorder {
    requests: Mutex<Vec<CollectRequest>>,
    batches: Mutex<Vec<Collected>>,
    request_calls: Mutex<usize>,
}

impl Recorder {
    fn with_requests(requests: Vec<CollectRequest>) -> Arc<Self> {
        Arc::new(Self {
            // Reversed so `pop` yields them in the order given.
            requests: Mutex::new(requests.into_iter().rev().collect()),
            ..Default::default()
        })
    }

    fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
        m.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn batches(&self) -> std::sync::MutexGuard<'_, Vec<Collected>> {
        Self::lock(&self.batches)
    }

    fn request_calls(&self) -> usize {
        *Self::lock(&self.request_calls)
    }

    /// The peer ports of the entries in batch `idx`, in the order collected.
    fn ports(&self, idx: usize) -> Vec<u16> {
        self.batches()[idx]
            .entries()
            .iter()
            .map(|e| e.peer().port())
            .collect()
    }
}

impl EntryConsumer for Recorder {
    fn request(&self) -> CollectRequest {
        *Self::lock(&self.request_calls) += 1;
        Self::lock(&self.requests)
            .pop()
            .unwrap_or(CollectRequest::Nothing)
    }

    fn consume(&self, collected: Collected) {
        self.batches().push(collected);
    }
}

fn peer(port: u16) -> SocketAddr {
    (Ipv4Addr::LOCALHOST, port).into()
}

/// Builds a map with `collector` registered and the background cleaner stopped, so tests drive
/// `clean` themselves.
fn map_with(
    consumer: Arc<dyn EntryConsumer>,
    capacity: usize,
) -> Arc<State<Clock, tracing::Subscriber>> {
    let map = State::builder()
        .with_signer(stateless_reset::Signer::new(b"secret"))
        .with_capacity(capacity)
        .with_clock(Clock)
        .with_subscriber(tracing::Subscriber::default())
        .with_entry_consumer(consumer)
        .build()
        .unwrap();
    map.cleaner().stop();
    map
}

fn insert_ports(state: &State<Clock, tracing::Subscriber>, ports: impl IntoIterator<Item=u16>) {
    for port in ports {
        state.test_insert(Entry::fake(peer(port), None));
    }
}

/// `Nothing` collects nothing, and `collect` is not called at all.
#[test]
fn nothing_collects_nothing() {
    let recorder = Recorder::with_requests(vec![CollectRequest::Nothing]);
    let map = map_with(recorder.clone(), 50);

    insert_ports(&map, 1..=5);
    map.cleaner().clean(&map, 10);

    assert_eq!(recorder.request_calls(), 1, "request is always consulted");
    assert!(
        recorder.batches().is_empty(),
        "collect must not be called for a Nothing request"
    );
}

/// With no collector registered the cleaner behaves as before.
#[test]
fn no_collector_registered() {
    let map = State::builder()
        .with_signer(stateless_reset::Signer::new(b"secret"))
        .with_capacity(50)
        .with_clock(Clock)
        .with_subscriber(tracing::Subscriber::default())
        .build()
        .unwrap();
    map.cleaner().stop();

    insert_ports(&map, 1..=5);
    map.cleaner().clean(&map, 10);

    assert_eq!(map.ids.len(), 5);
}

/// `Snapshot` collects every live entry, in insertion order.
#[test]
fn snapshot_collects_all_in_insertion_order() {
    let recorder = Recorder::with_requests(vec![CollectRequest::Snapshot]);
    let map = map_with(recorder.clone(), 50);

    let ports = [7u16, 3, 9, 1, 5];
    insert_ports(&map, ports);
    map.cleaner().clean(&map, 10);

    assert_eq!(recorder.batches().len(), 1);
    assert_eq!(
        recorder.ports(0),
        ports.to_vec(),
        "entries must arrive in insertion order, not sorted or hashed order"
    );
    assert_eq!(recorder.batches()[0].request(), CollectRequest::Snapshot);
    assert!(!recorder.batches()[0].is_truncated());
    assert_eq!(recorder.batches()[0].skipped(), 0);
}

/// A non-`Nothing` request that matches nothing still produces an empty batch, so a consumer can
/// tell "nothing matched" from "never asked".
#[test]
fn empty_batch_is_still_delivered() {
    let recorder = Recorder::with_requests(vec![CollectRequest::Snapshot]);
    let map = map_with(recorder.clone(), 50);

    map.cleaner().clean(&map, 10);

    assert_eq!(recorder.batches().len(), 1, "collect must still be called");
    assert!(recorder.batches()[0].entries().is_empty());
}

/// `Journal { since }` collects only entries created at or after the cutoff.
#[test]
fn journal_respects_cutoff() {
    let recorder = Arc::new(Recorder::default());
    let map = map_with(recorder.clone(), 50);

    insert_ports(&map, 1..=3);

    // Sleep so the cutoff is strictly after the first group's creation times.
    std::thread::sleep(std::time::Duration::from_millis(5));
    let cutoff = Instant::now();
    std::thread::sleep(std::time::Duration::from_millis(5));

    insert_ports(&map, 10..=12);

    *Recorder::lock(&recorder.requests) = vec![CollectRequest::Journal { since: cutoff }];

    map.cleaner().clean(&map, 10);

    assert_eq!(recorder.batches().len(), 1);
    assert_eq!(
        recorder.ports(0),
        vec![10, 11, 12],
        "only entries created at or after the cutoff are collected"
    );
}

/// Retired entries are excluded, even while still retained in the queue.
#[test]
fn retired_entries_are_excluded() {
    let recorder = Recorder::with_requests(vec![CollectRequest::Snapshot]);
    let map = map_with(recorder.clone(), 50);

    insert_ports(&map, [1, 2, 3]);

    // Retire the middle entry. A high eviction_cycles keeps it in the queue.
    map.peers.get(peer(2)).unwrap().retire(Epoch(1));

    map.cleaner().clean(&map, 1_000);

    assert_eq!(
        recorder.ports(0),
        vec![1, 3],
        "a retired entry must not be collected: restoring it would resurrect a superseded secret"
    );
}

/// An entry evicted by the same pass is never collected.
#[test]
fn evicted_entries_are_not_collected() {
    let recorder = Recorder::with_requests(vec![CollectRequest::Snapshot]);
    let map = map_with(recorder.clone(), 50);

    insert_ports(&map, [1, 2, 3]);

    let doomed = map.peers.get(peer(2)).unwrap();
    doomed.retire(Epoch(1));

    // eviction_cycles of 0 evicts anything already retired.
    map.cleaner().clean(&map, 0);

    assert_eq!(
        recorder.ports(0),
        vec![1, 3],
        "an entry evicted by this pass must not be handed out"
    );
}

/// Order is preserved across a pass that removes entries from the middle of the queue: `retain`
/// closes the gap without reordering the survivors.
#[test]
fn order_survives_eviction_from_the_middle() {
    let recorder = Recorder::with_requests(vec![CollectRequest::Nothing, CollectRequest::Snapshot]);
    let map = map_with(recorder.clone(), 50);

    insert_ports(&map, [1, 2, 3, 4, 5, 6, 7]);

    for port in [3u16, 5] {
        map.peers.get(peer(port)).unwrap().retire(Epoch(1));
    }
    // First pass evicts them, second collects the survivors.
    map.cleaner().clean(&map, 0);
    map.cleaner().clean(&map, 0);

    assert_eq!(
        recorder.ports(0),
        vec![1, 2, 4, 6, 7],
        "survivors must remain in insertion order after a gap is closed"
    );
}

/// Entries inserted later appear later in collection order.
#[test]
fn later_insertions_collect_later() {
    let recorder =
        Recorder::with_requests(vec![CollectRequest::Snapshot, CollectRequest::Snapshot]);
    let map = map_with(recorder.clone(), 50);

    insert_ports(&map, [1, 2]);
    map.cleaner().clean(&map, 10);

    insert_ports(&map, [3, 4]);
    map.cleaner().clean(&map, 10);

    assert_eq!(recorder.ports(0), vec![1, 2]);
    assert_eq!(
        recorder.ports(1),
        vec![1, 2, 3, 4],
        "entries inserted later must appear later in collection order"
    );
}

/// A batch that fills its buffer stops collecting, counts what it skipped, and reports a resume
/// point.
///
/// Drives [`Collector`] directly: the cleaner sizes the buffer from a current entry count, so it
/// does not truncate in practice. This exercises the mechanism that protects it if it ever does.
#[test]
fn truncation_reports_a_resume_point() {
    let mut collector = Collector::new(CollectRequest::Snapshot, 0).unwrap();
    let capacity = collector.entries.capacity();

    let entries: Vec<_> = (0..(capacity + 4))
        .map(|port| Entry::fake(peer(port as u16), None))
        .collect();

    for entry in &entries {
        collector.push(entry.clone());
    }

    let collected = collector.finish();

    assert_eq!(collected.entries().len(), capacity);
    assert_eq!(collected.skipped(), 4);
    assert!(collected.is_truncated());
    assert_eq!(
        collected.resume_from(),
        entries[capacity].creation_time(),
        "resume_from must be the earliest skipped creation time"
    );

    // Resuming from it collects exactly what was skipped.
    let mut resumed = Collector::new(
        CollectRequest::Journal {
            since: collected.resume_from(),
        },
        entries.len(),
    )
        .unwrap();
    for entry in &entries {
        resumed.push(entry.clone());
    }
    let resumed = resumed.finish();

    assert_eq!(
        resumed.entries().len(),
        4,
        "resuming must pick up exactly the skipped entries"
    );
    assert!(!resumed.is_truncated());
}

/// The resume point is the *earliest* skipped creation time, not the first entry that failed to fit.
///
/// `Entry` timestamps itself before taking the queue lock, so an entry can be enqueued after one
/// that was created later than it. Resuming from the first entry pushed would strand the earlier
/// one permanently.
#[test]
fn resume_point_is_the_earliest_skipped_not_the_first() {
    let mut collector = Collector::new(CollectRequest::Snapshot, 0).unwrap();
    let capacity = collector.entries.capacity();

    for port in 0..capacity {
        collector.push(Entry::fake(peer(port as u16), None));
    }

    let created_first = Entry::fake(peer(1), None);
    std::thread::sleep(std::time::Duration::from_millis(2));
    let created_later = Entry::fake(peer(2), None);

    // Push the later-created one first, mimicking a queue whose order disagrees with creation time.
    collector.push(created_later.clone());
    collector.push(created_first.clone());

    let collected = collector.finish();

    assert_eq!(collected.skipped(), 2);
    assert_eq!(
        collected.resume_from(),
        created_first.creation_time(),
        "resume_from must be the earliest skipped time, or the earlier entry is stranded"
    );
    assert!(
        collected.resume_from() < created_later.creation_time(),
        "resuming from the first entry pushed would skip the earlier one"
    );
}

/// A `Nothing` request allocates no buffer at all.
#[test]
fn nothing_allocates_no_collector() {
    assert!(Collector::new(CollectRequest::Nothing, 10_000).is_none());
}

/// Journal buffers are sized from the live entry count, not a fixed constant, so a journal resuming
/// after a failed cycle does not reallocate under the mutex.
#[test]
fn journal_buffer_is_sized_from_live_entries() {
    let live = 250_000;
    let collector = Collector::new(
        CollectRequest::Journal {
            since: Instant::now(),
        },
        live,
    )
        .unwrap();

    assert!(
        collector.entries.capacity() >= live,
        "a journal must hold every live entry without growing"
    );
}

/// A complete batch resumes just past the newest entry it collected, so a journal chained from it
/// re-reads nothing.
#[test]
fn complete_batch_resumes_past_the_newest_collected() {
    let mut collector = Collector::new(CollectRequest::Snapshot, 16).unwrap();

    let first = Entry::fake(peer(1), None);
    std::thread::sleep(std::time::Duration::from_millis(2));
    let newest = Entry::fake(peer(2), None);

    collector.push(first.clone());
    collector.push(newest.clone());

    let collected = collector.finish();

    assert!(!collected.is_truncated());
    assert_eq!(collected.skipped(), 0);
    assert!(
        collected.resume_from() > newest.creation_time(),
        "a complete batch must resume strictly after its newest entry"
    );

    // Chaining a journal from it collects neither entry again.
    let mut next = Collector::new(
        CollectRequest::Journal {
            since: collected.resume_from(),
        },
        16,
    )
        .unwrap();
    next.push(first);
    next.push(newest);

    assert!(
        next.finish().entries().is_empty(),
        "chaining from resume_from must not re-collect what the previous batch carried"
    );
}

/// The resume point of a complete batch follows the newest entry collected, not the last one pushed.
///
/// Same concurrency skew as the truncated case: the final entry in queue order can be older than one
/// before it, and resuming from that older time would re-collect the newer one every cycle.
#[test]
fn complete_batch_resume_point_follows_the_newest_not_the_last() {
    let mut collector = Collector::new(CollectRequest::Snapshot, 16).unwrap();

    let newest = Entry::fake(peer(1), None);
    std::thread::sleep(std::time::Duration::from_millis(2));
    let pushed_last = Entry::fake(peer(2), None);

    // Push in an order that disagrees with creation time: the newer one first.
    collector.push(pushed_last.clone());
    collector.push(newest.clone());

    let collected = collector.finish();

    assert!(
        collected.resume_from() > pushed_last.creation_time(),
        "resume_from must clear the newest entry collected, not merely the last one pushed"
    );
}

/// A batch that collected nothing still yields a usable resume point, and one that advances, so an
/// idle cycle does not leave a consumer re-reading the same window forever.
#[test]
fn empty_batch_still_advances_the_resume_point() {
    let before = Instant::now();
    let collector = Collector::new(CollectRequest::Snapshot, 16).unwrap();
    let collected = collector.finish();

    assert!(collected.entries().is_empty());
    assert!(!collected.is_truncated());
    assert!(
        collected.resume_from() >= before,
        "an empty batch resumes from when the walk began"
    );

    // An entry created after the walk is still eligible next cycle.
    let later = Entry::fake(peer(1), None);
    let mut next = Collector::new(
        CollectRequest::Journal {
            since: collected.resume_from(),
        },
        16,
    )
        .unwrap();
    next.push(later);
    assert_eq!(
        next.finish().entries().len(),
        1,
        "an empty cycle must not skip entries created after it"
    );
}

/// `is_truncated` reflects `skipped`, not the presence of a resume point: every batch has one.
#[test]
fn is_truncated_tracks_skipped() {
    let complete = Collector::new(CollectRequest::Snapshot, 16)
        .unwrap()
        .finish();
    assert_eq!(complete.skipped(), 0);
    assert!(!complete.is_truncated());

    let mut full = Collector::new(CollectRequest::Snapshot, 0).unwrap();
    for port in 0..full.entries.capacity() {
        full.push(Entry::fake(peer(port as u16), None));
    }
    let before_overflow = full.entries.len();
    full.push(Entry::fake(peer(0), None));
    let truncated = full.finish();

    assert_eq!(
        truncated.entries().len(),
        before_overflow,
        "buffer never grows"
    );
    assert_eq!(truncated.skipped(), 1);
    assert!(truncated.is_truncated());
}

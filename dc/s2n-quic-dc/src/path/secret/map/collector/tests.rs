// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::{
    event::{self, tracing},
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

// An `EntryConsumer` whose `request` is scripted: each call pops the next queued response and
// defaults to `Nothing` once exhausted, so one test can drive several passes with different asks.
#[derive(Default)]
struct Recorder {
    requests: Mutex<Vec<CollectRequest>>,
    batches: Mutex<Vec<Collected>>,
    request_calls: Mutex<usize>,
}

impl Recorder {
    fn with_requests(requests: Vec<CollectRequest>) -> Arc<Self> {
        Arc::new(Self {
            requests: Mutex::new(requests.into_iter().rev().collect()), // so `pop` yields in order
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

fn insert_ports(state: &State<Clock, tracing::Subscriber>, ports: impl IntoIterator<Item = u16>) {
    for port in ports {
        state.test_insert(Entry::fake(peer(port), None));
    }
}

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

#[test]
fn empty_batch_is_still_delivered() {
    let recorder = Recorder::with_requests(vec![CollectRequest::Snapshot]);
    let map = map_with(recorder.clone(), 50);

    map.cleaner().clean(&map, 10);

    assert_eq!(recorder.batches().len(), 1, "collect must still be called");
    assert!(recorder.batches()[0].entries().is_empty());
}

#[test]
fn journal_respects_cutoff() {
    let recorder = Arc::new(Recorder::default());
    let map = map_with(recorder.clone(), 50);

    insert_ports(&map, 1..=3);

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

#[test]
fn retired_entries_are_excluded() {
    let recorder = Recorder::with_requests(vec![CollectRequest::Snapshot]);
    let map = map_with(recorder.clone(), 50);

    insert_ports(&map, [1, 2, 3]);

    map.peers.get(peer(2)).unwrap().retire(Epoch(1));

    map.cleaner().clean(&map, 1_000);

    assert_eq!(
        recorder.ports(0),
        vec![1, 3],
        "a retired entry must not be collected: restoring it would resurrect a superseded secret"
    );
}

#[test]
fn evicted_entries_are_not_collected() {
    let recorder = Recorder::with_requests(vec![CollectRequest::Snapshot]);
    let map = map_with(recorder.clone(), 50);

    insert_ports(&map, [1, 2, 3]);

    let doomed = map.peers.get(peer(2)).unwrap();
    doomed.retire(Epoch(1));

    map.cleaner().clean(&map, 0);

    assert_eq!(
        recorder.ports(0),
        vec![1, 3],
        "an entry evicted by this pass must not be handed out"
    );
}

#[test]
fn order_survives_eviction_from_the_middle() {
    // One response per pass below, so the first pass evicts while collecting nothing and the
    // second collects from the queue `retain` has already compacted -- the case under test. Only
    // the `Snapshot` pass produces a batch, so `ports(0)` is the second one.
    let recorder = Recorder::with_requests(vec![CollectRequest::Nothing, CollectRequest::Snapshot]);
    let map = map_with(recorder.clone(), 50);

    insert_ports(&map, [1, 2, 3, 4, 5, 6, 7]);

    for port in [3u16, 5] {
        map.peers.get(peer(port)).unwrap().retire(Epoch(1));
    }
    map.cleaner().clean(&map, 0);
    map.cleaner().clean(&map, 0);

    assert_eq!(
        recorder.ports(0),
        vec![1, 2, 4, 6, 7],
        "survivors must remain in insertion order after a gap is closed"
    );
}

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

#[test]
// Drives `Collector` directly: the cleaner sizes the buffer from a current entry count, so it
// does not truncate in practice. This exercises the mechanism that protects it if it ever does.
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

#[test]
fn nothing_allocates_no_collector() {
    assert!(Collector::new(CollectRequest::Nothing, 10_000).is_none());
}

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

#[test]
fn complete_batch_resume_point_follows_the_newest_not_the_last() {
    let mut collector = Collector::new(CollectRequest::Snapshot, 16).unwrap();

    let created_first = Entry::fake(peer(1), None);
    std::thread::sleep(std::time::Duration::from_millis(2));
    let created_later = Entry::fake(peer(2), None);

    collector.push(created_later.clone());
    collector.push(created_first.clone());

    let collected = collector.finish();

    assert!(
        collected.resume_from() > created_later.creation_time(),
        "resume_from must clear the newest entry collected, not merely the last one pushed"
    );
}

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

#[derive(Default)]
struct CycleRecorder {
    cycles: Mutex<Vec<(u64, u64)>>,
}

impl CycleRecorder {
    fn cycles(&self) -> Vec<(u64, u64)> {
        Recorder::lock(&self.cycles).clone()
    }
}

impl event::Subscriber for CycleRecorder {
    type ConnectionContext = ();

    fn create_connection_context(
        &self,
        _meta: &event::api::ConnectionMeta,
        _info: &event::api::ConnectionInfo,
    ) -> Self::ConnectionContext {
    }

    fn on_path_secret_map_cleaner_cycled(
        &self,
        _meta: &event::api::EndpointMeta,
        event: &event::api::PathSecretMapCleanerCycled,
    ) {
        Recorder::lock(&self.cycles).push((event.entries_inserted, event.entries_journaled));
    }
}

struct Cycles {
    map: Arc<State<Clock, Arc<CycleRecorder>>>,
    recorder: Arc<Recorder>,
    cycles: Arc<CycleRecorder>,
    previous: u64,
}

impl Cycles {
    fn new(requests: Vec<CollectRequest>, entries: Vec<Arc<Entry>>) -> Self {
        let recorder = Recorder::with_requests(requests);
        let cycles = Arc::new(CycleRecorder::default());
        let map = State::builder()
            .with_signer(stateless_reset::Signer::new(b"secret"))
            .with_capacity(50)
            .with_clock(Clock)
            .with_subscriber(cycles.clone())
            .with_entry_consumer(recorder.clone())
            .with_entries(entries)
            .build()
            .unwrap();
        map.cleaner().stop();

        Self {
            map,
            recorder,
            cycles,
            previous: 0,
        }
    }

    fn insert(&self, port: u16) {
        self.map.test_insert(Entry::fake(peer(port), None));
    }

    fn clean(&mut self) {
        self.previous = self.map.cleaner().clean_since(&self.map, 10, self.previous);
    }

    fn journal_from_last_batch(&self) {
        let since = self.recorder.batches().last().unwrap().resume_from();
        Recorder::lock(&self.recorder.requests).push(CollectRequest::Journal { since });
    }

    fn reported(&self) -> Vec<(u64, u64)> {
        self.cycles.cycles()
    }
}

#[test]
fn a_journal_reports_insertions_since_the_previous_cycle() {
    let mut c = Cycles::new(vec![CollectRequest::Snapshot], vec![]);

    c.clean();
    c.journal_from_last_batch();

    for port in 1..=3 {
        c.insert(port);
    }
    c.clean();

    assert_eq!(c.reported()[1], (3, 3));

    c.journal_from_last_batch();
    c.clean();

    assert_eq!(
        c.reported()[2],
        (0, 0),
        "the window does not carry forward what the previous journal already reported"
    );
}

#[test]
fn a_journal_that_misses_an_insertion_reports_fewer_than_were_inserted() {
    let mut c = Cycles::new(vec![CollectRequest::Snapshot], vec![]);

    let stalled = Entry::fake(peer(1), None);
    c.insert(2);
    c.clean();

    c.journal_from_last_batch();
    c.map.test_insert(stalled);
    c.clean();

    assert_eq!(c.reported()[1], (1, 0));
}

#[test]
fn a_snapshot_cycle_reports_nothing() {
    let mut c = Cycles::new(vec![CollectRequest::Snapshot], vec![]);

    for port in 1..=3 {
        c.insert(port);
    }
    c.clean();

    assert_eq!(c.recorder.batches()[0].entries().len(), 3);
    assert_eq!(c.reported()[0], (0, 0));
}

#[test]
fn a_journal_after_a_snapshot_covers_only_its_own_window() {
    let mut c = Cycles::new(vec![CollectRequest::Snapshot], vec![]);

    c.insert(1);
    c.clean();

    c.journal_from_last_batch();
    c.insert(2);
    c.clean();

    assert_eq!(c.reported()[1], (1, 1));
}

#[test]
fn a_nothing_cycle_does_not_spend_its_window() {
    let mut c = Cycles::new(vec![CollectRequest::Snapshot], vec![]);

    c.clean();
    c.journal_from_last_batch();

    c.insert(1);
    c.clean();
    assert_eq!(c.reported()[1], (1, 1));

    let since = c.recorder.batches().last().unwrap().resume_from();
    c.insert(2);
    c.clean();
    assert_eq!(
        c.reported()[2],
        (0, 0),
        "a Nothing cycle reports nothing and calls no consumer"
    );

    Recorder::lock(&c.recorder.requests).push(CollectRequest::Journal { since });
    c.clean();
    assert_eq!(
        c.reported()[3],
        (1, 1),
        "the skipped window's insertion is reported by the journal that collects it"
    );
}

#[test]
fn restored_entries_are_not_counted_as_insertions() {
    let restored = vec![Entry::fake(peer(1), None), Entry::fake(peer(2), None)];
    let mut c = Cycles::new(vec![CollectRequest::Snapshot], restored);

    c.clean();
    c.journal_from_last_batch();
    c.clean();

    assert_eq!(c.reported()[1], (0, 0));
}

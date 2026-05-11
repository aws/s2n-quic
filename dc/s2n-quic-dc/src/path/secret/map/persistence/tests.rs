// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::{
    credentials::Id,
    event,
    path::secret::{map::store::Store as _, stateless_reset},
};
use s2n_quic_core::time::NoopClock;
use std::{
    net::SocketAddr,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

fn fake_entry(port: u16) -> PersistedEntry {
    let mut id_bytes = [0u8; 16];
    id_bytes[0..2].copy_from_slice(&port.to_be_bytes());
    PersistedEntry {
        peer: SocketAddr::from(([127, 0, 0, 1], port)),
        credential_id: Id::from(id_bytes),
    }
}

#[derive(Default, Clone)]
struct MockObserver {
    visited: Arc<Mutex<Vec<PersistedEntry>>>,
    cycles_completed: Arc<AtomicU32>,
}

impl PersistenceObserver for MockObserver {
    fn on_entry_visited(&self, entry: &PersistedEntry) {
        self.visited
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(*entry);
    }

    fn on_cycle_complete(&self) {
        self.cycles_completed.fetch_add(1, Ordering::Relaxed);
    }
}

fn build_state_with_observer(
    capacity: usize,
    observer: Arc<dyn PersistenceObserver>,
) -> Arc<crate::path::secret::map::state::State<NoopClock, event::tracing::Subscriber>> {
    crate::path::secret::map::state::State::builder()
        .with_signer(stateless_reset::Signer::new(b"test-secret"))
        .with_capacity(capacity)
        .with_clock(NoopClock)
        .with_subscriber(event::tracing::Subscriber::default())
        .with_persistence_observer(observer)
        .build()
        .unwrap()
}

fn build_state(
    capacity: usize,
) -> Arc<crate::path::secret::map::state::State<NoopClock, event::tracing::Subscriber>> {
    crate::path::secret::map::state::State::builder()
        .with_signer(stateless_reset::Signer::new(b"test-secret"))
        .with_capacity(capacity)
        .with_clock(NoopClock)
        .with_subscriber(event::tracing::Subscriber::default())
        .build()
        .unwrap()
}

#[test]
fn observer_wiring_single_thread() {
    let observer = MockObserver::default();
    let state = build_state_with_observer(100, Arc::new(observer.clone()));
    state.cleaner().stop();

    let entry = crate::path::secret::map::Entry::fake("127.0.0.1:1001".parse().unwrap(), None);
    state.test_insert(entry.clone());

    let entry2 = crate::path::secret::map::Entry::fake("127.0.0.1:1002".parse().unwrap(), None);
    state.test_insert(entry2.clone());

    state.cleaner().clean(&state, 0);

    let visited = observer.visited.lock().unwrap();
    assert_eq!(visited.len(), 2, "each live entry should be visited once");

    let visited_peers: std::collections::HashSet<SocketAddr> =
        visited.iter().map(|e| e.peer).collect();
    assert!(visited_peers.contains(&"127.0.0.1:1001".parse::<SocketAddr>().unwrap()));
    assert!(visited_peers.contains(&"127.0.0.1:1002".parse::<SocketAddr>().unwrap()));

    drop(visited);

    assert_eq!(
        observer.cycles_completed.load(Ordering::Relaxed),
        1,
        "on_cycle_complete should be called exactly once per clean()"
    );
}

#[test]
fn observer_wiring_under_concurrency() {
    let observer = MockObserver::default();
    let state = build_state_with_observer(500, Arc::new(observer.clone()));
    state.cleaner().stop();

    let n_threads = 4usize;
    let entries_per_thread = 25usize;
    let mut handles = Vec::with_capacity(n_threads);

    for tid in 0..n_threads {
        let state = state.clone();
        handles.push(std::thread::spawn(move || {
            for i in 0..entries_per_thread {
                let port = (tid * entries_per_thread + i + 1) as u16;
                let entry = crate::path::secret::map::Entry::fake(
                    SocketAddr::from(([127, 0, 0, 1], port)),
                    None,
                );
                state.test_insert(entry);
            }
        }));
    }

    for h in handles {
        h.join().unwrap();
    }

    let total_inserted = n_threads * entries_per_thread;

    state.cleaner().clean(&state, 0);

    let visited = observer.visited.lock().unwrap();
    assert_eq!(
        visited.len(),
        total_inserted,
        "all {total_inserted} live entries should be visited"
    );

    let unique_ids: std::collections::HashSet<Id> =
        visited.iter().map(|e| e.credential_id).collect();
    assert_eq!(
        unique_ids.len(),
        total_inserted,
        "each visited entry should have a unique credential_id"
    );

    drop(visited);

    assert_eq!(
        observer.cycles_completed.load(Ordering::Relaxed),
        1,
        "exactly one on_cycle_complete per clean() call"
    );
}

#[test]
fn observer_multiple_cycles() {
    let observer = MockObserver::default();
    let state = build_state_with_observer(100, Arc::new(observer.clone()));
    state.cleaner().stop();

    let entry = crate::path::secret::map::Entry::fake("127.0.0.1:1001".parse().unwrap(), None);
    state.test_insert(entry);

    state.cleaner().clean(&state, 0);
    state.cleaner().clean(&state, 0);
    state.cleaner().clean(&state, 0);

    assert_eq!(
        observer.cycles_completed.load(Ordering::Relaxed),
        3,
        "on_cycle_complete should be called once per clean() call"
    );

    let visited = observer.visited.lock().unwrap();
    assert_eq!(
        visited.len(),
        3,
        "the single live entry should be visited once per cycle"
    );
}

#[test]
fn replay_empty_list() {
    let state = build_state(10);
    let result = state.replay_unknown_path_secrets(vec![], 1000, Duration::from_secs(30));
    assert_eq!(result, ReplayResult::default());
    assert_eq!(result.sent, 0);
    assert_eq!(result.failed, 0);
    assert_eq!(result.remaining, 0);
}

#[test]
fn replay_rate_pacing_within_tolerance() {
    let state = build_state(200);

    let entries: Vec<PersistedEntry> = (0..100u16).map(|i| fake_entry(i + 1)).collect();

    let rate_pps = 1000u32;
    let timeout = Duration::from_secs(30);

    let start = std::time::Instant::now();
    let result = state.replay_unknown_path_secrets(entries, rate_pps, timeout);
    let elapsed = start.elapsed();

    // 100 entries at 1000 pps => ~99ms ideal (99 sleeps of 1ms each)
    // Allow generous tolerance: 50ms to 300ms
    assert!(
        elapsed >= Duration::from_millis(50),
        "replay took {elapsed:?}, expected at least ~50ms"
    );
    assert!(
        elapsed <= Duration::from_millis(300),
        "replay took {elapsed:?}, expected at most ~300ms"
    );

    assert_eq!(
        result.sent + result.failed,
        100,
        "all 100 entries should be attempted"
    );
    assert_eq!(result.remaining, 0, "none should remain with 30s timeout");
}

#[test]
fn replay_timeout_returns_remaining() {
    let state = build_state(20_000);

    let entries: Vec<PersistedEntry> = (0..10_000u16)
        .map(|i| {
            let mut id_bytes = [0u8; 16];
            id_bytes[0..2].copy_from_slice(&i.to_be_bytes());
            PersistedEntry {
                peer: SocketAddr::from(([10, 0, (i >> 8) as u8, (i & 0xff) as u8], 9000)),
                credential_id: Id::from(id_bytes),
            }
        })
        .collect();

    let rate_pps = 10u32;
    let timeout = Duration::from_millis(500);

    let result = state.replay_unknown_path_secrets(entries, rate_pps, timeout);

    assert!(
        result.remaining > 0,
        "with 10000 entries at 10 pps and 500ms timeout, many should remain; got remaining={}",
        result.remaining
    );

    assert_eq!(
        result.sent + result.failed + result.remaining,
        10_000,
        "sent + failed + remaining must equal total entries"
    );
}

#[test]
fn replay_sends_to_loopback() {
    let state = build_state(10);

    let entries = vec![fake_entry(1)];
    let result = state.replay_unknown_path_secrets(entries, 1000, Duration::from_secs(5));

    // With a real loopback socket, the packet should be sent (to a loopback
    // address nobody is listening on — send_to on UDP doesn't fail for
    // unreachable destinations).
    assert_eq!(
        result.sent + result.failed,
        1,
        "exactly one entry should be attempted"
    );
    assert_eq!(result.remaining, 0);
}

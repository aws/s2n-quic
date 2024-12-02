// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::state::State;
use crate::{
    event::{self, EndpointPublisher as _},
    path::secret::map::store::Store,
};
use rand::Rng as _;
use s2n_quic_core::time;
use std::{
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

const EVICTION_CYCLES: u64 = if cfg!(test) { 0 } else { 10 };

pub struct Cleaner {
    should_stop: AtomicBool,
    thread: Mutex<Option<std::thread::JoinHandle<()>>>,
    epoch: AtomicU64,
}

impl Drop for Cleaner {
    fn drop(&mut self) {
        self.stop();
    }
}

impl Cleaner {
    pub fn new() -> Cleaner {
        Cleaner {
            should_stop: AtomicBool::new(false),
            thread: Mutex::new(None),
            epoch: AtomicU64::new(1),
        }
    }

    pub fn stop(&self) {
        self.should_stop.store(true, Ordering::Relaxed);
        if let Some(thread) =
            std::mem::take(&mut *self.thread.lock().unwrap_or_else(|e| e.into_inner()))
        {
            thread.thread().unpark();

            // If this isn't getting dropped on the cleaner thread,
            // then wait for the background thread to finish exiting.
            if std::thread::current().id() != thread.thread().id() {
                // We expect this to terminate very quickly.
                thread.join().unwrap();
            }
        }
    }

    pub fn spawn_thread<C, S>(&self, state: Arc<State<C, S>>)
    where
        C: 'static + time::Clock + Send + Sync,
        S: event::Subscriber,
    {
        let state = Arc::downgrade(&state);
        let handle = std::thread::Builder::new()
            .name("dc_quic::cleaner".into())
            .spawn(move || loop {
                let Some(state) = state.upgrade() else {
                    break;
                };
                if state.cleaner().should_stop.load(Ordering::Relaxed) {
                    break;
                }
                state.cleaner().clean(&state, EVICTION_CYCLES);
                // in tests, we should try and be deterministic
                let pause = if cfg!(test) {
                    60
                } else {
                    rand::thread_rng().gen_range(5..60)
                };
                drop(state);
                std::thread::park_timeout(Duration::from_secs(pause));
            })
            .unwrap();
        *self.thread.lock().unwrap() = Some(handle);
    }

    /// Periodic maintenance for various maps.
    pub fn clean<C, S>(&self, state: &State<C, S>, eviction_cycles: u64)
    where
        C: 'static + time::Clock + Send + Sync,
        S: event::Subscriber,
    {
        let current_epoch = self.epoch.fetch_add(1, Ordering::Relaxed);
        let now = Instant::now();

        let utilization = |count: usize| (count as f32 / state.secrets_capacity() as f32) * 100.0;

        let mut initial_ids_count = 0usize;
        let mut initial_addresses_count = 0usize;
        let mut retired_ids_count = 0usize;
        let mut retired_addresses_count = 0usize;

        // For non-retired entries, if it's time for them to handshake again, request a
        // handshake to happen. This handshake will currently happen on the next request for this
        // particular peer.
        state.ids.retain(|_, entry| {
            initial_ids_count += 1;

            let retained = if let Some(retired_at) = entry.retired_at() {
                // retain if we aren't yet ready to evict.
                current_epoch.saturating_sub(retired_at) < eviction_cycles
            } else {
                if entry.rehandshake_time() <= now {
                    state.request_handshake(*entry.peer());
                }

                // always retain
                true
            };

            if !retained {
                retired_ids_count += 1;
            }

            retained
        });

        // Drop IP entries if we no longer have the path secret ID entry.
        // FIXME: Don't require a loop to do this. This is likely somewhat slow since it takes a
        // write lock + read lock essentially per-entry, but should be near-constant-time.
        state.peers.retain(|_, entry| {
            initial_addresses_count += 1;

            let retained = state.ids.contains_key(entry.id());

            if !retained {
                retired_addresses_count += 1;
            }

            retained
        });

        // Iteration order should be effectively random, so this effectively just prunes the list
        // periodically. 5000 is chosen arbitrarily to make sure this isn't a memory leak. Note
        // that peers the application is actively interested in will typically bypass this list, so
        // this is mostly a risk of delaying regular re-handshaking with very large cardinalities.
        //
        // FIXME: Long or mid-term it likely makes sense to replace this data structure with a
        // fuzzy set of some kind and/or just moving to immediate background handshake attempts.
        const MAX_REQUESTED_HANDSHAKES: usize = 5000;

        let mut handshake_requests_count = 0usize;
        let mut retired_handshake_requests_count = 0usize;
        state.requested_handshakes.pin().retain(|_| {
            handshake_requests_count += 1;
            let retain = handshake_requests_count < MAX_REQUESTED_HANDSHAKES;

            if !retain {
                retired_handshake_requests_count += 1;
            }

            retain
        });

        let entries = initial_ids_count - retired_ids_count;
        let addresses = initial_addresses_count - retired_addresses_count;

        state.subscriber().on_path_secret_map_cleaner_cycled(
            event::builder::PathSecretMapCleanerCycled {
                entries,
                retired_entries: retired_ids_count,
                entries_utilization: utilization(entries),
                entries_initial_utilization: utilization(initial_ids_count),
                addresses,
                addresses_utilization: utilization(addresses),
                addresses_initial_utilization: utilization(initial_addresses_count),
                retired_addresses: retired_addresses_count,
                handshake_requests: handshake_requests_count,
                retired_handshake_requests: retired_handshake_requests_count,
            },
        );
    }

    pub fn epoch(&self) -> u64 {
        self.epoch.load(Ordering::Relaxed)
    }
}

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::state::State;
use crate::{
    event::{self, EndpointPublisher as _},
    path::secret::map::store::Store,
};
use rand::RngExt as _;
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
        // check to see if we're in a simulation before spawning a thread
        #[cfg(any(test, feature = "testing"))]
        if bach::is_active() {
            return;
        }

        let state = Arc::downgrade(&state);
        let handle = std::thread::Builder::new()
            .name("dc_quic::cleaner".into())
            .spawn(move || loop {
                // in tests, we should try and be as deterministic as possible
                let pause = if cfg!(test) {
                    Duration::from_secs(60).as_millis() as u64
                } else {
                    rand::rng().random_range(
                        Duration::from_secs(5).as_millis() as u64
                            ..Duration::from_secs(60).as_millis() as u64,
                    )
                };

                let next_start = Instant::now() + Duration::from_secs(60);
                std::thread::park_timeout(Duration::from_millis(pause));

                let Some(state) = state.upgrade() else {
                    break;
                };
                if state.cleaner().should_stop.load(Ordering::Relaxed) {
                    break;
                }
                state.cleaner().clean(&state, EVICTION_CYCLES);

                // pause the rest of the time to run once a minute, not twice a minute
                std::thread::park_timeout(next_start.saturating_duration_since(Instant::now()));
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
        let start = state.clock.get_time();
        let current_epoch = self.epoch.fetch_add(1, Ordering::Relaxed);

        let utilization = |count: usize| (count as f32 / state.secrets_capacity() as f32) * 100.0;

        let id_entries_initial = state.ids.len();
        let mut id_entries_retired = 0usize;
        let mut id_entries_active = 0usize;
        let address_entries_initial = state.peers.len();
        let mut address_entries_retired = 0usize;
        let mut address_entries_active = 0usize;

        // We want to avoid taking long lived locks which affect gets on the maps (where we want
        // p100 latency to be in microseconds at most).
        //
        // Impeding *handshake* latency is much more acceptable though since this happens at most
        // once a minute and handshakes are of similar magnitude (~milliseconds/handshake, this is
        // also expected to run for single digit milliseconds).
        //
        // Note that we expect the queue to be an exhaustive list of entries - no entry should not
        // be in the queue but be in the maps for more than a few microseconds during a handshake
        // (when we pop from the queue to remove from the maps).
        let mut queue = state
            .eviction_queue
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let in_lock = state.clock.get_time();

        // This map is only accessed with queue lock held and in cleaner, so it is in practice
        // single threaded. No concurrent access is permitted.
        state.cleaner_peer_seen.clear();

        let mut rehandshake = state.rehandshake.lock().unwrap();
        let refill_rehandshakes = rehandshake.needs_refill();

        // FIXME: add metrics for queue depth?
        // These are sort of equivalent to the ID map -- so maybe not worth it for now unless we
        // can get something more interesting out of it.
        queue.retain(|entry| {
            let Some(entry) = entry.upgrade() else {
                return false;
            };

            if entry.take_accessed_id() {
                id_entries_active += 1;
            }

            // Avoid double counting by making sure we have unique peer IPs.
            // We clear/take the accessed bit regardless of whether we're going to count it to
            // preserve the property that every cleaner run snapshots last ~minute.
            if entry.take_accessed_addr()
                && state
                    .cleaner_peer_seen
                    .insert_no_events(entry.clone())
                    .is_none()
            {
                address_entries_active += 1;
            }

            if refill_rehandshakes {
                // We'll dedup after we fill, we preallocate for the max capacity so this shouldn't
                // allocate in practice.
                rehandshake.push(*entry.peer());
            }

            let retained = if let Some(retired_at) = entry.retired_at() {
                // retain if we aren't yet ready to evict.
                current_epoch.saturating_sub(retired_at) < eviction_cycles
            } else {
                // always retain non-retired entries.
                true
            };

            if !retained {
                let (id_removed, peer_removed) = state.evict(&entry);
                if id_removed {
                    id_entries_retired += 1;
                }
                if peer_removed {
                    address_entries_retired += 1;
                }
                return false;
            }

            true
        });

        // Avoid retaining entries for longer than expected.
        state.cleaner_peer_seen.clear();

        drop(queue);
        let handshake_lock_duration = state.clock.get_time().saturating_duration_since(in_lock);

        if refill_rehandshakes {
            rehandshake.adjust_post_refill();
        }

        let mut handshake_requests = 0;
        let handshake_requests_skipped =
            rehandshake.next_rehandshake_batch(state.peers.len(), |peer| {
                handshake_requests += 1;
                state.request_handshake(peer, crate::psk::io::HandshakeReason::Periodic)
            });

        drop(rehandshake);

        let id_entries = state.ids.len();
        let address_entries = state.peers.len();

        state.subscriber().on_path_secret_map_cleaner_cycled(
            event::builder::PathSecretMapCleanerCycled {
                id_entries,
                id_entries_retired,
                id_entries_active,
                id_entries_active_utilization: utilization(id_entries_active),
                id_entries_utilization: utilization(id_entries),
                id_entries_initial_utilization: utilization(id_entries_initial),
                address_entries,
                address_entries_active,
                address_entries_active_utilization: utilization(address_entries_active),
                address_entries_utilization: utilization(address_entries),
                address_entries_initial_utilization: utilization(address_entries_initial),
                address_entries_retired,
                handshake_requests,
                handshake_requests_skipped,
                handshake_lock_duration,
                duration: state.clock.get_time().saturating_duration_since(start),
            },
        );
    }

    pub fn epoch(&self) -> u64 {
        self.epoch.load(Ordering::Relaxed)
    }
}

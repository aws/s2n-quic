// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{rehandshake, state::State};
use crate::{
    clock::Clock,
    event::{self, EndpointPublisher as _},
    path::secret::map::store::Store,
};
use rand::Rng as _;
use std::{
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

#[cfg(test)]
mod tests;

const DEFAULT_EVICTION_CYCLES: u64 = 10;
const TEST_EVICTION_CYCLES: u64 = 0;
const TOKIO_EVICTION_CYCLES: u64 = if cfg!(test) {
    TEST_EVICTION_CYCLES
} else {
    DEFAULT_EVICTION_CYCLES
};
const BACH_EVICTION_CYCLES: u64 = DEFAULT_EVICTION_CYCLES;

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

    pub fn spawn_thread<C, S>(
        &self,
        state: Arc<State<C, S>>,
        rehandshake: rehandshake::RehandshakeState<C>,
    ) where
        C: Clock,
        S: event::Subscriber,
    {
        // check to see if we're in a simulation before spawning a thread
        #[cfg(any(test, feature = "testing"))]
        if bach::is_active() {
            return self.spawn_bach(state, rehandshake);
        }

        self.spawn_tokio(state, rehandshake);
    }

    #[cfg(any(test, feature = "testing"))]
    fn spawn_bach<C, S>(
        &self,
        state: Arc<State<C, S>>,
        mut rehandshake: rehandshake::RehandshakeState<C>,
    ) where
        C: Clock,
        S: event::Subscriber,
    {
        let state = Arc::downgrade(&state);

        let fut = async move {
            use ::bach::{ext::*, time::*};
            loop {
                let pause = (5..60).any();
                let pause = Duration::from_secs(pause);

                let next_start = Instant::now() + Duration::from_secs(60);
                pause.sleep().await;

                let Some(state) = state.upgrade() else {
                    break;
                };
                if state.cleaner().should_stop.load(Ordering::Relaxed) {
                    break;
                }
                state
                    .cleaner()
                    .clean(&state, &mut rehandshake, BACH_EVICTION_CYCLES)
                    .await;

                // pause the rest of the time to run once a minute, not twice a minute
                next_start
                    .saturating_duration_since(Instant::now())
                    .sleep()
                    .await;
            }
        };

        ::bach::task::spawn(fut);
    }

    fn spawn_tokio<C, S>(
        &self,
        state: Arc<State<C, S>>,
        mut rehandshake: rehandshake::RehandshakeState<C>,
    ) where
        C: Clock,
        S: event::Subscriber,
    {
        let state = Arc::downgrade(&state);

        let fut = async move {
            use tokio::time::{sleep, Instant};

            loop {
                // in tests, we should try and be as deterministic as possible
                let pause = if cfg!(test) {
                    60
                } else {
                    rand::rng().random_range(5..60)
                };
                let pause = Duration::from_secs(pause);

                let next_start = Instant::now() + Duration::from_secs(60);
                sleep(pause).await;

                let Some(state) = state.upgrade() else {
                    break;
                };
                if state.cleaner().should_stop.load(Ordering::Relaxed) {
                    break;
                }
                state
                    .cleaner()
                    .clean(&state, &mut rehandshake, TOKIO_EVICTION_CYCLES)
                    .await;

                // pause the rest of the time to run once a minute, not twice a minute
                sleep(next_start.saturating_duration_since(Instant::now())).await;
            }
        };

        let handle = std::thread::Builder::new()
            .name("dc_quic::cleaner".into())
            .spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap();

                rt.block_on(fut);
            })
            .unwrap();
        *self.thread.lock().unwrap() = Some(handle);
    }

    /// Periodic maintenance for various maps.
    async fn clean<C, S>(
        &self,
        state: &State<C, S>,
        rehandshake: &mut rehandshake::RehandshakeState<C>,
        eviction_cycles: u64,
    ) where
        C: Clock,
        S: event::Subscriber,
    {
        let result = self.process_queue(state, rehandshake, eviction_cycles);

        if result.refill_rehandshakes {
            rehandshake.adjust_post_refill();
        }

        let mut handshake_requests = 0;
        let clock = &state.clock;
        rehandshake
            .next_rehandshake_batch(state.peers.len(), clock, |peer| {
                handshake_requests += 1;
                state.request_handshake(peer);
            })
            .await;

        result.emit(state, handshake_requests);
    }

    fn process_queue<C, S>(
        &self,
        state: &State<C, S>,
        rehandshake: &mut rehandshake::RehandshakeState<C>,
        eviction_cycles: u64,
    ) -> ProcessQueueResult
    where
        C: Clock,
        S: event::Subscriber,
    {
        let current_epoch = self.epoch.fetch_add(1, Ordering::Relaxed);

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

        // This map is only accessed with queue lock held and in cleaner, so it is in practice
        // single threaded. No concurrent access is permitted.
        state.cleaner_peer_seen.clear();

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
            if entry.take_accessed_addr() && state.cleaner_peer_seen.insert(entry.clone()).is_none()
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

        ProcessQueueResult {
            id_entries_initial,
            id_entries_retired,
            id_entries_active,
            address_entries_initial,
            address_entries_active,
            address_entries_retired,
            refill_rehandshakes,
        }
    }

    pub fn epoch(&self) -> u64 {
        self.epoch.load(Ordering::Relaxed)
    }
}

struct ProcessQueueResult {
    id_entries_initial: usize,
    id_entries_retired: usize,
    id_entries_active: usize,
    address_entries_initial: usize,
    address_entries_active: usize,
    address_entries_retired: usize,
    refill_rehandshakes: bool,
}

impl ProcessQueueResult {
    fn emit<C, S>(self, state: &State<C, S>, handshake_requests: usize)
    where
        C: Clock,
        S: event::Subscriber,
    {
        let Self {
            id_entries_initial,
            id_entries_retired,
            id_entries_active,
            address_entries_initial,
            address_entries_active,
            address_entries_retired,
            refill_rehandshakes: _,
        } = self;

        let utilization = |count: usize| (count as f32 / state.secrets_capacity() as f32) * 100.0;

        let id_entries = state.ids.len();
        let address_entries = state.peers.len();

        let event = event::builder::PathSecretMapCleanerCycled {
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
            handshake_requests_retired: 0,
        };

        state.subscriber().on_path_secret_map_cleaner_cycled(event);
    }
}

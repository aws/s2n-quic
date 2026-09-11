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

/// The cleaner loop runs roughly once per this interval. Serialization periods are expressed as a
/// number of these cycles, and the access-time epoch advances once per cycle.
pub(super) const CLEANER_CYCLE: Duration = Duration::from_secs(60);

/// Converts a serialization `period` into a number of cleaner cycles (at least one).
///
/// The cleaner already runs on a jittered ~once-per-minute cadence, so rather than jittering the
/// serialization time separately we pick a random cycle within each window of this many cycles to
/// serialize in. This spreads writes across processes for free.
fn period_in_cycles(period: Duration) -> u64 {
    (period.as_secs() / CLEANER_CYCLE.as_secs()).max(1)
}

pub struct Cleaner {
    should_stop: AtomicBool,
    thread: Mutex<Option<std::thread::JoinHandle<()>>>,
    epoch: AtomicU64,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Epoch(pub(crate) u64);

impl Epoch {
    #[inline]
    pub fn get(self) -> u64 {
        self.0
    }

    pub(crate) fn duration_since(&self, base: Epoch) -> Duration {
        let delta = self.0.saturating_sub(base.0);
        CLEANER_CYCLE.saturating_mul(u32::try_from(delta).unwrap_or(u32::MAX))
    }
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
                #[expect(
                    clippy::unwrap_used,
                    reason = "panicking if a panic already occurred is OK"
                )]
                thread.join().unwrap();
            }
        }
    }

    #[expect(clippy::unwrap_in_result)]
    pub fn spawn_thread<C, S>(&self, state: Arc<State<C, S>>) -> std::io::Result<()>
    where
        C: 'static + time::Clock + Send + Sync,
        S: event::Subscriber,
    {
        // check to see if we're in a simulation before spawning a thread
        #[cfg(any(test, feature = "testing"))]
        if bach::is_active() {
            return Ok(());
        }

        // The serialization period, expressed in cleaner cycles, if configured. The cleaner drives
        // serialization so we don't need a second background thread.
        let serialize_cycles = state
            .serializer
            .as_ref()
            .and_then(|s| s.period())
            .map(period_in_cycles);

        let state = Arc::downgrade(&state);
        let handle = std::thread::Builder::new()
            .name("dc_quic::cleaner".into())
            .spawn(move || {
                // We serialize at most once per `serialize_cycles` window. `cycle` counts cleaner
                // runs within the current window (wrapping back to 0 at the window boundary) and
                // `serialize_at` is the randomly-chosen cycle within the window in which we write.
                // Re-rolled each window so the cleaner's own jitter spreads writes across
                // processes without a separate timer.
                let mut cycle = 0u64;
                let mut serialize_at =
                    serialize_cycles.map(|cycles| rand::rng().random_range(0..cycles));

                loop {
                    // in tests, we should try and be as deterministic as possible
                    let pause = if cfg!(test) {
                        CLEANER_CYCLE.as_millis() as u64
                    } else {
                        rand::rng().random_range(
                            Duration::from_secs(5).as_millis() as u64
                                ..CLEANER_CYCLE.as_millis() as u64,
                        )
                    };

                    let next_start = Instant::now() + CLEANER_CYCLE;
                    std::thread::park_timeout(Duration::from_millis(pause));

                    let Some(state) = state.upgrade() else {
                        break;
                    };
                    if state.cleaner().should_stop.load(Ordering::Relaxed) {
                        break;
                    }
                    state.cleaner().clean(&state, EVICTION_CYCLES);

                    // Serialize the map in the chosen cycle of each window, then advance the cycle
                    // counter and re-roll at the window boundary.
                    if let Some(cycles) = serialize_cycles {
                        if Some(cycle) == serialize_at {
                            if let Err(err) = state.serialize_to_disk() {
                                tracing::warn!(%err, "failed to serialize path secret map to disk");
                            }
                        }

                        cycle += 1;
                        if cycle >= cycles {
                            cycle = 0;
                            serialize_at = Some(rand::rng().random_range(0..cycles));
                        }
                    }

                    // pause the rest of the time to run once a minute, not twice a minute
                    std::thread::park_timeout(next_start.saturating_duration_since(Instant::now()));
                }
            });
        let handle = handle?;
        #[expect(clippy::unwrap_used, reason = "panic only if already panicked")]
        {
            *self.thread.lock().unwrap() = Some(handle);
        }

        Ok(())
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
        // Entries created within the last rehandshake period. This tracks how much of the cache
        // churns over a single rehandshake period, which is useful for understanding capacity
        // pressure relative to the rate at which we re-handshake peers.
        let mut id_entries_in_last_hs_period = 0usize;

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

        #[expect(
            clippy::unwrap_used,
            reason = "lock is only poisoned if another thread already panicked while holding it"
        )]
        let mut rehandshake = state.rehandshake.lock().unwrap();
        let refill_rehandshakes = rehandshake.needs_refill();
        // Compute the cutoff once (rather than calling `Instant::now()` per entry via `age()`) so
        // we can cheaply compare each entry's creation timestamp directly. Entries created at or
        // after this cutoff are considered to have been created within the last rehandshake period.
        let recent_cutoff = Instant::now().checked_sub(rehandshake.rehandshake_period());

        // FIXME: add metrics for queue depth?
        // These are sort of equivalent to the ID map -- so maybe not worth it for now unless we
        // can get something more interesting out of it.
        queue.retain(|entry| {
            let Some(entry) = entry.upgrade() else {
                return false;
            };

            // Entries created within the last rehandshake period. If subtraction underflowed
            // (period exceeds process uptime), all entries are considered recent.
            let in_rehandshake_period =
                recent_cutoff.is_none_or(|cutoff| entry.creation_time() >= cutoff);

            if entry.take_accessed_id() {
                id_entries_active += 1;
            }

            if in_rehandshake_period {
                id_entries_in_last_hs_period += 1;
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
                current_epoch.saturating_sub(retired_at.get()) < eviction_cycles
            } else {
                // always retain non-retired entries.
                true
            };

            if !retained {
                let (id_removed, peer_removed) =
                    state.evict(&entry, event::builder::EvictionReason::Retiring);
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
                id_entries_in_last_hs_period,
                id_entries_in_last_hs_period_utilization: utilization(id_entries_in_last_hs_period),
                handshake_requests,
                handshake_requests_skipped,
                handshake_lock_duration,
                duration: state.clock.get_time().saturating_duration_since(start),
            },
        );
    }

    pub fn epoch(&self) -> Epoch {
        Epoch(self.epoch.load(Ordering::Relaxed))
    }
}

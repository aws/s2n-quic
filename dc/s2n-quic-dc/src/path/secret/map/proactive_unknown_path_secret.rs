// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Paced, proactive emission of [`UnknownPathSecret`] control packets.
//!
//! After a server restart, peers may still hold cached path secrets that this endpoint no longer
//! knows about. Rather than waiting for each peer to discover the restart reactively (by having a
//! request dropped and, in turn, receiving an `UnknownPathSecret` reply), we can proactively tell
//! every persisted peer to re-handshake by sending it an authenticated `UnknownPathSecret` control
//! packet.
//!
//! [`pace_attempts`] contains the transport-agnostic pacing/accounting loop. The actual packet
//! construction and socket send live on the map's `State` (see `state.rs`); this module is kept
//! separate so the pacer can be unit tested without a socket.
//!
//! [`UnknownPathSecret`]: crate::packet::secret_control::UnknownPathSecret

use super::DiskEntry;
use crate::{credentials, packet::secret_control as control, path::secret::stateless_reset};
use core::{num::NonZeroU32, time::Duration};
use s2n_quic_core::time::{Clock, StdClock, Timestamp};
use std::net::SocketAddr;

/// Statistics describing a completed (or deadline-truncated) run of [`Map::send_unknown_path_secrets`].
///
/// The four counters partition the input exactly:
///
/// ```text
/// sent + failed + skipped + remaining == total number of input entries
/// ```
///
/// [`Map::send_unknown_path_secrets`]: crate::path::secret::Map::send_unknown_path_secrets
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct SendStats {
    /// Entries for which an `UnknownPathSecret` packet was successfully written to the socket.
    pub sent: usize,
    /// Entries whose send was attempted but failed (socket error, including `WouldBlock`, which we
    /// drop-and-count rather than retry -- see the module docs / `state.rs`).
    pub failed: usize,
    /// Entries skipped without a send attempt because they carried no credential id (v0 records).
    /// These peers recover reactively; there is no packet we can build for them.
    pub skipped: usize,
    /// Entries never attempted because the deadline passed first.
    ///
    /// This includes an entry that was pulled from the iterator and was pending a send when the
    /// deadline hit: because no send was attempted for it, it counts as `remaining`, not `failed`.
    pub remaining: usize,
}

/// Minimum pacing tick.
///
/// The pacer sends a batch of packets, then sleeps the rest of the tick, so a high rate coalesces
/// many sends into one wakeup rather than sleeping once per packet -- a sleep-per-packet loop is
/// capped by OS sleep granularity (~1ms) to roughly 20-30K packets/sec, below the rate a large
/// (e.g. 500K-peer) map must sustain to drain in tens of seconds. For rates too low to need
/// batching, the tick stretches beyond this floor so the exact rate is still honored.
const MIN_TICK: Duration = Duration::from_millis(5);

/// A [`Clock`] that can also block the current thread until a later instant -- the blocking analog
/// of [`ClockWithTimer`] (which adds an async timer). Injected into [`pace_attempts`] so tests can
/// advance time without real sleeps; production is [`StdClock`]. Bundling the wait with the clock
/// keeps them consistent: a fake that advances itself on `sleep` can't be paired with a real one.
///
/// [`ClockWithTimer`]: s2n_quic_core::time::clock::ClockWithTimer
trait PacingClock: Clock {
    fn sleep(&self, dur: Duration);
}

impl PacingClock for StdClock {
    fn sleep(&self, dur: Duration) {
        std::thread::sleep(dur);
    }
}

/// Builds, signs, and sends one `UnknownPathSecret` packet per entry that has a credential id,
/// paced at approximately `rate` packets per second and stopping once `timeout` of wall-clock time
/// elapses. This is the module's entry point; it paces against a real clock ([`StdClock`]) and
/// [`pace_attempts`] provides the pacing loop.
///
/// `send` performs the actual transmission and any success-only side effect (e.g. emitting the sent
/// event, which needs the map's subscriber); v0 entries (no credential id) are skipped without a
/// packet.
///
/// A `send` error -- including `WouldBlock` backpressure on the shared, non-blocking control
/// socket -- is counted as a failure and not retried (a tight retry would burn CPU and starve the
/// reactive control path; the peer simply recovers reactively). `WouldBlock` is expected under
/// load and counted silently; any other error is logged as genuinely unexpected.
pub(super) fn emit_packets(
    entries: &mut dyn ExactSizeIterator<Item = DiskEntry>,
    rate: NonZeroU32,
    timeout: Duration,
    signer: &stateless_reset::Signer,
    mut send: impl FnMut(credentials::Id, &[u8], &SocketAddr) -> std::io::Result<()>,
) -> SendStats {
    let clock = StdClock::default();
    let deadline = clock.get_time() + timeout;
    pace_attempts(entries, rate, deadline, &clock, |id, peer| {
        let mut buffer = [0u8; control::UnknownPathSecret::MAX_PACKET_SIZE];
        let len = super::encode_unknown_path_secret(&mut buffer, signer, id, None);

        match send(id, &buffer[..len], &peer) {
            Ok(()) => true,
            Err(err) => {
                if err.kind() != std::io::ErrorKind::WouldBlock {
                    tracing::warn!(
                        ?err,
                        credential_id = ?id,
                        "failed to send proactive UnknownPathSecret packet"
                    );
                }
                false
            }
        }
    })
}

/// Rate-limits calls to `attempt`, one per sendable entry, until `deadline` passes.
///
/// `attempt` is invoked for each entry that has a credential id and returns `true` if the packet
/// was sent, `false` if it failed; the outcome is tallied in the returned [`SendStats`]. v0
/// (`None`) entries carry no credential id, so they are counted as `skipped` without an attempt --
/// and without consuming pacing budget, so a file full of v0 records drains instantly rather than
/// being throttled.
///
/// ## Pacing
///
/// Each tick sends up to `batch` packets, then sleeps the remainder of `interval` (>= `MIN_TICK`).
/// `batch / interval` averages `rate` per second; a high rate coalesces many sends into one wakeup
/// (clear of the OS sleep-granularity floor), and a low rate stretches `interval` past `MIN_TICK`
/// to hold the exact rate. The sleep is `interval` minus the time already spent sending, so send
/// time is credited against the tick rather than added on top; and because each tick sends at most
/// `batch`, a stalled thread can't build up a catch-up burst onto the shared control socket.
///
/// `clock` is injected so the loop can be driven deterministically in tests; production passes
/// [`StdClock`].
fn pace_attempts(
    entries: &mut dyn ExactSizeIterator<Item = DiskEntry>,
    rate: NonZeroU32,
    deadline: Timestamp,
    clock: &impl PacingClock,
    mut attempt: impl FnMut(credentials::Id, SocketAddr) -> bool,
) -> SendStats {
    let mut stats = SendStats::default();
    let total = entries.len();

    let rate = u64::from(rate.get());
    let batch = (rate * MIN_TICK.as_millis() as u64).div_ceil(1000).max(1);
    let interval = Duration::from_nanos(batch * 1_000_000_000 / rate);

    'outer: loop {
        let tick_start = clock.get_time();
        if tick_start >= deadline {
            break;
        }

        for _ in 0..batch {
            let Some((id, peer)) = next_sendable(entries, clock, deadline, &mut stats.skipped)
            else {
                break 'outer; // iterator exhausted or deadline passed
            };
            // A slow `next()` above may have crossed the deadline; don't send past it. The entry is
            // left unsent and counted as `remaining`.
            if clock.get_time() >= deadline {
                break 'outer;
            }
            if attempt(id, peer) {
                stats.sent += 1;
            } else {
                stats.failed += 1;
            }
        }

        // Sleep the remainder of the tick, crediting the time already spent sending, capped at the
        // deadline.
        let now = clock.get_time();
        if now >= deadline {
            break;
        }
        let elapsed = if now > tick_start {
            now - tick_start
        } else {
            Duration::ZERO
        };
        let wait = interval.saturating_sub(elapsed).min(deadline - now);
        if !wait.is_zero() {
            clock.sleep(wait);
        }
    }

    stats.remaining = total - stats.sent - stats.failed - stats.skipped;
    stats
}

/// Pulls the next entry that can be sent, discarding v0 (`None`) records along the way (each
/// tallied in `skipped`). Returns `None` once the iterator is exhausted or the deadline passes.
///
/// The deadline is checked *before* each `next()`, so a crossed deadline stops the run before
/// advancing a possibly-slow iterator, and entries beyond it stay unread -- counted as `remaining`,
/// never skipped.
fn next_sendable(
    entries: &mut dyn ExactSizeIterator<Item = DiskEntry>,
    clock: &impl Clock,
    deadline: Timestamp,
    skipped: &mut usize,
) -> Option<(credentials::Id, SocketAddr)> {
    loop {
        if clock.get_time() >= deadline {
            return None;
        }
        match entries.next()? {
            DiskEntry { id: None, .. } => *skipped += 1,
            DiskEntry {
                id: Some(id), peer, ..
            } => return Some((id, peer)),
        }
    }
}

#[cfg(test)]
mod tests;

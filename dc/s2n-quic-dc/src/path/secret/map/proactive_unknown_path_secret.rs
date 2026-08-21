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
use s2n_quic_core::time::{
    timer::Provider as _, token_bucket::TokenBucket, Clock, StdClock, Timestamp,
};
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

/// Lower bound on the token-bucket refill interval.
///
/// Refills release a batch of tokens each interval, so a high rate coalesces many sends into one
/// wakeup rather than sleeping once per packet -- a sleep-per-packet loop is capped by OS sleep
/// granularity (~1ms) to roughly 20-30K packets/sec, below the rate a large (e.g. 500K-peer) map
/// must sustain to drain in tens of seconds. For rates too low to need batching, the interval
/// stretches beyond this floor so the exact rate is still honored.
const MIN_TICK: Duration = Duration::from_millis(5);

/// The pacer's view of time: read the clock, and block until a later instant.
///
/// Injected into [`pace_attempts`] so tests can drive pacing deterministically without real sleeps;
/// production uses [`RealTime`]. Bundling "read the clock" and "wait" into one capability keeps them
/// consistent -- a fake that advances its own clock on `sleep` can't be paired with a real blocking
/// sleep (which would never advance a fake clock, hanging the loop), and vice versa.
trait PacingTime {
    fn now(&self) -> Timestamp;
    fn sleep(&mut self, dur: Duration);
}

/// Production [`PacingTime`]: a real monotonic clock paired with a real blocking sleep.
struct RealTime(StdClock);

impl PacingTime for RealTime {
    fn now(&self) -> Timestamp {
        self.0.get_time()
    }

    fn sleep(&mut self, dur: Duration) {
        std::thread::sleep(dur);
    }
}

/// Builds, signs, and sends one `UnknownPathSecret` packet per entry that has a credential id,
/// paced at approximately `rate` packets per second and stopping once `timeout` of wall-clock time
/// elapses. This is the module's entry point; it paces against a real clock ([`RealTime`]) and
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
pub(super) fn emit_packets<Send>(
    entries: &mut dyn ExactSizeIterator<Item = DiskEntry>,
    rate: NonZeroU32,
    timeout: Duration,
    signer: &stateless_reset::Signer,
    mut send: Send,
) -> SendStats
where
    Send: FnMut(credentials::Id, &[u8], &SocketAddr) -> std::io::Result<()>,
{
    let mut time = RealTime(StdClock::default());
    let deadline = time.now() + timeout;
    pace_attempts(entries, rate, deadline, &mut time, |id, peer| {
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
/// A [`TokenBucket`] meters attempts at `rate`/sec, refilling a batch of tokens on an interval of
/// at least `MIN_TICK` (stretched longer for low rates), so a high rate releases many packets per
/// wakeup instead of sleeping once per packet, while the burst -- including the first -- stays
/// capped at one batch, so a stalled caller can't then dump a catch-up burst onto the shared
/// control socket. `time` is injected so the loop can be driven deterministically in tests; in
/// production it is [`RealTime`]. Because the bucket refills from the current time (re-read after
/// each batch of work), time spent iterating and sending is credited against the interval rather
/// than added on top of it.
fn pace_attempts<T, F>(
    entries: &mut dyn ExactSizeIterator<Item = DiskEntry>,
    rate: NonZeroU32,
    deadline: Timestamp,
    time: &mut T,
    mut attempt: F,
) -> SendStats
where
    T: PacingTime,
    F: FnMut(credentials::Id, SocketAddr) -> bool,
{
    let mut stats = SendStats::default();
    let total = entries.len();

    let rate = u64::from(rate.get());
    let refill_amount = (rate * MIN_TICK.as_millis() as u64).div_ceil(1000).max(1);
    let refill_interval = Duration::from_nanos(refill_amount * 1_000_000_000 / rate);
    let mut bucket = TokenBucket::builder()
        .with_refill_interval(refill_interval)
        .with_refill_amount(refill_amount)
        .with_max(refill_amount)
        .build();

    while let Some((id, peer)) = next_sendable(entries, &*time, deadline, &mut stats.skipped) {
        if !wait_for_token(&mut bucket, time, deadline, refill_interval) {
            break;
        }
        if attempt(id, peer) {
            stats.sent += 1;
        } else {
            stats.failed += 1;
        }
    }

    stats.remaining = total - stats.sent - stats.failed - stats.skipped;
    stats
}

/// Blocks until a pacing token is available, sleeping via `time` between the bucket's refills.
/// Returns `true` once a token is taken, or `false` if `deadline` passes first (the caller then
/// leaves the current entry unattempted). `fallback_interval` is used only if the bucket has no
/// armed refill yet, which cannot happen after the first `take`.
fn wait_for_token<T: PacingTime>(
    bucket: &mut TokenBucket,
    time: &mut T,
    deadline: Timestamp,
    fallback_interval: Duration,
) -> bool {
    loop {
        let now = time.now();
        if now >= deadline {
            return false;
        }
        if bucket.take(1, now) == 1 {
            return true;
        }
        let wait = bucket
            .next_expiration()
            .map(|next| next.saturating_duration_since(now))
            .unwrap_or(fallback_interval)
            .min(deadline.saturating_duration_since(now));
        if !wait.is_zero() {
            time.sleep(wait);
        }
    }
}

/// Pulls the next entry that can be sent, discarding v0 (`None`) records along the way (each
/// tallied in `skipped`). Returns `None` once the iterator is exhausted or the deadline passes.
fn next_sendable(
    entries: &mut dyn ExactSizeIterator<Item = DiskEntry>,
    time: &impl PacingTime,
    deadline: Timestamp,
    skipped: &mut usize,
) -> Option<(credentials::Id, SocketAddr)> {
    loop {
        if time.now() >= deadline {
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

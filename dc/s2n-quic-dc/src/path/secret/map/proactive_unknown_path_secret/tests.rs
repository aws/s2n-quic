// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use std::{
    cell::Cell,
    net::{Ipv4Addr, SocketAddrV4},
    rc::Rc,
};

fn nz(v: u32) -> NonZeroU32 {
    NonZeroU32::new(v).unwrap()
}

fn peer(n: u16) -> SocketAddr {
    SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, n))
}

fn id(n: u8) -> credentials::Id {
    credentials::Id::from([n; 16])
}

fn v1(n: u8) -> DiskEntry {
    DiskEntry {
        peer: peer(n as u16),
        id: Some(id(n)),
    }
}

fn v0(n: u16) -> DiskEntry {
    DiskEntry {
        peer: peer(n),
        id: None,
    }
}

/// A controllable [`PacingClock`] backed by a shared cell. Simulated time advances only when
/// something explicitly moves it -- the pacer's own `sleep`, or a test's iterator/attempt via
/// [`advance`] -- so pacing is exercised with no wall-clock dependency. Clone shares the same clock.
///
/// [`advance`]: FakeClock::advance
#[derive(Clone)]
struct FakeClock {
    at: Rc<Cell<Timestamp>>,
    allow_sleep: bool,
}

impl FakeClock {
    fn new() -> Self {
        // Safety: the duration is non-zero.
        let base = unsafe { Timestamp::from_duration(Duration::from_secs(3600)) };
        Self {
            at: Rc::new(Cell::new(base)),
            allow_sleep: true,
        }
    }

    /// Like [`new`](Self::new), but any sleep fails the test -- for runs that must not pace.
    fn frozen() -> Self {
        Self {
            allow_sleep: false,
            ..Self::new()
        }
    }

    fn now(&self) -> Timestamp {
        self.at.get()
    }

    fn advance(&self, by: Duration) {
        self.at.set(self.at.get() + by);
    }
}

impl Clock for FakeClock {
    fn get_time(&self) -> Timestamp {
        self.at.get()
    }
}

impl PacingClock for FakeClock {
    fn sleep(&self, dur: Duration) {
        assert!(self.allow_sleep, "the pacer must not sleep in this test");
        self.at.set(self.at.get() + dur);
    }
}

/// The four counters always partition the input.
fn assert_partitions(stats: &SendStats, total: usize) {
    assert_eq!(
        stats.sent + stats.failed + stats.skipped + stats.remaining,
        total,
        "counters must partition the input: {stats:?}"
    );
}

#[test]
fn v0_entries_are_skipped_not_attempted() {
    // Interleave v0 (None) and v1 (Some) entries.
    let entries = vec![v0(1), v0(2), v1(3), v0(4), v1(5)];
    let time = FakeClock::frozen();
    let deadline = time.now() + Duration::from_secs(30);

    let mut attempts = 0;
    let stats = pace_attempts(
        &mut entries.into_iter(),
        nz(1_000_000),
        deadline,
        &time,
        |_id, _peer| {
            attempts += 1;
            true
        },
    );

    assert_eq!(attempts, 2, "attempt is never invoked for v0 entries");
    assert_eq!(
        stats,
        SendStats {
            sent: 2,
            failed: 0,
            skipped: 3,
            remaining: 0,
        }
    );
}

#[test]
fn all_v0_file_drains_without_pacing() {
    // A v0-only file must not be throttled: v0 entries consume no pacing budget, so the run
    // completes without ever sleeping even at a slow rate. A frozen clock (whose `sleep` panics)
    // proves it; a few entries suffice (no need to allocate a whole file or measure wall time).
    let entries: Vec<_> = (0..5).map(v0).collect();
    let total = entries.len();
    let time = FakeClock::frozen();
    let deadline = time.now() + Duration::from_secs(30);

    let stats = pace_attempts(
        &mut entries.into_iter(),
        nz(1), // 1/sec -- would pace forever if v0 entries consumed budget
        deadline,
        &time,
        |_id, _peer| unreachable!("no sends for a v0-only file"),
    );

    assert_eq!(
        stats,
        SendStats {
            sent: 0,
            failed: 0,
            skipped: total,
            remaining: 0,
        }
    );
}

#[test]
fn per_entry_failure_is_counted_not_fatal() {
    let entries: Vec<_> = (0..20).map(v1).collect();
    let total = entries.len();
    let time = FakeClock::frozen();
    let deadline = time.now() + Duration::from_secs(30);

    // Fail every third attempt; the run must continue past each failure.
    let mut n: usize = 0;
    let stats = pace_attempts(
        &mut entries.into_iter(),
        nz(1_000_000),
        deadline,
        &time,
        |_id, _peer| {
            let fail = n.is_multiple_of(3); // indices 0,3,6,9,12,15,18 -> 7 failures
            n += 1;
            !fail
        },
    );

    assert_eq!(
        stats,
        SendStats {
            sent: total - 7,
            failed: 7,
            skipped: 0,
            remaining: 0,
        }
    );
}

#[test]
fn deadline_cuts_run_short() {
    // Deterministic: simulated time advances only when the pacer sleeps, so a low rate against
    // a short deadline attempts only a bounded prefix and reports the rest as `remaining`.
    let entries: Vec<_> = (0..500).map(|n| v1((n % 250) as u8)).collect();
    let total = entries.len();
    let time = FakeClock::new();
    let deadline = time.now() + Duration::from_millis(50);

    let stats = pace_attempts(
        &mut entries.into_iter(),
        nz(1_000),
        deadline,
        &time,
        |_id, _peer| true,
    );

    // At 1000/sec the batch is 5 sends per 5ms tick, at t = 0, 5, .. 45ms (10 ticks) before
    // t = 50ms crosses the deadline -- 50 sent, the rest left as remaining.
    assert_eq!(
        stats,
        SendStats {
            sent: 50,
            failed: 0,
            skipped: 0,
            remaining: total - 50,
        }
    );
}

#[test]
fn low_rate_is_not_quantized_up() {
    // A rate below 1 / MIN_TICK (200/sec) must be honored, not rounded up. At 100/sec the tick is
    // one packet per 10ms; 10 land within the 100ms window (a fixed-5ms-tick pacer would send ~20).
    let entries: Vec<_> = (0..1000).map(|n| v1((n % 250) as u8)).collect();
    let total = entries.len();
    let time = FakeClock::new();
    let deadline = time.now() + Duration::from_millis(100);

    let stats = pace_attempts(
        &mut entries.into_iter(),
        nz(100),
        deadline,
        &time,
        |_id, _peer| true,
    );

    assert_eq!(
        stats,
        SendStats {
            sent: 10,
            failed: 0,
            skipped: 0,
            remaining: total - 10,
        }
    );
}

#[test]
fn pacing_sustains_target_rate() {
    // Deterministic pacing: simulated time advances only when the pacer sleeps, so a rate far
    // above the ~20-30K/sec sleep-per-packet ceiling still yields ~rate * window attempts. A
    // broken pacer (unpaced burst, or stuck below the ceiling) lands far outside the band.
    let target = 40_000u32;
    let window = Duration::from_millis(100);
    let expected = (u64::from(target) * window.as_millis() as u64 / 1000) as usize; // 4_000

    // More entries than the window can drain, so the deadline (not the iterator) bounds the run.
    let entries: Vec<_> = (0..(expected * 4)).map(|n| v1((n % 251) as u8)).collect();
    let time = FakeClock::new();
    let deadline = time.now() + window;

    let stats = pace_attempts(
        &mut entries.into_iter(),
        nz(target),
        deadline,
        &time,
        |_id, _peer| true,
    );

    let attempts = stats.sent + stats.failed;
    let (lo, hi) = (expected * 95 / 100, expected * 105 / 100);
    assert!(
        (lo..=hi).contains(&attempts),
        "attempts {attempts} outside [{lo}, {hi}] for {target}/sec over {window:?}"
    );
}

#[test]
fn first_batch_is_immediate() {
    // The first batch must go out before the pacer ever sleeps. With fewer entries than one tick's
    // budget, all of them send in that batch and the frozen clock (whose `sleep` panics) is never
    // touched.
    let rate = nz(100_000); // per-tick budget = 100_000 * MIN_TICK(5ms) = 500
    let entries: Vec<_> = (0..100).map(v1).collect(); // 100 < 500, fits the first batch
    let total = entries.len();
    let time = FakeClock::frozen();
    let deadline = time.now() + Duration::from_secs(5);

    let stats = pace_attempts(
        &mut entries.into_iter(),
        rate,
        deadline,
        &time,
        |_id, _peer| true,
    );

    assert_eq!(
        stats.sent, total,
        "the whole first batch sends before any sleep"
    );
    assert_partitions(&stats, total);
}

#[test]
fn does_not_send_after_deadline_crossed_while_queuing() {
    // A slow `next()` crosses the deadline *between* queuing an entry and sending it: the
    // iterator advances the clock past the deadline as it yields. The pre-send recheck must then
    // stop the run without attempting a send.
    struct AdvancingIter {
        time: FakeClock,
        items: std::vec::IntoIter<DiskEntry>,
        jump: Duration,
    }
    impl Iterator for AdvancingIter {
        type Item = DiskEntry;
        fn next(&mut self) -> Option<DiskEntry> {
            let next = self.items.next();
            if next.is_some() {
                self.time.advance(self.jump);
            }
            next
        }
    }
    impl ExactSizeIterator for AdvancingIter {
        fn len(&self) -> usize {
            self.items.len()
        }
    }

    let time = FakeClock::frozen(); // the first batch has capacity; the deadline stops us
    let mut entries = AdvancingIter {
        time: time.clone(),
        items: vec![v1(1), v1(2)].into_iter(),
        jump: Duration::from_secs(1),
    };
    let total = entries.len();
    let deadline = time.now() + Duration::from_millis(10);

    let mut attempts = 0;
    let stats = pace_attempts(&mut entries, nz(100_000), deadline, &time, |_id, _peer| {
        attempts += 1;
        true
    });

    assert_eq!(
        attempts, 0,
        "no send once the deadline is crossed before it"
    );
    assert_eq!(stats.sent, 0);
    assert_eq!(stats.remaining, total);
    assert_partitions(&stats, total);
}

#[test]
fn v0_after_deadline_counts_remaining_not_skipped() {
    // The send crosses the deadline (the attempt advances the clock past it); the *following*
    // v0 entry must then be counted as `remaining` -- the loop must not advance the iterator to
    // fetch and skip it after the deadline.
    let time = FakeClock::frozen();
    let attempt = {
        let time = time.clone();
        move |_id, _peer| {
            time.advance(Duration::from_secs(1));
            true
        }
    };
    let deadline = time.now() + Duration::from_millis(10);

    let stats = pace_attempts(
        &mut vec![v1(1), v0(2)].into_iter(),
        nz(100_000),
        deadline,
        &time,
        attempt,
    );

    assert_eq!(
        stats,
        SendStats {
            sent: 1,
            failed: 0,
            skipped: 0, // the v0 past the deadline is `remaining`, not fetched-and-skipped
            remaining: 1,
        }
    );
}

#[test]
fn builds_authentic_packets_and_skips_v0() {
    use crate::packet::secret_control as control;
    use s2n_codec::DecoderBufferMut;

    let secret = b"emission-secret";
    let signer = stateless_reset::Signer::new(secret);

    // Two v1 entries plus one v0 that must be skipped without a packet being built. A generous
    // timeout means everything fits the first batch, so this doesn't sleep despite using real time.
    let entries = vec![v1(1), v0(2), v1(3)];

    let mut captured: Vec<(credentials::Id, Vec<u8>, SocketAddr)> = Vec::new();
    let stats = emit_packets(
        &mut entries.into_iter(),
        nz(1_000_000),
        Duration::from_secs(30),
        &signer,
        |id, bytes, peer| {
            captured.push((id, bytes.to_vec(), *peer));
            Ok(())
        },
    );

    assert_eq!(
        stats,
        SendStats {
            sent: 2,
            failed: 0,
            skipped: 1,
            remaining: 0,
        }
    );
    // The two v1 entries are sent, in order, to their peers -- the v0 in between is skipped.
    let peers: Vec<_> = captured.iter().map(|(_, _, peer)| *peer).collect();
    assert_eq!(peers, vec![peer(1), peer(3)]);

    let verifier = stateless_reset::Signer::new(secret);
    let wrong = stateless_reset::Signer::new(b"a different signer's secret");

    for (id, mut bytes, _peer) in captured {
        let (packet, _) =
            control::unknown_path_secret::Packet::decode(DecoderBufferMut::new(&mut bytes))
                .unwrap();
        // Authenticates against a signer sharing the secret...
        let authentic = packet
            .authenticate(&verifier.sign(&id))
            .expect("packet must verify against the signer");
        assert_eq!(authentic.credential_id, id);
        // ...and is rejected by a signer that does not.
        assert!(
            packet.authenticate(&wrong.sign(&id)).is_none(),
            "packet must not verify against a different signer"
        );
    }
}

#[test]
fn counts_send_failures() {
    let signer = stateless_reset::Signer::new(b"secret");

    // The send fails, as `WouldBlock` would on a saturated non-blocking socket; the run must
    // count the failure rather than abort.
    let stats = emit_packets(
        &mut vec![v1(1)].into_iter(),
        nz(1_000_000),
        Duration::from_secs(30),
        &signer,
        |_id, _bytes, _peer| Err(std::io::Error::from(std::io::ErrorKind::WouldBlock)),
    );

    assert_eq!(
        stats,
        SendStats {
            sent: 0,
            failed: 1,
            skipped: 0,
            remaining: 0,
        }
    );
}

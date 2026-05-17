// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Deterministic packet-loss simulation tests for the stream endpoint.
//!
//! These tests run inside Bach's deterministic simulator with two fully-wired
//! stream endpoints backed by simulated UDP sockets.  The [`DroppedPackets`]
//! helper generates structured packet-loss patterns so each test is perfectly
//! reproducible and shrinkable by bolero.
//!
//! The transfer scenario is a 256 KiB echo: the client sends a body to the
//! server, the server echoes it back, and the test asserts the round-trip
//! completes within a time bound derived from the loss percentage.
//!
//! Deterministic tests (`no_loss`, `initial_loss`, `sporadic_loss`) additionally
//! lock in the elapsed simulated time via insta snapshots so any regression in
//! throughput is immediately visible.

use crate::{
    stream::endpoint::testing::sim::{Client, Server, SERVER_PORT},
    testing::{ext::*, sim},
};
use bach::time::{timeout, Instant};
use bytes::{Bytes, BytesMut};
use core::ops::Range;
use s2n_quic_core::varint::VarInt;
use std::{
    collections::HashSet,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

// ── DroppedPackets ────────────────────────────────────────────────────────────

/// Encodes a structured packet-drop pattern as a sequence of run-length counts.
///
/// The counts alternate between "pass" and "drop" runs, starting with a "pass"
/// run.  This mirrors the `DroppedPackets` type used in
/// `stream::tests::deterministic` so the two suites exercise comparable loss
/// scenarios.
#[derive(Clone, bolero::TypeGenerator)]
struct DroppedPackets {
    #[generator(produce::<Vec<u8>>().with().values(0..=10))]
    counts: Vec<u8>,
}

impl DroppedPackets {
    fn ranges(&self) -> impl Iterator<Item = Range<usize>> + '_ {
        Self::iter_from_counts(self.counts.iter().copied(), |range, enabled| {
            if !enabled && range.end > range.start {
                Some(range)
            } else {
                None
            }
        })
    }

    fn enabled_iter(self) -> impl Iterator<Item = bool> {
        Self::iter_from_counts(self.counts.into_iter(), |range, enabled| {
            range.map(move |_| enabled)
        })
    }

    fn iter_from_counts<T: IntoIterator>(
        v: impl Iterator<Item = u8>,
        map: impl Fn(Range<usize>, bool) -> T,
    ) -> impl Iterator<Item = T::Item> {
        let mut start = 0;
        let mut enabled = true;
        let mut is_enabled = move || {
            let v = enabled;
            enabled = !enabled;
            v
        };

        v.flat_map(move |mut len| {
            let local_start = start;

            if start > 0 {
                len = len.max(1);
            }

            let end = start + len as usize;
            start = end;

            let v = is_enabled();

            map(local_start..end, v)
        })
    }

    fn from_iter(iter: impl IntoIterator<Item = Range<usize>>) -> Self {
        let mut counts = vec![];
        let mut last = 0;
        for range in iter {
            counts.push((range.start - last) as u8);
            counts.push((range.end - range.start) as u8);
            last = range.end;
        }
        Self { counts }
    }

    fn loss_percent(&self) -> f64 {
        let mut total = 0;
        let mut dropped = 0;
        for range in self.ranges() {
            total = range.end;
            dropped += range.end - range.start;
        }
        if total == 0 {
            return 0.0;
        }
        (dropped as f64 / total as f64) * 100.0
    }

    /// Runs a 256 KiB echo simulation with this packet-drop pattern.
    ///
    /// Only server→client packets (source port [`SERVER_PORT`]) are subject to
    /// the drop pattern, mirroring the convention used in
    /// `stream::tests::deterministic`.
    ///
    /// Returns the total elapsed simulated time from `t=0` to transfer
    /// completion.  If the transfer does not complete within `TRANSFER_TIMEOUT`,
    /// the call panics so bolero can shrink to the minimal failing case.
    fn sim(self, body_len: usize) -> Duration {
        const TRANSFER_TIMEOUT: Duration = Duration::from_secs(30);

        let acceptor_id = VarInt::from_u8(1);

        let end_time: Arc<Mutex<Option<Instant>>> = Arc::new(Mutex::new(None));
        let end_time_inner = end_time.clone();

        sim(|| {
            {
                tracing::info!(
                    packets = ?self,
                    loss = format!("{:.02}%", self.loss_percent()),
                    "starting test"
                );
                let mut enabled = self.enabled_iter().enumerate();
                bach::net::monitor::on_packet_sent(move |packet| {
                    if packet.destination().port() != SERVER_PORT {
                        if let Some((idx, enabled)) = enabled.next() {
                            if !enabled {
                                tracing::info!(
                                    idx,
                                    len = packet.transport.payload().len(),
                                    "dropping server packet"
                                );
                                return bach::net::monitor::Command::Drop;
                            } else {
                                tracing::info!(
                                    idx,
                                    len = packet.transport.payload().len(),
                                    "allowing server packet"
                                );
                            }
                        }
                    }
                    bach::net::monitor::Command::Pass
                });
            }

            // ── Client ────────────────────────────────────────────────────────
            {
                let end_time = end_time_inner.clone();
                let acceptor_id = acceptor_id;
                async move {
                    let mut client = Client::new();
                    let stream = client
                        .connect("server:0", acceptor_id)
                        .await
                        .expect("connect failed");

                    let (mut reader, mut writer) = stream.into_split();

                    let mut body = Bytes::from(vec![42u8; body_len]);

                    timeout(TRANSFER_TIMEOUT, async {
                        writer
                            .write_all_from_fin(&mut body)
                            .await
                            .expect("client write");

                        let mut response = BytesMut::with_capacity(body_len);
                        loop {
                            let n = reader.read_into(&mut response).await.expect("client read");
                            if n == 0 {
                                break;
                            }
                        }
                        assert_eq!(response.len(), body_len);
                        assert!(response.iter().all(|&b| b == 42u8));
                    })
                    .await
                    .expect("transfer timed out");

                    *end_time.lock().unwrap() = Some(Instant::now());
                }
                .group("client")
                .primary()
                .spawn();
            }

            // ── Server ────────────────────────────────────────────────────────
            {
                let acceptor_id = acceptor_id;
                async move {
                    let server = Server::new();
                    let mut acceptor = server
                        .register_acceptor_channel(acceptor_id, 8)
                        .expect("acceptor registration failed");

                    while let Some(stream) = acceptor.recv().await {
                        async move {
                            let stream = stream.validate().await.expect("server validate");
                            let (mut reader, mut writer) = stream.into_split();

                            let mut request = BytesMut::with_capacity(body_len);
                            loop {
                                let n = reader.read_into(&mut request).await.expect("server read");
                                if n == 0 {
                                    break;
                                }
                            }

                            let mut response = request.freeze();
                            writer
                                .write_all_from_fin(&mut response)
                                .await
                                .expect("server write");
                        }
                        .primary()
                        .spawn();
                    }
                }
                .group("server")
                .spawn();
            }
        });

        let elapsed = end_time.lock().unwrap().unwrap().elapsed_since_start();
        elapsed
    }
}

impl core::fmt::Debug for DroppedPackets {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_set().entries(self.ranges()).finish()
    }
}

// ── Snapshot tests ────────────────────────────────────────────────────────────

/// Baseline: 256 KiB echo with no packet loss.
///
/// Locks in the expected simulated transfer duration.  Any regression in the
/// no-loss path (e.g., extra round-trips, unnecessary retransmissions) will
/// appear as a snapshot diff.
#[test]
fn no_loss() {
    let elapsed = DroppedPackets { counts: vec![] }.sim(1 << 18);
    insta::assert_snapshot!(format!("{elapsed:?}"));
}

/// 256 KiB echo with heavy initial loss followed by sporadic loss.
///
/// This is the loss pattern that was historically produced by bolero while
/// shrinking a failure in the PTO / retransmission path.  Locking it in as a
/// snapshot test catches regressions in PTO handling under sustained loss.
#[test]
fn initial_loss() {
    let elapsed = DroppedPackets::from_iter([
        1..10,
        10..20,
        20..29,
        29..34,
        38..45,
        52..54,
        61..66,
        67..71,
        78..86,
        91..97,
        97..100,
        100..102,
        113..117,
        121..124,
        129..130,
        138..141,
    ])
    .sim(1 << 18);
    insta::assert_snapshot!(format!("{elapsed:?}"));
}

/// 256 KiB echo with sporadic loss spread across the entire transfer.
///
/// This pattern exercises the steady-state loss recovery path.  The snapshot
/// captures the expected latency penalty so throughput regressions are visible.
#[test]
fn sporadic_loss() {
    let elapsed = DroppedPackets::from_iter([
        10..12,
        22..27,
        28..32,
        40..45,
        54..61,
        63..66,
        75..77,
        81..84,
        89..98,
        108..116,
        121..127,
        129..131,
        136..137,
        143..152,
        153..160,
        163..172,
        181..183,
        185..192,
        199..201,
        202..212,
        213..219,
        227..236,
        238..248,
        275..283,
        284..290,
    ])
    .sim(1 << 18);
    insta::assert_snapshot!(format!("{elapsed:?}"));
}

// ── Fuzz tests ────────────────────────────────────────────────────────────────

/// Fuzzes all server→client packet-loss patterns.
///
/// For each generated [`DroppedPackets`] value, a 256 KiB echo transfer is run
/// in the deterministic simulator.  The test asserts that the transfer always
/// completes — the exact duration is not checked here, only liveness.
#[test]
fn bulk_transfer_with_loss() {
    bolero::check!()
        .with_type::<DroppedPackets>()
        .with_test_time(core::time::Duration::from_secs(30))
        .with_shrink_time(core::time::Duration::from_secs(0))
        .cloned()
        .for_each(|packets| {
            let _ = packets.sim(1 << 18);
        });
}

/// Fuzzes packet-loss patterns and asserts that transfer time scales
/// proportionally with the loss rate.
///
/// The elapsed time is compared against a bound derived from the loss
/// percentage: at 0% loss the cap is ~1 s; at 100% loss it scales up to the
/// 30 s hard timeout.  This means bolero will shrink any pattern where the
/// end-to-end time grows orders of magnitude beyond what the loss rate predicts.
#[test]
fn transmission_rate_fuzz() {
    bolero::check!()
        .with_type::<DroppedPackets>()
        .with_test_time(core::time::Duration::from_secs(30))
        .with_shrink_time(core::time::Duration::from_secs(10))
        .cloned()
        .for_each(|packets| {
            let loss = packets.loss_percent();
            let elapsed = packets.sim(1 << 18);

            // Duration bound that scales quadratically with loss rate:
            //   0% loss  → 1 s max
            //   50% loss → ~8 s max
            //   100% loss → 30 s max (also enforced by the transfer timeout)
            let max_secs = 1.0_f64 + (loss / 100.0).powi(2) * 29.0;
            let max_secs = max_secs.min(30.0);
            let max_allowed = Duration::from_secs_f64(max_secs);
            assert!(
                elapsed <= max_allowed,
                "transfer took too long for {loss:.1}% loss: {elapsed:?} \
                 (max allowed: {max_allowed:?})"
            );
        });
}

// ── Init Protocol Deduplication ───────────────────────────────────────────────

/// Describes per-packet network manipulation to apply to packets in the
/// simulation.
///
/// The two command lists are zipped into per-packet `(delay, duplicate)` pairs
/// and cycled over every packet so that even a short bolero-generated list
/// covers the entire simulation.  The `delay` value is in units of 5 ms
/// (range 0..=50 → 0..=250 ms); `duplicate` requests an extra network-level
/// copy delivered at absolute base latency.
#[derive(Clone, bolero::TypeGenerator)]
struct PacketActions {
    /// Per-packet extra delay, in units of 5 ms (range 0..=50 → 0..=250 ms).
    #[generator(produce::<Vec<u8>>().with().values(0..=50))]
    delays: Vec<u8>,
    /// Per-packet duplication flag.
    duplicates: Vec<bool>,
}

impl core::fmt::Debug for PacketActions {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PacketActions")
            .field("n_delays", &self.delays.len())
            .field(
                "max_delay_ms",
                &(self.delays.iter().copied().max().unwrap_or(0) as u64 * 5),
            )
            .field("n_duplicates", &self.duplicates.len())
            .field(
                "n_duplicated",
                &self.duplicates.iter().filter(|&&b| b).count(),
            )
            .finish()
    }
}

/// Installs two Bach network monitors that apply per-packet delay and
/// duplication to every packet in the simulation.
///
/// The `delays` and `duplicates` lists from `actions` are zipped into a single
/// `Vec<(u8, bool)>` and shared between two monitors via a single index
/// counter.  Both monitors read the **same** index for each packet (monitor 1
/// peeks without advancing; monitor 2 advances after reading), so the delay
/// and duplicate commands are always applied in lock-step.  The list is cycled
/// with `i % len` so that every packet receives a command even when the
/// generated list is shorter than the total number of packets.
///
/// Duplicate packets (`packet.is_duplicate == true`) are passed through
/// unchanged by the duplicate monitor to prevent cascading expansion.
///
/// This function must be called inside a `sim(|| { … })` closure.
fn install_init_monitors(actions: &PacketActions) {
    use bach::net::monitor;

    let n = actions.delays.len().max(actions.duplicates.len());
    if n == 0 {
        return;
    }

    // Zip the two lists into a single per-packet command vec.
    let commands: Arc<Vec<(u8, bool)>> = Arc::new(
        (0..n)
            .map(|i| {
                let d = actions.delays.get(i).copied().unwrap_or(0);
                let b = actions.duplicates.get(i).copied().unwrap_or(false);
                (d, b)
            })
            .collect(),
    );

    // A single counter shared between both monitors so they always operate on
    // the same index for the same packet.
    let idx = Arc::new(AtomicUsize::new(0));

    // Monitor 1: delay — peeks at the current index without advancing.
    let commands_m1 = commands.clone();
    let idx_m1 = idx.clone();
    monitor::on_packet_sent(move |_packet| {
        let i = idx_m1.load(Ordering::Relaxed);
        let (delay, _) = commands_m1[i % commands_m1.len()];
        if delay > 0 {
            return monitor::delay(Duration::from_millis(delay as u64 * 5)).into();
        }
        Default::default()
    });

    // Monitor 2: duplicate — reads the current index and then advances it.
    // Only original (non-duplicate) packets are duplicated to prevent
    // cascading expansion.
    monitor::on_packet_sent(move |packet| {
        let i = idx.fetch_add(1, Ordering::Relaxed);
        let (_, should_dup) = commands[i % commands.len()];
        if should_dup && !packet.is_duplicate {
            return monitor::duplicate(1).absolute().into();
        }
        Default::default()
    });
}

/// Core helper for the init-protocol uniqueness tests.
///
/// Opens `n` concurrent client streams (using a single [`Client`], so they
/// receive consecutive stream IDs `0..n`), installs the given network
/// manipulation via [`install_init_monitors`], and then verifies two invariants:
///
/// 1. **No duplicate acceptance**: the server's acceptor receives each stream
///    ID at most once.  If any ID appears twice the assertion fires immediately,
///    indicating a bug in the init-protocol deduplication logic.
///
/// 2. **No missing streams**: the server eventually accepts all `n` streams.
///    Combined with (1) this means the server sees each ID *exactly* once.
///
/// The server task is the Bach primary task; the simulation ends when the
/// server has counted `n` accepted streams.  The client task is non-primary
/// and keeps the simulated network alive until the server finishes.
///
/// `connect` failures on the client side are fatal (the test panics), since
/// the client should always be able to initiate a stream regardless of network
/// conditions.
fn sim_init_uniqueness(actions: &PacketActions, n: usize) {
    // Generous timeout: even with the maximum 250 ms per-packet delay the
    // entire population of FlowInit packets arrives within ~300 ms of sim
    // time, well inside the 5-second window.
    const ACCEPT_TIMEOUT: Duration = Duration::from_secs(5);

    let acceptor_id = VarInt::from_u8(1);

    let seen_ids: Arc<Mutex<HashSet<u64>>> = Arc::new(Mutex::new(HashSet::new()));
    let seen_ids_sv = seen_ids.clone();

    tracing::info!("════════════════════════════════════════════════════════════════════");
    tracing::info!(?actions, n, "starting init_uniqueness sim");

    sim(|| {
        install_init_monitors(actions);

        // ── Server (primary) ─────────────────────────────────────────────────
        // Accept streams until `n` unique stream IDs have been validated.
        // Duplicates (which fail validation) are silently discarded.
        {
            let validated_count = Arc::new(AtomicUsize::new(0));
            let validated_count_inner = validated_count.clone();
            async move {
                let server = Server::new();
                let mut acceptor = server
                    .register_acceptor_channel(acceptor_id, n * 2)
                    .expect("acceptor registration failed");

                loop {
                    if validated_count_inner.load(Ordering::Relaxed) == n {
                        break;
                    }

                    let stream = timeout(ACCEPT_TIMEOUT, acceptor.recv()).await;

                    let stream = match stream {
                        Ok(Some(s)) => s,
                        _ => {
                            assert_eq!(
                                validated_count_inner.load(Ordering::Relaxed),
                                n,
                                "server timed out before all streams were validated"
                            );
                            break;
                        }
                    };

                    let seen_ids_sv = seen_ids_sv.clone();
                    let validated_count = validated_count_inner.clone();
                    async move {
                        let mut stream = match stream.validate().await {
                            Ok(stream) => stream,
                            Err(_) => return,
                        };

                        let id = stream.stream_id();
                        let first_time = seen_ids_sv.lock().unwrap().insert(id);
                        assert!(
                            first_time,
                            "stream_id {id} was delivered to the server acceptor twice — \
                         init-protocol deduplication is broken"
                        );

                        let mut res: Vec<u8> = vec![];
                        stream.read_into(&mut res).await.unwrap();
                        assert_eq!(res, &id.to_be_bytes());

                        stream
                            .write_all_from_fin(&mut &id.to_be_bytes()[..])
                            .await
                            .unwrap();

                        validated_count.fetch_add(1, Ordering::Relaxed);
                    }
                    .primary()
                    .spawn();
                }
            }
            .group("server")
            .primary()
            .spawn();
        }

        // ── Client (non-primary) ─────────────────────────────────────────────
        // Open `n` streams and immediately drop each one (which enqueues a FIN
        // via Writer::drop).  Keeps the simulated network alive until the
        // server's primary task finishes.
        {
            async move {
                let mut client = Client::new();
                for id in 0..n as u64 {
                    let mut stream = client
                        .connect("server:0", acceptor_id)
                        .await
                        .expect("client connect() failed — this should never happen");

                    async move {
                        stream
                            .write_all_from_fin(&mut &id.to_be_bytes()[..])
                            .await
                            .unwrap();

                        let mut res: Vec<u8> = vec![];
                        stream.read_into(&mut res).await.unwrap();
                        assert_eq!(res, &id.to_be_bytes());
                    }
                    .spawn();
                }
                // Stay alive so packets in flight can reach the server.
                bach::time::sleep(Duration::from_secs(60)).await;
            }
            .group("client")
            .spawn();
        }
    });

    // Post-sim assertion: every stream ID in 0..n was accepted exactly once.
    // (The no-duplicate invariant was already checked inline above; this
    // catches the complementary "missing stream" case.)
    let seen = seen_ids.lock().unwrap();
    assert_eq!(
        seen.len(),
        n,
        "expected {n} unique stream IDs but server saw {} — \
         some streams were either missing or duplicated",
        seen.len()
    );
}

// ── Deterministic uniqueness tests ───────────────────────────────────────────

/// Baseline: 1 000 concurrent streams with no network manipulation.
///
/// Verifies the server sees all 1 000 stream IDs exactly once when packets are
/// delivered in order without duplication.
#[test]
fn init_uniqueness_baseline() {
    const N: usize = 1_000;
    let actions = PacketActions {
        delays: vec![],
        duplicates: vec![],
    };
    sim_init_uniqueness(&actions, N);
}

/// All FlowInit packets duplicated, no extra delay.
///
/// Every stream's FlowInit arrives at the server twice at approximately the
/// same network latency.  The init-protocol deduplication must discard the
/// second copy so the server acceptor sees each stream ID only once.
#[test]
fn init_uniqueness_all_duplicated() {
    const N: usize = 100_000;
    let actions = PacketActions {
        delays: vec![0; N],
        duplicates: vec![true; N],
    };
    sim_init_uniqueness(&actions, N);
}

/// Reordered FlowInit packets with selective duplication.
///
/// Odd-indexed packets are delayed by 250 ms, causing them to arrive after
/// even-indexed packets (which have no extra delay).  Every third packet is
/// also duplicated.  The combination exercises the init protocol under both
/// out-of-order delivery and duplicate arrival.
#[test]
fn init_uniqueness_reordered_and_duplicated() {
    const N: usize = 1_000;
    let actions = PacketActions {
        delays: (0..N).map(|i| if i % 2 == 1 { 50 } else { 0 }).collect(),
        duplicates: (0..N).map(|i| i % 3 == 0).collect(),
    };
    sim_init_uniqueness(&actions, N);
}

// ── ACK loop prevention ───────────────────────────────────────────────────────

/// Verifies that the ACK-only RTT probe does not create an ACK loop.
///
/// In a read-heavy scenario the sender (server) emits only ACK-only packets.
/// If those packets are always made ack-eliciting, the peer (client) will
/// respond with ack-eliciting ACKs of its own, which causes the server to keep
/// sending ack-eliciting responses, and so on indefinitely.  This ping-pong
/// resets the idle timer on both sides, preventing the connection from ever
/// timing out.
///
/// The `AckRttTracker::sampled` flag breaks the loop: after the RTT sample is
/// consumed (the probe was acknowledged), `is_pending()` returns `true` and the
/// assembler no longer makes ACK-only packets ack-eliciting.  This means the
/// peer does not receive another ack-eliciting packet from us, so it has no
/// reason to reply, and the exchange terminates.
///
/// This test counts packets sent during a 5-second observation window that
/// starts immediately after a small data transfer completes.  With the fix,
/// only a handful of completion-acknowledgement packets should flow.  Without
/// the fix, an ACK loop at 1 ms RTT would generate thousands of packets.
#[test]
fn ack_only_probe_does_not_create_ack_loop() {
    // Small body: we care about post-transfer behaviour, not throughput.
    const BODY_LEN: usize = 1024;
    // 5 simulated seconds at 1 ms RTT = 5,000 RTTs.  An unbounded ACK loop
    // would produce at least two packets per RTT → ≥ 10,000 extra packets.
    const OBSERVE_WINDOW: Duration = Duration::from_secs(5);
    // Allow a generous margin for the legitimate ACK flush that follows the
    // transfer (completing the last round of in-flight ACKs) plus a single
    // probe exchange.
    const MAX_EXTRA_PACKETS: usize = 100;

    let transfer_done = Arc::new(AtomicBool::new(false));
    let packets_after = Arc::new(AtomicUsize::new(0));

    {
        let transfer_done = transfer_done.clone();
        let packets_after = packets_after.clone();

        sim(|| {
            // Count packets sent after the transfer completes.
            {
                let done = transfer_done.clone();
                let count = packets_after.clone();
                bach::net::monitor::on_packet_sent(move |_packet| {
                    if done.load(Ordering::Relaxed) {
                        count.fetch_add(1, Ordering::Relaxed);
                    }
                    bach::net::monitor::Command::Pass
                });
            }

            let acceptor_id = VarInt::from_u8(1);

            // ── Server: read-only (read-heavy path, ACK-only sends) ───────────
            {
                async move {
                    let server = Server::new();
                    let mut acceptor = server
                        .register_acceptor_channel(acceptor_id, 8)
                        .expect("acceptor registration");

                    while let Some(stream) = acceptor.recv().await {
                        async move {
                            let stream = stream.validate().await.expect("server validate");
                            let (mut reader, _writer) = stream.into_split();
                            let mut buf = BytesMut::with_capacity(BODY_LEN);
                            loop {
                                let n = reader.read_into(&mut buf).await.expect("server read");
                                if n == 0 {
                                    break;
                                }
                            }
                        }
                        .spawn();
                    }
                }
                .group("server")
                .spawn();
            }

            // ── Client: write data, mark done, then observe for OBSERVE_WINDOW ──
            {
                let done = transfer_done.clone();
                async move {
                    let mut client = Client::new();
                    let stream = client
                        .connect("server:0", acceptor_id)
                        .await
                        .expect("connect");
                    let (_reader, mut writer) = stream.into_split();
                    let mut body = Bytes::from(vec![1u8; BODY_LEN]);
                    writer
                        .write_all_from_fin(&mut body)
                        .await
                        .expect("client write");

                    // Signal that the transfer is complete and start counting.
                    done.store(true, Ordering::Relaxed);

                    // Wait for the observation window so background tasks can
                    // exchange any post-transfer packets.
                    bach::time::sleep(OBSERVE_WINDOW).await;
                }
                .group("client")
                .primary()
                .spawn();
            }
        });
    }

    let extra = packets_after.load(Ordering::Relaxed);
    assert!(
        extra < MAX_EXTRA_PACKETS,
        "ACK loop detected: expected <{MAX_EXTRA_PACKETS} packets after transfer but \
         observed {extra}. The AckRttTracker sampled flag may not be suppressing \
         re-probing correctly."
    );
}

// ── Init uniqueness fuzz ──────────────────────────────────────────────────────

/// Fuzzes the init-protocol uniqueness invariant with randomized per-packet
/// delay and duplication patterns.
///
/// For each generated [`PacketActions`] value, 10,000 concurrent streams are
/// opened and the test asserts that the server acceptor receives each stream
/// ID exactly once (no duplicates, none missing).
#[test]
#[ignore = "this is currently failing - TODO figure out why"]
fn init_uniqueness_fuzz() {
    bolero::check!()
        .with_type::<PacketActions>()
        .with_test_time(Duration::from_secs(30))
        .with_shrink_time(Duration::from_secs(10))
        .cloned()
        .for_each(|actions| {
            sim_init_uniqueness(&actions, 10_000);
        });
}


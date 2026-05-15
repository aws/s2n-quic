// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Deterministic packet-loss simulation tests for the stream3 endpoint.
//!
//! These tests run inside Bach's deterministic simulator with two fully-wired
//! stream3 endpoints backed by simulated UDP sockets.  The [`DroppedPackets`]
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
    stream3::endpoint::testing::sim::{Client, Server, SERVER_PORT},
    testing::{ext::*, sim, spawn},
};
use bach::time::Instant;
use bytes::{Bytes, BytesMut};
use core::ops::Range;
use s2n_quic_core::varint::VarInt;
use std::{
    sync::{Arc, Mutex},
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

                    crate::testing::timeout(TRANSFER_TIMEOUT, async {
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
                    let acceptor = server
                        .register_acceptor_channel(acceptor_id, 8)
                        .expect("acceptor registration failed");

                    while let Ok(stream) = acceptor.recv_front().await {
                        spawn(async move {
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
                        });
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

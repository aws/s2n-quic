// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! End-to-end recv-credit fair-share contention tests.
//!
//! These reproduce the dc-tester `xlarge-request-100` scenario inside Bach's deterministic
//! simulator: many concurrent client→server bulk uploads share a single, deliberately
//! undersized server-side recv credit pool. Each server reader must repeatedly acquire pool
//! credit to advertise a receive window (MAX_DATA); when the pool is exhausted the reader parks
//! and the credit [`Distributor`](crate::credit::Distributor) must hand it a fair slice so it can
//! make forward progress. If the distribution starves any reader, its peer writer's flow-control
//! budget stays at zero, the writer hangs, and that stream never completes — which the chunk-level
//! liveness watchdog below surfaces as a stuck future.
//!
//! ## Liveness, not a transfer deadline
//!
//! The watchdog mirrors dc-tester's `send_recv` exactly: every individual `write_from_fin` /
//! `read_into` call must produce *a* chunk within [`CHUNK_TIMEOUT`]. This is a *liveness* check on
//! the future, not a fixed deadline on the whole transfer — a slow-but-progressing stream is fine;
//! a stream whose future stops being woken is not. On a timeout we retry once with a 1 ms budget:
//! if data is suddenly available, the original poll missed a wakeup (a real bug, panicked
//! distinctly); if not, the future is genuinely wedged (the straggler we're hunting).
//!
//! ## Fidelity
//!
//! Running this end-to-end (rather than a synthetic pool unit test) keeps the real reader
//! window-growth policy, the real dispatch-side credit release as bytes arrive, the real writer
//! blocked-signal feedback, and the real distributor task all in the loop, exactly as in dc-tester.
//! The unit tests in `credit::pool::tests` pin the distributor's accounting; this pins the *system*
//! behaviour the accounting is supposed to produce.

use crate::{
    credit::Config as CreditConfig,
    stream::endpoint::testing::sim::{Client, Server, SimEndpointConfig},
    testing::{ext::*, sim},
    tracing::*,
};
use bach::time::timeout;
use s2n_quic_core::{buffer::reader::Reader as _, stream::testing::Data, varint::VarInt};
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

/// Per-chunk liveness budget. A single `write_from_fin` / `read_into` that makes no progress
/// within this much *simulated* time is treated as a stall. Matches dc-tester's 10 s.
const CHUNK_TIMEOUT: Duration = Duration::from_secs(10);

/// Drive a writer to completion one chunk at a time, asserting liveness on every chunk.
///
/// Mirrors dc-tester's bulk send loop: each `write_from_fin` must produce a chunk within
/// [`CHUNK_TIMEOUT`]; on timeout, a 1 ms retry distinguishes a missed waker (data was actually
/// ready — a bug) from a genuine stall. Panics on either, naming the offset so a straggler is
/// immediately identifiable.
async fn drive_writer(writer: &mut crate::stream::Writer, body_len: u64, stream_idx: usize) {
    let mut payload = Data::new(body_len);
    loop {
        if payload.is_finished() {
            break;
        }
        let before = payload.current_offset().as_u64();
        match timeout(CHUNK_TIMEOUT, writer.write_from_fin(&mut payload)).await {
            Ok(res) => {
                res.expect("writer chunk failed");
            }
            Err(_) => {
                // Liveness probe: was a wakeup missed, or is the writer genuinely stuck?
                match timeout(
                    Duration::from_millis(1),
                    writer.write_from_fin(&mut payload),
                )
                .await
                {
                    Ok(Ok(n)) if n > 0 => {
                        panic!(
                            "BUG: missed waker on writer! wrote {n} bytes on immediate retry \
                             after {CHUNK_TIMEOUT:?}. stream={stream_idx} offset={before}/{body_len}"
                        );
                    }
                    _ => {
                        panic!(
                            "writer stalled: no chunk produced within {CHUNK_TIMEOUT:?} and none \
                             on retry. stream={stream_idx} offset={before}/{body_len} \
                             (peer reader never advertised enough window — recv-credit starvation)"
                        );
                    }
                }
            }
        }
    }
}

/// Drain a reader to EOF one chunk at a time, asserting liveness on every chunk. Mirrors the
/// writer-side watchdog and dc-tester's `recv` loop.
async fn drain_reader(reader: &mut crate::stream::Reader, body_len: u64, stream_idx: usize) {
    let mut rx = Data::new(body_len);
    loop {
        let before = rx.current_offset().as_u64();
        let n = match timeout(CHUNK_TIMEOUT, reader.read_into(&mut rx)).await {
            Ok(res) => res.expect("reader chunk failed"),
            Err(_) => match timeout(Duration::from_millis(1), reader.read_into(&mut rx)).await {
                Ok(Ok(n)) if n > 0 => {
                    panic!(
                            "BUG: missed waker on reader! read {n} bytes on immediate retry \
                             after {CHUNK_TIMEOUT:?}. stream={stream_idx} offset={before}/{body_len}"
                        );
                }
                _ => {
                    panic!(
                        "reader stalled: no chunk produced within {CHUNK_TIMEOUT:?} and none \
                             on retry. stream={stream_idx} offset={before}/{body_len} \
                             (reader could not acquire recv credit to advertise window)"
                    );
                }
            },
        };
        if n == 0 {
            break;
        }
    }
    assert!(rx.is_finished(), "reader EOF before full body received");
}

/// Run `num_streams` concurrent client→server bulk uploads of `body_len` bytes each, with the
/// server's recv credit pool sized to `(recv_cap, max_single_acquire)`.
///
/// Every per-stream task — both the client writer and the server reader — is `.primary()`, so the
/// simulation stays open until each one finishes. A stuck stream therefore cannot be masked by the
/// run ending early; instead its chunk-level watchdog fires and panics. The test passes only if
/// all `num_streams` writers AND all `num_streams` readers complete.
///
/// The client uses a large default send pool so this isolates *recv*-side contention — the writer
/// is gated only by the receive window its peer reader advertises, never by its own local send
/// credit. The server only reads (no echo) so the bulk data flows one direction and the server
/// reader is the sole consumer of the contended recv pool.
fn run_fair_share(
    num_streams: usize,
    body_len: u64,
    recv_cap: u64,
    max_single_acquire: u64,
) -> (usize, usize) {
    let acceptor_id = VarInt::from_u8(1);

    let writers_done = Arc::new(AtomicUsize::new(0));
    let readers_done = Arc::new(AtomicUsize::new(0));

    let writers_done_cl = writers_done.clone();
    let readers_done_sv = readers_done.clone();

    sim(|| {
        // ── Server: undersized recv pool, read-only ────────────────────────────
        // The server is the reader under contention. Size its recv pool like the dc-tester
        // production config so `num_streams` readers genuinely fight over it.
        {
            let readers_done_sv = readers_done_sv.clone();
            async move {
                let recv_pool_config =
                    CreditConfig::new(recv_cap).with_max_single_acquire_uniform(max_single_acquire);
                let server = SimEndpointConfig::default()
                    .recv_credit_pool_config(recv_pool_config)
                    .server();
                let mut acceptor = server
                    .register_acceptor_channel(acceptor_id, num_streams * 2)
                    .expect("acceptor registration failed");

                let mut idx = 0usize;
                while let Some(stream) = acceptor.recv().await {
                    let readers_done_sv = readers_done_sv.clone();
                    let stream_idx = idx;
                    idx += 1;
                    // `.primary()` keeps the sim alive until this reader has fully drained — a
                    // stalled reader fires its own watchdog instead of being silently abandoned
                    // when the client side finishes.
                    async move {
                        let (mut reader, _writer) = stream.into_split();
                        drain_reader(&mut reader, body_len, stream_idx).await;
                        readers_done_sv.fetch_add(1, Ordering::Relaxed);
                    }
                    .primary()
                    .spawn();
                }
            }
            .group("server")
            .spawn();
        }

        // ── Client: open all streams, upload concurrently ──────────────────────
        {
            async move {
                let mut client = Client::new();

                // Connect all streams up front so their writers all become backlogged in the same
                // window of sim time — this is what maximizes contention on the server recv pool.
                let mut streams = Vec::with_capacity(num_streams);
                for _ in 0..num_streams {
                    let stream = client
                        .connect("server:0", acceptor_id)
                        .await
                        .expect("connect failed");
                    streams.push(stream);
                }

                for (i, stream) in streams.into_iter().enumerate() {
                    let writers_done_cl = writers_done_cl.clone();
                    // Each writer is also `.primary()`: the run only ends once every upload has
                    // completed (or a watchdog panics).
                    async move {
                        let (_reader, mut writer) = stream.into_split();
                        drive_writer(&mut writer, body_len, i).await;
                        writers_done_cl.fetch_add(1, Ordering::Relaxed);
                    }
                    .primary()
                    .spawn();
                }
            }
            .group("client")
            .primary()
            .spawn();
        }
    });

    (
        writers_done.load(Ordering::Relaxed),
        readers_done.load(Ordering::Relaxed),
    )
}

/// Mirror of [`run_fair_share`] but contending the client-side **send** credit pool instead of the
/// server recv pool. The server gets a large recv pool (so the writer is never gated by the
/// advertised window) and the client gets an undersized send pool sized to `(send_cap,
/// max_single_acquire)`. This isolates the writer's local send-credit acquire/release loop — the
/// exact path dc-tester exercises with its production-sized send pool.
fn run_send_fair_share(
    num_streams: usize,
    body_len: u64,
    send_cap: u64,
    max_single_acquire: u64,
) -> (usize, usize) {
    let acceptor_id = VarInt::from_u8(1);

    let writers_done = Arc::new(AtomicUsize::new(0));
    let readers_done = Arc::new(AtomicUsize::new(0));

    let writers_done_cl = writers_done.clone();
    let readers_done_sv = readers_done.clone();

    sim(|| {
        // ── Server: large recv pool, read-only ─────────────────────────────────
        // The server reader must never be the bottleneck — give it a big recv pool so the only
        // contention in the system is the client's send pool.
        {
            let readers_done_sv = readers_done_sv.clone();
            async move {
                let server = Server::new();
                let mut acceptor = server
                    .register_acceptor_channel(acceptor_id, num_streams * 2)
                    .expect("acceptor registration failed");

                let mut idx = 0usize;
                while let Some(stream) = acceptor.recv().await {
                    let readers_done_sv = readers_done_sv.clone();
                    let stream_idx = idx;
                    idx += 1;
                    async move {
                        let (mut reader, _writer) = stream.into_split();
                        drain_reader(&mut reader, body_len, stream_idx).await;
                        readers_done_sv.fetch_add(1, Ordering::Relaxed);
                    }
                    .primary()
                    .spawn();
                }
            }
            .group("server")
            .spawn();
        }

        // ── Client: undersized send pool, open all streams, upload concurrently ─
        {
            async move {
                let send_pool_config =
                    CreditConfig::new(send_cap).with_max_single_acquire_uniform(max_single_acquire);
                let mut client = Client::with_config(
                    SimEndpointConfig::default().send_credit_pool_config(send_pool_config),
                );

                let mut streams = Vec::with_capacity(num_streams);
                for _ in 0..num_streams {
                    let stream = client
                        .connect("server:0", acceptor_id)
                        .await
                        .expect("connect failed");
                    streams.push(stream);
                }

                for (i, stream) in streams.into_iter().enumerate() {
                    let writers_done_cl = writers_done_cl.clone();
                    async move {
                        let (_reader, mut writer) = stream.into_split();
                        drive_writer(&mut writer, body_len, i).await;
                        writers_done_cl.fetch_add(1, Ordering::Relaxed);
                    }
                    .primary()
                    .spawn();
                }
            }
            .group("client")
            .primary()
            .spawn();
        }
    });

    (
        writers_done.load(Ordering::Relaxed),
        readers_done.load(Ordering::Relaxed),
    )
}

/// Reproduction of the dc-tester send-pool wedge: many concurrent writers share an undersized send
/// credit pool. With the production sizing (2 MiB capacity, 256 KiB per-acquire cap) and a 64 KiB
/// fair-share `min_grant_slice`, each writer asks for its full flow-control window, gets a partial
/// 64 KiB grant from the distributor, but `poll_acquire_credits` refuses to return until it has the
/// *whole* `want` — so it re-parks without sending the slice it holds. Every writer pins a slice
/// below its want, nothing is sent, nothing is released, and the pool wedges. If the writer is fixed
/// to send whatever partial credit it gets, every writer makes forward progress and completes.
#[test]
fn send_fair_share_partial_grants_no_stragglers() {
    let _no_snap = crate::testing::without_snapshots();
    let _no_trace = crate::testing::without_tracing();

    // 64 streams, 2 MiB each. 2 MiB send pool, 256 KiB per-acquire cap — production dc-tester
    // sizing. 64 streams * (>=64 KiB pinned each) overcommits the 2 MiB pool.
    let (writers, readers) = run_send_fair_share(64, 2 * 1024 * 1024, 2 * 1024 * 1024, 256 * 1024);

    info!(writers, readers, "send_fair_share result");
    assert_eq!(writers, 64, "all 64 writers must complete");
    assert_eq!(readers, 64, "all 64 readers must complete");
}

/// Smoke test: a handful of streams against a pool that comfortably fits a couple of windows.
/// Establishes the harness works end-to-end before scaling up to the contended case.
#[test]
fn fair_share_smoke_small() {
    let _no_snap = crate::testing::without_snapshots();
    let _no_trace = crate::testing::without_tracing();

    // 4 streams, 4 MiB each. 8 MiB pool, 1 MiB per-acquire cap — light contention.
    let (writers, readers) = run_fair_share(4, 4 * 1024 * 1024, 8 * 1024 * 1024, 1024 * 1024);

    assert_eq!(writers, 4, "all 4 writers must complete");
    assert_eq!(readers, 4, "all 4 readers must complete");
}

/// The real reproduction: 100 concurrent bulk uploads against a dc-tester-sized recv pool
/// (16 MiB capacity, 2 MiB per-acquire cap). With a 1 MiB per-stream initial window only ~16
/// streams can hold a full window at once, so the distributor must round-robin credit across all
/// 100. Each 4 MiB body is several per-stream windows past the 1 MiB unbacked initial window, so
/// every reader must park and re-acquire pool credit many times — sustained contention, the regime
/// where dc-tester stragglers appeared. If fair-share works, every writer and every reader makes
/// forward progress on every chunk and the run completes; any starved reader (or its blocked peer
/// writer) trips the chunk watchdog and panics. Body size is kept modest because the sim is
/// single-threaded: it must finish well inside the test runner's wall-clock cap while still forcing
/// deep round-robin (the property under test is liveness/fairness, not throughput).
#[test]
#[ignore = "this is fairly expensive so disabling for now"]
fn fair_share_100_streams_no_stragglers() {
    let _no_snap = crate::testing::without_snapshots();
    let _no_trace = crate::testing::without_tracing();

    let (writers, readers) =
        run_fair_share(100, 4 * 1024 * 1024, 16 * 1024 * 1024, 2 * 1024 * 1024);

    info!(writers, readers, "fair_share_100_streams result");
    assert_eq!(writers, 100, "all 100 writers must complete");
    assert_eq!(readers, 100, "all 100 readers must complete");
}

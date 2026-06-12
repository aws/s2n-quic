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

/// Reproduction of the dc-tester **read-path** send-pool drain under mid-stream cancellation.
///
/// In dc-tester reads, the aggregator issues each read to all 3 storage replicas and keeps the
/// first response, cancelling the other 2 by dropping/resetting its reader. The storage node is the
/// bulk **sender**, so it is *storage's send credit pool* that must recover the credit held by a
/// stream that gets cancelled mid-transfer. Writes never exercise this (the aggregator is the
/// sender on writes and is never reset), so a reset-path send-credit leak is invisible until reads
/// run — exactly the production symptom (storage RX at 144 Gbps, TX pinned at ~1.7 Gbps, reads
/// timing out).
///
/// This test maps the roles directly: the **server is the bulk sender** with a deliberately
/// undersized send pool, and the **client is the reader** that cancels a fraction of streams
/// mid-read (the first-wins replica cancellation). It runs many rounds so any per-cancel leak
/// accumulates and drains the pool. After every stream has settled and the pool is quiescent, the
/// pool must conserve `available + returned == capacity`; a reset-path leak shows up as the free
/// total falling short. The liveness watchdog on the surviving (un-cancelled) streams is the
/// secondary signal — once the pool drains, their writers can no longer acquire and stall.
fn run_reset_cancel_drain(
    rounds: usize,
    streams_per_round: usize,
    body_len: u64,
    cancel_after: u64,
    send_cap: u64,
    max_single_acquire: u64,
) -> i64 {
    let acceptor_id = VarInt::from_u8(1);

    // Captured out of the sim so the assertion can run after the simulation closes.
    let leak = Arc::new(AtomicUsize::new(0));
    let leak_capture = leak.clone();

    sim(|| {
        // ── Server: undersized SEND pool, acts as the bulk sender (storage) ─────
        {
            let leak_capture = leak_capture.clone();
            async move {
                let send_pool_config = CreditConfig::new(send_cap)
                    .with_max_single_acquire_uniform(max_single_acquire);
                let server = SimEndpointConfig::default()
                    .send_credit_pool_config(send_pool_config)
                    .server();
                // Expose the send pool so we can check conservation once everything quiesces.
                let send_pool = server.send_credit_pool();
                let mut acceptor = server
                    .register_acceptor_channel(acceptor_id, streams_per_round * rounds * 2)
                    .expect("acceptor registration failed");

                let mut idx = 0usize;
                while let Some(stream) = acceptor.recv().await {
                    let stream_idx = idx;
                    idx += 1;
                    // Each accepted stream's server side is the SENDER: push the full body. If the
                    // client cancels mid-read, the writer observes a reset and must release any
                    // send credit it was holding back to the pool.
                    async move {
                        let (mut reader, mut writer) = stream.into_split();
                        // Drain the small client request first so the stream is established.
                        let mut req = Data::new(64);
                        let _ = timeout(CHUNK_TIMEOUT, reader.read_into(&mut req)).await;
                        // Now stream the bulk response — this is the credit-consuming send.
                        let mut payload = Data::new(body_len);
                        loop {
                            if payload.is_finished() {
                                break;
                            }
                            match timeout(CHUNK_TIMEOUT, writer.write_from_fin(&mut payload)).await {
                                Ok(Ok(_)) => {}
                                // Cancelled by the peer reset, or genuinely done — either way this
                                // sender is finished. A real wedge (pool drained by leaked credit)
                                // surfaces on a *different*, un-cancelled stream's writer as a
                                // stall, panicking via the watchdog in `drive_writer`.
                                Ok(Err(_)) => break,
                                Err(_) => break,
                            }
                        }
                        let _ = stream_idx;
                    }
                    .primary()
                    .spawn();
                }

                // After the acceptor channel closes (all clients done and dropped), the system is
                // quiescent: no writer holds credit, nothing is in flight. The send pool must have
                // recovered every byte. Capture the shortfall for the post-sim assertion.
                let shortfall = send_pool.debug_capacity() as i64 - send_pool.debug_free_total();
                leak_capture.store(shortfall.max(0) as usize, Ordering::Relaxed);
            }
            .group("server")
            .spawn();
        }

        // ── Client: reader that cancels a fraction of streams mid-read ──────────
        {
            async move {
                let mut client = Client::new();

                for round in 0..rounds {
                    let mut streams = Vec::with_capacity(streams_per_round);
                    for _ in 0..streams_per_round {
                        let stream = client
                            .connect("server:0", acceptor_id)
                            .await
                            .expect("connect failed");
                        streams.push(stream);
                    }

                    for (i, mut stream) in streams.into_iter().enumerate() {
                        // Cancel 2 of every 3 streams mid-read, mirroring the aggregator keeping
                        // the first of 3 replicas and resetting the rest. The kept stream drains
                        // fully; the cancelled ones reset after `cancel_after` bytes.
                        let cancel = i % 3 != 0;
                        async move {
                            // Send a small request so the server accepts the stream and starts
                            // streaming the response back (mirrors a read request → bulk response).
                            {
                                let (_reader, writer) = stream.split();
                                let mut req = Data::new(64);
                                writer
                                    .write_from_fin(&mut req)
                                    .await
                                    .expect("client request write failed");
                            }
                            let mut rx = Data::new(body_len);
                            let mut read_total = 0u64;
                            loop {
                                let before = rx.current_offset().as_u64();
                                // Re-borrow the reader each iteration so `stream` stays owned and
                                // can be `reset()` on the cancel branch below.
                                let (reader, _writer) = stream.split();
                                let n = match timeout(CHUNK_TIMEOUT, reader.read_into(&mut rx)).await
                                {
                                    Ok(Ok(n)) => n,
                                    Ok(Err(_)) => break,
                                    Err(_) => {
                                        panic!(
                                            "reader stalled at offset {before}/{body_len} — \
                                             surviving stream could not make progress (send pool \
                                             drained by leaked credit on cancelled streams?)"
                                        );
                                    }
                                };
                                if n == 0 {
                                    break;
                                }
                                read_total += n as u64;
                                if cancel && read_total >= cancel_after {
                                    // First-wins cancellation: reset the stream mid-read. This
                                    // sends a QueueReset to the server's writer, which must
                                    // release its held send credit.
                                    stream.reset(crate::stream::endpoint::Error::StopSending);
                                    break;
                                }
                            }
                            // `stream` drops here, tearing down both halves.
                            let _ = round;
                        }
                        .primary()
                        .spawn();
                    }

                    // Let this round's streams settle before opening the next.
                    bach::time::sleep(Duration::from_millis(50)).await;
                }
            }
            .group("client")
            .primary()
            .spawn();
        }
    });

    leak.load(Ordering::Relaxed) as i64
}

/// Send credit must be fully recovered when streams are cancelled mid-transfer by a peer reset —
/// the dc-tester read path (storage = sender, aggregator cancels 2 of 3 replicas). If the reset
/// teardown leaks any held send credit, the pool drains over successive rounds and the conservation
/// check (`available + returned == capacity`) fails; on a real cluster this manifests as storage
/// receiving full rate but unable to send read responses.
#[test]
fn reset_cancel_send_credit_conserved() {
    let _no_snap = crate::testing::without_snapshots();
    let _no_trace = crate::testing::without_tracing();

    // 6 rounds × 24 streams = 144 cancellable transfers. 512 KiB body each, cancel survivors after
    // 128 KiB. 2 MiB send pool, 256 KiB per-acquire cap — production dc-tester sizing, oversubscribed
    // (24 concurrent senders vs a pool that holds ~8 windows) so any credit withheld by a
    // cancelled-but-not-yet-dropped writer drains it and stalls survivors.
    let leak = run_reset_cancel_drain(
        6,
        24,
        512 * 1024,
        128 * 1024,
        2 * 1024 * 1024,
        256 * 1024,
    );

    info!(leak, "reset_cancel send pool shortfall");
    assert_eq!(
        leak, 0,
        "send credit pool leaked {leak} bytes across reset-cancelled streams \
         (available + returned fell {leak} short of capacity)"
    );
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

/// Recv-credit conservation under the dc-tester **read** pattern with mid-read cancellation and
/// connection churn — the path the committed `reset_cancel_send_credit_conserved` test does NOT
/// cover (it only checks the *server send* pool).
///
/// Here the **client is the reader** that drops/cancels streams mid-transfer, so it is the
/// *client's RECV credit pool* under scrutiny. Each reader that the application abandons mid-stream
/// must reconcile its advertised receive window via `finish_recv_accounting` and return every
/// pool-backed credit it advertised-but-never-filled. Connection churn (fresh queue slots each
/// round, recycled bindings) is what stresses the per-slot `advertised_window` / `recv_finished`
/// reconciliation against `observe_offset`'s per-arrival release.
///
/// If the reader teardown leaks any advertised-but-unfilled recv credit, the client recv pool
/// drains over rounds and later readers can no longer advertise a window — their peer writers stall
/// and the surviving (un-cancelled) streams' chunk watchdog fires. At quiescence the pool must
/// conserve `available + returned == capacity`.
#[allow(clippy::too_many_arguments)]
fn run_recv_churn_cancel(
    rounds: usize,
    streams_per_round: usize,
    body_len: u64,
    cancel_after: u64,
    recv_cap: u64,
    max_single_acquire: u64,
    per_stream_window: u64,
) -> i64 {
    let acceptor_id = VarInt::from_u8(1);
    let shortfall = Arc::new(AtomicUsize::new(0));
    let shortfall_cap = shortfall.clone();
    let window = VarInt::new(per_stream_window).unwrap();

    sim(move || {
        // ── Server: bulk sender (large send pool, never the bottleneck) ─────────
        {
            async move {
                let server = SimEndpointConfig::default()
                    .send_window(window)
                    .recv_credit_pool_config(
                        CreditConfig::new(recv_cap)
                            .with_max_single_acquire_uniform(max_single_acquire),
                    )
                    .server();
                let mut acceptor = server
                    .register_acceptor_channel(acceptor_id, streams_per_round * rounds * 2)
                    .expect("acceptor registration failed");

                while let Some(stream) = acceptor.recv().await {
                    async move {
                        let (mut reader, mut writer) = stream.into_split();
                        let mut req = Data::new(64);
                        let _ = timeout(CHUNK_TIMEOUT, reader.read_into(&mut req)).await;
                        let mut payload = Data::new(body_len);
                        loop {
                            if payload.is_finished() {
                                break;
                            }
                            match timeout(CHUNK_TIMEOUT, writer.write_from_fin(&mut payload)).await {
                                Ok(Ok(_)) => {}
                                Ok(Err(_)) => break,
                                Err(_) => break,
                            }
                        }
                    }
                    .primary()
                    .spawn();
                }
            }
            .group("server")
            .spawn();
        }

        // ── Client: reader pool under test, cancels 2/3 mid-read, churns rounds ──
        {
            let shortfall_cap = shortfall_cap.clone();
            async move {
                let recv_pool_config = CreditConfig::new(recv_cap)
                    .with_max_single_acquire_uniform(max_single_acquire);
                let config = SimEndpointConfig::default()
                    .send_window(window)
                    .recv_credit_pool_config(recv_pool_config);
                let mut client = Client::with_config(config);
                let recv_pool = client.recv_credit_pool();

                for _round in 0..rounds {
                    let mut streams = Vec::with_capacity(streams_per_round);
                    for _ in 0..streams_per_round {
                        let stream = client
                            .connect("server:0", acceptor_id)
                            .await
                            .expect("connect failed");
                        streams.push(stream);
                    }

                    let mut handles = Vec::with_capacity(streams_per_round);
                    for (i, mut stream) in streams.into_iter().enumerate() {
                        let cancel = i % 3 != 0;
                        let handle = async move {
                            {
                                let (_reader, writer) = stream.split();
                                let mut req = Data::new(64);
                                if writer.write_from_fin(&mut req).await.is_err() {
                                    return;
                                }
                            }
                            let mut rx = Data::new(body_len);
                            let mut read_total = 0u64;
                            loop {
                                let before = rx.current_offset().as_u64();
                                let (reader, _writer) = stream.split();
                                let n = match timeout(CHUNK_TIMEOUT, reader.read_into(&mut rx)).await
                                {
                                    Ok(Ok(n)) => n,
                                    Ok(Err(_)) => break,
                                    Err(_) => {
                                        panic!(
                                            "reader stalled at offset {before}/{body_len} — \
                                             recv pool drained by leaked credit on cancelled \
                                             readers?"
                                        );
                                    }
                                };
                                if n == 0 {
                                    break;
                                }
                                read_total += n as u64;
                                if cancel && read_total >= cancel_after {
                                    stream.reset(crate::stream::endpoint::Error::StopSending);
                                    break;
                                }
                            }
                        }
                        .primary()
                        .spawn();
                        handles.push(handle);
                    }

                    for handle in handles {
                        let _ = handle.await;
                    }
                    bach::time::sleep(Duration::from_millis(50)).await;
                }

                // Drain the final teardown before sampling.
                bach::time::sleep(Duration::from_secs(1)).await;
                let s = recv_pool.debug_capacity() as i64 - recv_pool.debug_free_total();
                shortfall_cap.store(s.max(0) as usize, Ordering::Relaxed);
            }
            .group("client")
            .primary()
            .spawn();
        }
    });

    shortfall.load(Ordering::Relaxed) as i64
}

/// Recv credit must be fully recovered when a reader is cancelled mid-transfer. The client reader
/// abandons 2 of 3 streams mid-read across churned rounds; if `finish_recv_accounting` fails to
/// return the advertised-but-unfilled window on any teardown, the recv pool drains and the
/// conservation check fails.
#[test]
fn recv_cancel_credit_conserved() {
    let _no_snap = crate::testing::without_snapshots();
    let _no_trace = crate::testing::without_tracing();

    // 6 rounds × 24 streams = 144 cancellable reads. 512 KiB body, cancel survivors after 128 KiB.
    // 8 MiB recv pool, 2 MiB per-acquire cap, 256 KiB initial window — so each reader must acquire
    // pool credit (body is past the initial window) and a cancelled reader holds advertised window
    // that must come back.
    let leak = run_recv_churn_cancel(
        6,
        24,
        512 * 1024,
        128 * 1024,
        8 * 1024 * 1024,
        2 * 1024 * 1024,
        256 * 1024,
    );

    info!(leak, "recv_cancel recv pool shortfall");
    assert_eq!(
        leak, 0,
        "recv credit pool leaked {leak} bytes across reset-cancelled readers \
         (available + returned fell {leak} short of capacity)"
    );
}

/// Read-pattern stall reproduction **with packet loss**. The production cluster shows growing
/// PN-threshold loss over time alongside reads timing out; this drives many concurrent reads with
/// a small per-stream window (so the reader must emit a MAX_DATA top-up roughly every window's
/// worth of consumption) and drops a deterministic 1-in-N fraction of packets in both directions.
///
/// The hypothesis under test: a lost `QueueMaxData` (window grant) wedges the peer writer. The
/// writer advances `remote_max_data` only from MAX_DATA frames it actually receives; the reader
/// advances its local `remote_max_data` the moment it *sends* a grant and computes the next
/// `delta` against that already-advanced value. So a dropped MAX_DATA can only be recovered by
/// retransmission of that exact frame — never re-derived by the reader's top-up logic. If that
/// retransmission ever fails to fire (e.g. the frame completed/cancelled out of the inflight map,
/// or window growth latched), the writer stalls forever with data to send and the reader parks
/// waiting for bytes that will never come. The chunk watchdog in `drain_reader`/`drive_writer`
/// fires on either side.
fn run_loss_read_stall(
    num_streams: usize,
    body_len: u64,
    per_stream_window: u64,
    recv_cap: u64,
    max_single_acquire: u64,
    drop_one_in: u64,
) -> (usize, usize) {
    let acceptor_id = VarInt::from_u8(1);
    let writers_done = Arc::new(AtomicUsize::new(0));
    let readers_done = Arc::new(AtomicUsize::new(0));
    let writers_done_sv = writers_done.clone();
    let readers_done_cl = readers_done.clone();
    let window = VarInt::new(per_stream_window).unwrap();

    sim(move || {
        // Deterministic partial loss in BOTH directions. A free-running counter dropping every
        // Nth packet exercises MAX_DATA / data / ACK loss uniformly without RNG.
        {
            let mut pkt = 0u64;
            bach::net::monitor::on_packet_sent(move |_packet| {
                pkt += 1;
                if drop_one_in > 0 && pkt % drop_one_in == 0 {
                    return bach::net::monitor::Command::Drop;
                }
                bach::net::monitor::Command::Pass
            });
        }

        // ── Server: bulk sender; large pools so only loss + window gate it ──────
        {
            let writers_done_sv = writers_done_sv.clone();
            async move {
                let server = SimEndpointConfig::default().send_window(window).server();
                let mut acceptor = server
                    .register_acceptor_channel(acceptor_id, num_streams * 2)
                    .expect("acceptor registration failed");

                let mut idx = 0usize;
                while let Some(stream) = acceptor.recv().await {
                    let writers_done_sv = writers_done_sv.clone();
                    let stream_idx = idx;
                    idx += 1;
                    async move {
                        let (mut reader, mut writer) = stream.into_split();
                        let mut req = Data::new(64);
                        let _ = timeout(CHUNK_TIMEOUT, reader.read_into(&mut req)).await;
                        drive_writer(&mut writer, body_len, stream_idx).await;
                        writers_done_sv.fetch_add(1, Ordering::Relaxed);
                    }
                    .primary()
                    .spawn();
                }
            }
            .group("server")
            .spawn();
        }

        // ── Client: many concurrent readers, small window forces MAX_DATA churn ─
        {
            let readers_done_cl = readers_done_cl.clone();
            async move {
                let recv_pool_config = CreditConfig::new(recv_cap)
                    .with_max_single_acquire_uniform(max_single_acquire);
                let mut client = Client::with_config(
                    SimEndpointConfig::default()
                        .send_window(window)
                        .recv_credit_pool_config(recv_pool_config),
                );

                let mut streams = Vec::with_capacity(num_streams);
                for _ in 0..num_streams {
                    let stream = client
                        .connect("server:0", acceptor_id)
                        .await
                        .expect("connect failed");
                    streams.push(stream);
                }

                for (i, mut stream) in streams.into_iter().enumerate() {
                    let readers_done_cl = readers_done_cl.clone();
                    async move {
                        {
                            let (_reader, writer) = stream.split();
                            let mut req = Data::new(64);
                            writer.write_from_fin(&mut req).await.expect("req write");
                        }
                        let (mut reader, _writer) = stream.into_split();
                        drain_reader(&mut reader, body_len, i).await;
                        readers_done_cl.fetch_add(1, Ordering::Relaxed);
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

/// Read pattern under sustained partial packet loss must still complete: a lost MAX_DATA window
/// grant must always be recovered by retransmission, never permanently stall the peer writer.
#[test]
fn loss_read_pattern_no_stall() {
    let _no_snap = crate::testing::without_snapshots();
    let _no_trace = crate::testing::without_tracing();

    // 16 concurrent reads, 1 MiB each, 128 KiB per-stream window (8 MAX_DATA top-ups per stream),
    // drop every 7th packet in both directions. Large recv pool so the *only* gates are the
    // per-stream window and loss recovery — isolating the lost-MAX_DATA hypothesis.
    let (writers, readers) = run_loss_read_stall(
        16,
        1024 * 1024,
        128 * 1024,
        64 * 1024 * 1024,
        2 * 1024 * 1024,
        7,
    );

    info!(writers, readers, "loss_read_pattern result");
    assert_eq!(writers, 16, "all 16 writers must complete under loss");
    assert_eq!(readers, 16, "all 16 readers must complete under loss");
}

/// Heavier-loss variant: tiny window (32 KiB → 32 MAX_DATA top-ups per stream) and 1-in-3 packet
/// loss, which makes MAX_DATA loss the common case rather than the exception. If any single lost
/// window grant fails to be retransmitted, a writer wedges and its reader's watchdog fires.
#[test]
fn heavy_loss_read_pattern_no_stall() {
    let _no_snap = crate::testing::without_snapshots();
    let _no_trace = crate::testing::without_tracing();

    let (writers, readers) = run_loss_read_stall(
        8,
        512 * 1024,
        32 * 1024,
        64 * 1024 * 1024,
        2 * 1024 * 1024,
        3,
    );

    info!(writers, readers, "heavy_loss_read_pattern result");
    assert_eq!(writers, 8, "all 8 writers must complete under heavy loss");
    assert_eq!(readers, 8, "all 8 readers must complete under heavy loss");
}

/// The full production stew: contended **server send pool** (storage is the bulk sender) + read
/// pattern + first-wins cancellation (aggregator keeps 1 of 3 replicas) + connection churn +
/// partial packet loss, all at once. This is the regime where the cluster stalls: storage RX at
/// line rate, TX pinned far below, reads timing out, PN-threshold loss climbing.
///
/// The combination is what no isolated test covers. A cancelled reader resets mid-transfer (loss
/// can delay or drop that reset, so the server writer keeps its inflight frames longer); a lost
/// MAX_DATA delays a survivor's window; a churned binding recycles a slot whose prior generation
/// may still have credit in flight. If any of these interactions strands send-pool credit, the
/// pool drains over rounds and survivors stall — caught by the watchdog — and the post-sim
/// conservation check (`available + returned == capacity`) fails.
#[allow(clippy::too_many_arguments)]
fn run_full_stew(
    rounds: usize,
    streams_per_round: usize,
    body_len: u64,
    cancel_after: u64,
    per_stream_window: u64,
    send_cap: u64,
    max_single_acquire: u64,
    drop_one_in: u64,
) -> i64 {
    let acceptor_id = VarInt::from_u8(1);
    let shortfall = Arc::new(AtomicUsize::new(0));
    let shortfall_cap = shortfall.clone();
    let window = VarInt::new(per_stream_window).unwrap();

    sim(move || {
        {
            let mut pkt = 0u64;
            bach::net::monitor::on_packet_sent(move |_packet| {
                pkt += 1;
                if drop_one_in > 0 && pkt % drop_one_in == 0 {
                    return bach::net::monitor::Command::Drop;
                }
                bach::net::monitor::Command::Pass
            });
        }

        // ── Server: bulk sender, undersized SEND pool (storage) ─────────────────
        {
            let shortfall_cap = shortfall_cap.clone();
            async move {
                let send_pool_config = CreditConfig::new(send_cap)
                    .with_max_single_acquire_uniform(max_single_acquire);
                let server = SimEndpointConfig::default()
                    .send_window(window)
                    .send_credit_pool_config(send_pool_config)
                    .server();
                let send_pool = server.send_credit_pool();
                let mut acceptor = server
                    .register_acceptor_channel(acceptor_id, streams_per_round * rounds * 2)
                    .expect("acceptor registration failed");

                while let Some(stream) = acceptor.recv().await {
                    async move {
                        let (mut reader, mut writer) = stream.into_split();
                        let mut req = Data::new(64);
                        let _ = timeout(CHUNK_TIMEOUT, reader.read_into(&mut req)).await;
                        let mut payload = Data::new(body_len);
                        loop {
                            if payload.is_finished() {
                                break;
                            }
                            match timeout(CHUNK_TIMEOUT, writer.write_from_fin(&mut payload)).await {
                                Ok(Ok(_)) => {}
                                Ok(Err(_)) => break,
                                Err(_) => {
                                    panic!(
                                        "server writer stalled mid-send — send pool drained by \
                                         stranded credit under loss+cancel+churn"
                                    );
                                }
                            }
                        }
                    }
                    .primary()
                    .spawn();
                }

                bach::time::sleep(Duration::from_secs(2)).await;
                let s = send_pool.debug_capacity() as i64 - send_pool.debug_free_total();
                shortfall_cap.store(s.max(0) as usize, Ordering::Relaxed);
            }
            .group("server")
            .spawn();
        }

        // ── Client: reader, cancels 2/3 mid-read, churns rounds, under loss ─────
        {
            async move {
                let mut client = Client::with_config(
                    SimEndpointConfig::default().send_window(window),
                );

                for _round in 0..rounds {
                    let mut streams = Vec::with_capacity(streams_per_round);
                    for _ in 0..streams_per_round {
                        let stream = client
                            .connect("server:0", acceptor_id)
                            .await
                            .expect("connect failed");
                        streams.push(stream);
                    }

                    let mut handles = Vec::with_capacity(streams_per_round);
                    for (i, mut stream) in streams.into_iter().enumerate() {
                        let cancel = i % 3 != 0;
                        let handle = async move {
                            {
                                let (_reader, writer) = stream.split();
                                let mut req = Data::new(64);
                                if writer.write_from_fin(&mut req).await.is_err() {
                                    return;
                                }
                            }
                            let mut rx = Data::new(body_len);
                            let mut read_total = 0u64;
                            loop {
                                let (reader, _writer) = stream.split();
                                let n = match timeout(CHUNK_TIMEOUT, reader.read_into(&mut rx)).await
                                {
                                    Ok(Ok(n)) => n,
                                    Ok(Err(_)) => break,
                                    Err(_) => break,
                                };
                                if n == 0 {
                                    break;
                                }
                                read_total += n as u64;
                                if cancel && read_total >= cancel_after {
                                    stream.reset(crate::stream::endpoint::Error::StopSending);
                                    break;
                                }
                            }
                        }
                        .primary()
                        .spawn();
                        handles.push(handle);
                    }

                    for handle in handles {
                        let _ = handle.await;
                    }
                    bach::time::sleep(Duration::from_millis(50)).await;
                }
            }
            .group("client")
            .primary()
            .spawn();
        }
    });

    shortfall.load(Ordering::Relaxed) as i64
}

/// Full production stew must conserve send credit and never stall survivors.
#[test]
fn full_stew_loss_cancel_churn_conserves() {
    let _no_snap = crate::testing::without_snapshots();
    let _no_trace = crate::testing::without_tracing();

    // 6 rounds × 12 streams = 72 transfers, 256 KiB body, cancel survivors after 64 KiB,
    // 64 KiB per-stream window (4 MAX_DATA top-ups/stream), 2 MiB send pool / 256 KiB cap
    // (production sizing), drop every 5th packet.
    let leak = run_full_stew(
        6,
        12,
        256 * 1024,
        64 * 1024,
        64 * 1024,
        2 * 1024 * 1024,
        256 * 1024,
        5,
    );

    info!(leak, "full_stew send pool shortfall");
    assert_eq!(
        leak, 0,
        "send credit pool leaked {leak} bytes under loss+cancel+churn"
    );
}

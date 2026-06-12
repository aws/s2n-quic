// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Tests for the stream Reader.
//!
//! ## Organization
//!
//! * **Synchronous unit tests** – exercise `write_data_reader` directly without
//!   an async runtime or task scheduler; these test an internal helper function
//!   in isolation.
//!
//! * **Bach async tests** – each test runs inside `crate::testing::sim` and uses
//!   **two separate primary tasks** to model how a real application and endpoint
//!   interact:
//!
//!   * **Application task** (primary) – owns the [`Reader`]; calls `read_into`
//!     and asserts on the data it receives.
//!   * **Endpoint task** (primary) – owns the [`Pusher`]; sends [`msg::Stream`]
//!     messages into the flow queue and asserts on [`Frame`]s the Reader emits
//!     (e.g. `MAX_DATA`, `STOP_SENDING`).
//!
//!   Both tasks are marked `.primary()` so the sim runs until both complete.
//!   The two sides talk over the real flow-queue / frame-submission channels,
//!   without any actual UDP sockets or cryptography.

use super::{error, msg, write_data_reader, ReadToEnd, Reader};
use crate::{
    endpoint::frame::{self, Frame, Header, PriorityStorage, SubmissionReceiver},
    intrusive,
    packet::datagram::ResetTarget,
    path::secret::map::Entry as PathSecretEntry,
    stream::metrics::ReaderMetrics,
    testing::{ext::*, sim},
};
use bytes::BytesMut;
use s2n_quic_core::{
    buffer::{writer::Storage as _, Reassembler},
    endpoint,
    stream::testing::Data,
    varint::VarInt,
};
use std::{net::SocketAddr, sync::Arc, time::Duration};

// ─── Test helpers ─────────────────────────────────────────────────────────────

fn make_pair() -> (Reader, Pusher) {
    make_pair_with_pool(crate::sync::Arc::new(crate::credit::Pool::new(
        crate::credit::Config::default(),
    )))
}

fn make_pair_with_pool(
    recv_credit_pool: crate::sync::Arc<crate::credit::Pool>,
) -> (Reader, Pusher) {
    // Most tests don't exercise pool accounting, so the slot's unbacked initial window defaults to
    // zero here. `make_pair_for_conservation` seeds it to the reader's window (matching production
    // in `path/secret/map/entry.rs`) so the acquire/release books actually balance.
    make_pair_with_pool_and_initial_window(recv_credit_pool, 0)
}

fn make_pair_with_pool_and_initial_window(
    recv_credit_pool: crate::sync::Arc<crate::credit::Pool>,
    initial_recv_window: u64,
) -> (Reader, Pusher) {
    make_pair_with_pool_and_initial_window_and_entry(
        recv_credit_pool,
        initial_recv_window,
        PathSecretEntry::builder("127.0.0.1:4433".parse().unwrap())
            .endpoint_type(endpoint::Type::Client)
            .build(),
    )
}

fn make_pair_with_pool_and_initial_window_and_entry(
    recv_credit_pool: crate::sync::Arc<crate::credit::Pool>,
    initial_recv_window: u64,
    path_secret_entry: std::sync::Arc<PathSecretEntry>,
) -> (Reader, Pusher) {
    let client_state = std::sync::Arc::new(crate::queue::ClientState::new(
        VarInt::from_u16(100),
        initial_recv_window,
    ));
    let dest_queue_id = client_state.peer_free.try_alloc().unwrap();
    let alloc = client_state.alloc_local(dest_queue_id).unwrap();
    let dispatcher = crate::queue::ClientDispatch::new(client_state);

    let queue_id = alloc.stream.queue_id();
    let binding_id = alloc.stream.binding_id();

    let (frame_tx, frame_rx) = frame::submission_channel(1);

    let reader = Reader::new_client(
        frame_tx,
        path_secret_entry,
        dest_queue_id,
        alloc.stream,
        crate::time::DefaultClock::default(),
        Arc::new(ReaderMetrics::new(
            &crate::counter::Registry::default(),
            "test",
        )),
        recv_credit_pool.clone(),
        crate::credit::Priority::default(),
    );

    let pusher = Pusher {
        dispatcher,
        queue_id,
        binding_id,
        frame_rx,
        frame_storage: PriorityStorage::default(),
        recv_credit_pool: Some(recv_credit_pool),
    };

    (reader, pusher)
}

/// Build a *server* reader (via `Reader::new_server`) plus a `Pusher` that injects stream messages
/// into the same queue slot. `peer_fin_received = false` so the server reader emits a flow update
/// after consuming, modeling the binding-confirmation path. The slot's unbacked initial window is
/// seeded to `initial_recv_window` to match production.
fn make_server_pair_with_pool_and_initial_window(
    recv_credit_pool: crate::sync::Arc<crate::credit::Pool>,
    initial_recv_window: u64,
) -> (Reader, Pusher) {
    let peer: SocketAddr = "127.0.0.1:4433".parse().unwrap();
    let path_secret_entry = PathSecretEntry::builder(peer)
        .endpoint_type(endpoint::Type::Client)
        .build();

    let client_state = std::sync::Arc::new(crate::queue::ClientState::new(
        VarInt::from_u16(100),
        initial_recv_window,
    ));
    let dest_queue_id = client_state.peer_free.try_alloc().unwrap();
    let alloc = client_state.alloc_local(dest_queue_id).unwrap();
    let dispatcher = crate::queue::ClientDispatch::new(client_state);

    let queue_id = alloc.stream.queue_id();
    let binding_id = alloc.stream.binding_id();

    let (frame_tx, frame_rx) = frame::submission_channel(1);

    let reader = Reader::new_server(
        frame_tx,
        path_secret_entry,
        dest_queue_id,
        alloc.stream,
        false, // peer_fin_received: emit flow update after consuming
        crate::time::DefaultClock::default(),
        Arc::new(ReaderMetrics::new(
            &crate::counter::Registry::default(),
            "test",
        )),
        recv_credit_pool.clone(),
        crate::credit::Priority::default(),
    );

    let pusher = Pusher {
        dispatcher,
        queue_id,
        binding_id,
        frame_rx,
        frame_storage: PriorityStorage::default(),
        recv_credit_pool: Some(recv_credit_pool),
    };

    (reader, pusher)
}

#[test]
fn peer_addr_returns_handshake_addr() {
    let (reader, _) = make_pair();
    let expected: SocketAddr = "127.0.0.1:4433".parse().unwrap();
    assert_eq!(reader.peer_addr(), expected);
}

#[test]
fn application_data_is_none_when_not_set() {
    let (reader, _) = make_pair();
    assert!(reader.application_data().is_none());
}

#[test]
fn application_data_returns_value_from_path_secret_entry() {
    let peer: SocketAddr = "127.0.0.1:4433".parse().unwrap();
    let data: crate::path::secret::map::ApplicationData = Arc::new(42u32);
    let entry = PathSecretEntry::builder(peer)
        .endpoint_type(endpoint::Type::Client)
        .application_data(Some(data.clone()))
        .build();
    let pool = crate::sync::Arc::new(crate::credit::Pool::new(crate::credit::Config::default()));
    let (reader, _) = make_pair_with_pool_and_initial_window_and_entry(pool, 0, entry);
    let returned = reader
        .application_data()
        .expect("application_data should be Some");
    assert!(Arc::ptr_eq(returned, &data));
}

/// Finding 2 (bootstrap deadlock): a server reader must advertise its initial window — the
/// MAX_DATA that confirms the binding and unblocks the peer writer out of `InitSent` — even when
/// the recv pool is fully drained. The initial window is unbacked (the peer is already bounded by
/// the handshake `local_recv_max_data`), so advertising it must not require pool credit.
///
/// Before the unbacked-window fix, the server's first `maybe_send_max_data` called `poll_acquire`
/// for the whole window; against a drained pool that returned `Pending`, no MAX_DATA was sent, and
/// the peer hung in `InitSent` forever (the connection wedged, and in the sim the run never
/// terminated → unbounded memory). This test pins that the MAX_DATA still goes out.
#[test]
fn server_advertises_initial_window_against_drained_pool() {
    sim(|| {
        // Zero-capacity pool with an unrestricted per-priority cap: any pool acquire parks
        // forever (no distributor is run). The server must NOT depend on the pool to advertise
        // its initial window.
        let cfg = crate::credit::Config {
            capacity: 0,
            max_single_acquire: [u64::MAX; crate::credit::Priority::LEVELS],
            min_grant_slice: [u64::MAX; crate::credit::Priority::LEVELS],
        };
        let pool = crate::sync::Arc::new(crate::credit::Pool::new(cfg));
        // Hold a distributor so the pool stays open (drop would close it and change the path).
        let distributor = crate::credit::Distributor::new(pool.clone());

        // Seed the slot's unbacked initial window to the reader's window, as production does.
        let (mut reader, mut pusher) =
            make_server_pair_with_pool_and_initial_window(pool, 1024 * 1024);
        let window_size = reader.0.window_size;
        assert!(window_size > 0);

        async move {
            // Peer sends a small amount of data (well within the unbacked window).
            pusher.push_data(0, b"hello server", false);
            // The server must emit a MAX_DATA advertising its initial window despite the drained
            // pool — this is the binding-confirmation signal.
            let frames = pusher.recv_frames_timeout(1.s()).await;
            let frames = frames.expect(
                "server must advertise its initial window (send MAX_DATA) even when the recv \
                 pool is drained — otherwise the peer writer hangs in InitSent (Finding 2)",
            );
            let max_data = frames
                .iter()
                .find_map(decode_max_data_from_queue_control)
                .expect("expected a QueueMaxData frame confirming the binding");
            assert!(
                max_data.as_u64() > 0,
                "advertised window must be > 0 to confirm the binding, got {}",
                max_data.as_u64()
            );
            let _keep_alive = &distributor;
            1.s().sleep().await;
        }
        .primary()
        .spawn();

        async move {
            let mut buf = BytesMut::with_capacity(64);
            let n = reader
                .read_into(&mut buf)
                .await
                .expect("server read failed");
            assert_eq!(n, b"hello server".len());
            assert_eq!(&buf[..], b"hello server");
            1.s().sleep().await;
        }
        .primary()
        .spawn();
    });
}

/// Mock endpoint side of a reader test.
///
/// `push_*` injects [`msg::Stream`] messages into the flow-queue dispatcher,
/// automatically waking any waiting Reader task.  `recv_frames` asynchronously
/// waits for [`Frame`]s that the Reader submitted (e.g. `MAX_DATA`,
/// `STOP_SENDING`).
struct Pusher {
    dispatcher: crate::queue::ClientDispatch,
    queue_id: VarInt,
    binding_id: VarInt,
    /// Outbound frames submitted by the Reader (MAX_DATA, STOP_SENDING, …).
    frame_rx: SubmissionReceiver,
    /// Reusable priority-storage buffer; avoids re-allocating the fixed-size
    /// array on every `recv_frames` call.
    frame_storage: PriorityStorage,
    /// Recv credit pool, so the pusher can mirror the real dispatch path and
    /// `release` the `release_bytes` returned by `send_stream`. Conservation
    /// tests rely on this; most tests ignore it.
    recv_credit_pool: Option<crate::sync::Arc<crate::credit::Pool>>,
}

impl Pusher {
    fn push(&mut self, message: msg::Stream) {
        let (_waker, release_bytes) = self
            .dispatcher
            .send_stream(
                self.queue_id,
                self.binding_id,
                intrusive::Entry::new(message),
            )
            .unwrap_or_else(|_| panic!("send_stream should succeed in tests"));
        // Mirror `endpoint/dispatch.rs`: as buffered bytes cross the unbacked
        // initial window they are released back to the recv pool.
        if let Some(pool) = &self.recv_credit_pool {
            pool.release(release_bytes);
        }
    }

    fn push_data(&mut self, offset: u64, data: &[u8], fin: bool) {
        let peer_max_offset = offset + data.len() as u64;
        self.push_data_hint(offset, data, fin, peer_max_offset);
    }

    /// Push a data frame with an explicit writer high-watermark hint. Use when the test needs the
    /// writer to signal it wants to send beyond this frame (so the reader extends its window).
    fn push_data_hint(&mut self, offset: u64, data: &[u8], fin: bool, peer_max_offset: u64) {
        self.push(msg::Stream::Data {
            offset: VarInt::new(offset).unwrap(),
            peer_max_offset: VarInt::new(peer_max_offset).unwrap(),
            payload: BytesMut::from(data),
            fin,
            blocked: false,
        });
    }

    /// Push a data frame carrying the writer's `blocked` signal with an explicit desired offset.
    #[allow(dead_code)]
    fn push_data_blocked(&mut self, offset: u64, data: &[u8], peer_max_offset: u64) {
        self.push(msg::Stream::Data {
            offset: VarInt::new(offset).unwrap(),
            peer_max_offset: VarInt::new(peer_max_offset).unwrap(),
            payload: BytesMut::from(data),
            fin: false,
            blocked: true,
        });
    }

    /// Push a standalone `QueueDataBlocked` signal (real peer streaming back-pressure).
    #[allow(dead_code)]
    fn push_blocked(&mut self, desired_offset: u64) {
        self.push(msg::Stream::Blocked {
            desired_offset: VarInt::new(desired_offset).unwrap(),
            synthetic: false,
        });
    }

    /// Push a synthetic blocked signal (receiver-generated for an oversized QueueMsg segment).
    #[allow(dead_code)]
    fn push_synthetic_blocked(&mut self, desired_offset: u64) {
        self.push(msg::Stream::Blocked {
            desired_offset: VarInt::new(desired_offset).unwrap(),
            synthetic: true,
        });
    }

    fn push_reset(&mut self, error_code: VarInt) {
        self.push(msg::Stream::Reset { error_code });
    }

    /// Asynchronously wait for frames submitted by the Reader.
    ///
    /// Suspends until at least one frame (or a channel-close) is received,
    /// then returns all frames collected in that batch as a flat intrusive
    /// queue.  The `PriorityStorage` allocation is reused across calls.
    ///
    /// # Iterating the result
    ///
    /// Use [`Queue::iter`][`intrusive_queue::Queue::iter`] to borrow frames
    /// without consuming them, or iterate by value to take ownership of each
    /// `Entry<Frame>`.  Entries deref to `Frame` so you can access fields
    /// (e.g. `entry.header`) without calling `into_inner`.
    async fn recv_frames(&mut self) -> intrusive::Queue<Frame> {
        core::future::poll_fn(|cx| self.frame_rx.poll_swap(cx, &mut self.frame_storage)).await;
        let mut result = intrusive::Queue::default();
        for (_priority, mut queue) in self.frame_storage.drain() {
            result.append(&mut queue);
        }
        result
    }

    /// Asynchronously waits for frames up to `duration`.
    ///
    /// Returns `Some(queue)` when at least one frame is received before timeout.
    /// Returns `None` on timeout or when only an empty wake/close is observed.
    async fn recv_frames_timeout(&mut self, duration: Duration) -> Option<intrusive::Queue<Frame>> {
        let queue = bach::time::timeout(duration, self.recv_frames())
            .await
            .ok()?;
        if queue.is_empty() {
            None
        } else {
            Some(queue)
        }
    }

    fn complete_with_status(
        &mut self,
        mut frames: intrusive::Queue<Frame>,
        status: frame::TransmissionStatus,
    ) {
        while let Some(entry) = frames.pop_front() {
            let mut completed = entry.into_inner();
            let Some(sender) = completed.completion.take() else {
                continue;
            };
            completed.status = status;

            let mut queue = intrusive::Queue::new();
            queue.push_back(completed.into());
            sender
                .send_batch(queue)
                .expect("completion send should succeed in tests");
        }
    }
}

fn decode_max_data_from_queue_control(frame: &Frame) -> Option<VarInt> {
    match frame.header {
        Header::QueueMaxData { maximum_data, .. } => Some(maximum_data),
        _ => None,
    }
}

// ─── write_data_reader unit tests (no I/O, no tasks) ──────────────────────────

#[test]
fn write_data_reader_bypasses_reassembler_for_in_order_data() {
    let mut reassembler = Reassembler::new();
    let mut reader = Data::new(8);
    let mut app_buf: Vec<u8> = Vec::new();

    write_data_reader(&mut reassembler, &mut reader, &mut app_buf).unwrap();

    assert_eq!(app_buf, Data::send_one_at(0, 8));
    assert_eq!(reassembler.consumed_len(), 8);
    assert_eq!(reassembler.final_size(), Some(8));
    assert!(reassembler.is_empty());
    assert!(reassembler.is_reading_complete());
}

#[test]
fn write_data_reader_keeps_out_of_order_data_in_reassembler() {
    let mut reassembler = Reassembler::new();
    let mut reader = Data::new(8);
    let mut app_buf: Vec<u8> = Vec::new();

    reader.seek_forward(4);
    write_data_reader(&mut reassembler, &mut reader, &mut app_buf).unwrap();

    // Nothing was delivered to the application yet — the tail (offset 4-7) is
    // buffered in the reassembler, but there is a gap at 0-3.  `is_empty()` and
    // `total_received_len()` both report zero because they only count bytes
    // contiguous from the current read position (offset 0).  `final_size()` is
    // set, confirming the tail and FIN were recorded internally.
    assert!(app_buf.is_empty());
    assert_eq!(reassembler.consumed_len(), 0);
    assert_eq!(reassembler.total_received_len(), 0);
    assert!(reassembler.is_empty());
    assert!(!reassembler.is_reading_complete());
    assert_eq!(
        reassembler.final_size(),
        Some(8),
        "FIN should be recorded even though the head is missing"
    );

    // Once the missing head is written, all 8 bytes become available.
    reassembler
        .write_at(0u32.into(), &Data::send_one_at(0, 4))
        .unwrap();
    assert_eq!(reassembler.len(), 8);
}

#[test]
fn write_data_reader_does_not_interpose_when_reassembler_has_head_data() {
    let mut reassembler = Reassembler::new();
    let mut reader = Data::new(8);
    let mut app_buf: Vec<u8> = Vec::new();

    reassembler
        .write_at(0u32.into(), &Data::send_one_at(0, 4))
        .unwrap();
    reader.seek_forward(4);

    write_data_reader(&mut reassembler, &mut reader, &mut app_buf).unwrap();

    // The interposer bypass is skipped because the reassembler already holds
    // data at the head (offset 0-3).  Both head and tail (reader, offset 4-7)
    // are stored in the reassembler; all 8 bytes are contiguous so they are
    // immediately accessible without a gap.
    assert!(app_buf.is_empty());
    assert_eq!(reassembler.len(), 8);
    assert_eq!(reassembler.total_received_len(), 8);
    assert!(!reassembler.is_empty());
}

// ─── Bach async tests ─────────────────────────────────────────────────────────
//
// Each test uses two *primary* tasks:
//   • endpoint task – owns Pusher; sends stream messages and asserts on frames.
//   • app task      – owns Reader; calls read_into and asserts on received data.
//
// Both tasks are marked `.primary()` so the Bach sim runs until *both* complete,
// providing backpressure-free cooperative scheduling between the two sides.

/// Basic in-order read: endpoint sends data + FIN, application reads until EOF.
#[test]
fn basic_read() {
    sim(|| {
        let (mut reader, mut pusher) = make_pair();

        // Endpoint task: push data + FIN then exit.
        async move {
            pusher.push_data(0, b"hello world", true);
        }
        .primary()
        .spawn();

        // App task: read until EOF.
        async move {
            let mut buf = BytesMut::with_capacity(32);
            let outcome = reader.read_to_end(&mut buf).await.expect("read failed");
            assert_eq!(outcome, ReadToEnd::Complete(11));
            assert_eq!(&buf[..], b"hello world");
            assert!(reader.0.status.is_complete());
        }
        .primary()
        .spawn();
    });
}

/// In-order read counts bytes correctly and marks the stream complete.
///
/// Mirrors `poll_read_into_counts_direct_interposer_writes` but uses the
/// proper two-task async harness instead of a noop waker.
#[test]
fn in_order_read_reports_byte_count_and_completes() {
    sim(|| {
        let (mut reader, mut pusher) = make_pair();
        let expected = Data::send_one_at(0, 8);

        async move {
            pusher.push_data(0, &expected, true);
        }
        .primary()
        .spawn();

        async move {
            let mut out = Vec::new();
            let n = reader.read_into(&mut out).await.expect("read failed");
            assert_eq!(n, 8);
            assert_eq!(out, Data::send_one_at(0, 8));
            assert!(reader.0.status.is_complete());
        }
        .primary()
        .spawn();
    });
}

/// Repeated post-EOF reads should trip a debug assertion so applications do not
/// accidentally spin on clean completion forever.
#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "Reader returned EOF again on stream 1")]
fn repeated_post_eof_reads_panic_in_debug() {
    sim(|| {
        let (mut reader, mut pusher) = make_pair();

        async move {
            pusher.push_data(0, b"ok", true);
        }
        .primary()
        .spawn();

        async move {
            let mut buf = BytesMut::with_capacity(16);

            let n = reader.read_into(&mut buf).await.expect("read failed");
            assert_eq!(n, 2);
            assert_eq!(&buf[..], b"ok");

            let eof = reader.read_into(&mut buf).await.expect("read failed");
            assert_eq!(eof, 0);

            let _ = reader.read_into(&mut buf).await;
        }
        .primary()
        .spawn();
    });
}

/// Out-of-order delivery: endpoint pushes tail then head; app reads complete
/// data after reassembly.  Both tasks are primaries so neither holds the other
/// open artificially.
#[test]
fn out_of_order_reassembly() {
    sim(|| {
        let (mut reader, mut pusher) = make_pair();

        // Endpoint task: push tail first so the app must wait for the head.
        async move {
            pusher.push_data(5, b"world", true); // tail: out of order
            bach::task::yield_now().await; // yield so app can process the tail
            pusher.push_data(0, b"hello", false); // head: fills the gap
        }
        .primary()
        .spawn();

        // App task: read until EOF.
        async move {
            let mut buf = BytesMut::with_capacity(32);
            let outcome = reader.read_to_end(&mut buf).await.expect("read failed");
            assert_eq!(outcome, ReadToEnd::Complete(10));
            assert_eq!(&buf[..], b"helloworld");
        }
        .primary()
        .spawn();
    });
}

/// Per-frame coop budget: a batch larger than `BUDGET` frames must drain across multiple polls,
/// in order, with the unprocessed remainder stashed in `pending_rx` between polls. Each frame is a
/// distinct 1-byte payload at a contiguous offset so reassembly order is observable, and the total
/// exceeds `BUDGET` so at least one budget break occurs.
#[test]
fn pending_rx_drains_across_polls_in_order() {
    // >BUDGET single-byte frames emit hundreds of repetitive per-frame trace lines; snapshotting
    // that is unreasonably large and adds no regression signal beyond the explicit assertions.
    let _guard = crate::testing::without_snapshots();
    sim(|| {
        let (mut reader, mut pusher) = make_pair();
        let budget = crate::stream::coop::BUDGET as u64;
        // One more than two full budgets, so we span at least three polls.
        let total = budget * 2 + 1;

        async move {
            // Push every frame up front as a single backlog, then FIN. A single `poll_swap` will
            // hand the reader the whole batch; the per-frame budget forces it to stash the tail.
            for offset in 0..total {
                pusher.push_data(offset, b"x", false);
            }
            pusher.push_data(total, b"", true); // FIN at the end
        }
        .primary()
        .spawn();

        async move {
            let mut buf = BytesMut::with_capacity((total + 1) as usize);
            let outcome = reader.read_to_end(&mut buf).await.expect("read failed");
            assert_eq!(outcome, ReadToEnd::Complete(total as usize));
            // Every byte arrived, in order, exactly once.
            assert_eq!(buf.len(), total as usize);
            assert!(buf.iter().all(|&b| b == b'x'));
            // The backlog fully drained: nothing left stashed.
            assert!(reader.0.pending_rx.is_empty());
        }
        .primary()
        .spawn();
    });
}

/// A reset stashed behind the budget boundary (i.e. left in `pending_rx` after a budget break)
/// must still be observed: the data ahead of it is delivered first, then the reset surfaces on a
/// later poll. This exercises the drain-`pending_rx`-before-`poll_swap` path.
#[test]
fn stashed_reset_after_budget_is_observed() {
    // >BUDGET frames → unreasonably large snapshot; rely on the explicit assertions instead.
    let _guard = crate::testing::without_snapshots();
    sim(|| {
        let (mut reader, mut pusher) = make_pair();
        let budget = crate::stream::coop::BUDGET as u64;
        // Enough data to overflow one budget, followed by a reset that lands behind the boundary.
        let total = budget + 10;

        async move {
            for offset in 0..total {
                pusher.push_data(offset, b"y", false);
            }
            pusher.push_reset(VarInt::from_u8(7));
        }
        .primary()
        .spawn();

        async move {
            let mut buf = BytesMut::with_capacity((total + 1) as usize);
            loop {
                match reader.read_into(&mut buf).await {
                    Ok(0) => panic!("unexpected clean EOF, expected reset"),
                    // A read that drains buffered data in the same poll the Reset is processed
                    // returns the data via `buf` and then surfaces the error on that same call
                    // (the reassembler is empty so the error is not deferred), so count delivered
                    // bytes from `buf`, not the `Ok` returns.
                    Ok(_) => {}
                    Err(e) => {
                        assert_eq!(e.kind(), std::io::ErrorKind::ConnectionReset);
                        break;
                    }
                }
            }
            // All data ahead of the reset landed in the buffer before the reset surfaced.
            assert_eq!(buf.len(), total as usize);
            assert!(buf.iter().all(|&b| b == b'y'));
            assert!(reader.0.status.is_reset());
        }
        .primary()
        .spawn();
    });
}

/// Regression: a sub-budget `pending_rx` drain that makes no contiguous progress must still
/// (re)register the polling task's waker on the stream half, or a later data arrival is lost.
///
/// The lost wakeup, step by step (see `poll_stream_rx`):
///   1. The endpoint delivers `BUDGET + k` (k < BUDGET) stream frames in one batch, ALL at offsets
///      past the read cursor (a gap at offset 0 — the out-of-order / loss regime). The first poll
///      (waker A) drains `BUDGET` frames into the reassembler behind the gap (0 bytes deliverable),
///      breaks on the coop budget, and stashes the remaining `k` frames in `pending_rx`. That first
///      poll went through `poll_swap`, so the stream-half slot holds waker A.
///   2. The task migrates and re-polls with a DIFFERENT waker B (normal under select!/timeout/
///      FuturesUnordered/work-stealing — `Future::poll` does not promise a stable waker). Because
///      `pending_rx` is non-empty, `poll_stream_rx` takes the stash and SKIPS `poll_swap` entirely,
///      so the slot is never updated to B. The `k` stashed frames still don't fill the gap, so the
///      drain delivers 0 bytes and returns `Pending` as a *natural wait* (budget not exhausted) —
///      no coop self-wake either.
///   3. The endpoint now sends offset 0 (fills the gap → data is deliverable). `push_stream` wakes
///      whatever the slot holds: waker A, the stale one. Waker B — the live task — is never woken,
///      so the read hangs forever despite fully-deliverable data sitting in the reassembler.
///
/// This polls the reader manually with two distinct counting wakers to model the task migration.
/// It asserts B (the waker live at the time the gap is filled) is woken; on the current code only A
/// is, so the assertion fails — proving the lost wakeup.
#[test]
fn pending_rx_subbudget_drain_loses_wakeup_on_task_migration() {
    use core::{
        sync::atomic::{AtomicUsize, Ordering},
        task::{Context, Poll, RawWaker, RawWakerVTable, Waker},
    };

    // A waker that records how many times it was woken.
    struct CountingWaker(AtomicUsize);
    fn counting_waker(inner: Arc<CountingWaker>) -> Waker {
        unsafe fn clone(p: *const ()) -> RawWaker {
            let arc = Arc::from_raw(p as *const CountingWaker);
            let cloned = arc.clone();
            let _ = Arc::into_raw(arc);
            RawWaker::new(Arc::into_raw(cloned) as *const (), &VTABLE)
        }
        unsafe fn wake(p: *const ()) {
            let arc = Arc::from_raw(p as *const CountingWaker);
            arc.0.fetch_add(1, Ordering::SeqCst);
        }
        unsafe fn wake_by_ref(p: *const ()) {
            let arc = Arc::from_raw(p as *const CountingWaker);
            arc.0.fetch_add(1, Ordering::SeqCst);
            let _ = Arc::into_raw(arc);
        }
        unsafe fn drop_fn(p: *const ()) {
            let _ = Arc::from_raw(p as *const CountingWaker);
        }
        static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, wake, wake_by_ref, drop_fn);
        let raw = RawWaker::new(Arc::into_raw(inner) as *const (), &VTABLE);
        unsafe { Waker::from_raw(raw) }
    }

    // >BUDGET frames → huge snapshot; rely on explicit assertions.
    let _guard = crate::testing::without_snapshots();
    sim(|| {
        let (mut reader, mut pusher) = make_pair();
        let budget = crate::stream::coop::BUDGET as u64;
        // BUDGET + 8 frames, ALL at offset >= 1 so there is a gap at offset 0 and nothing is
        // deliverable yet. After the first poll drains BUDGET and stashes 8, the remainder is
        // sub-budget — the path that skips `poll_swap`.
        let gap_frames = budget + 8;

        async move {
            for offset in 1..=gap_frames {
                pusher.push_data(offset, b"x", false);
            }

            // Give the app task a moment to perform its two manual polls and park.
            bach::time::sleep(Duration::from_millis(10)).await;

            // Fill the gap. `push_stream` wakes whatever waker the slot holds.
            pusher.push_data(0, b"x", false);
        }
        .primary()
        .spawn();

        async move {
            let a = Arc::new(CountingWaker(AtomicUsize::new(0)));
            let b = Arc::new(CountingWaker(AtomicUsize::new(0)));
            let waker_a = counting_waker(a.clone());
            let waker_b = counting_waker(b.clone());

            // Let the endpoint enqueue the full gap backlog first.
            bach::time::sleep(Duration::from_millis(1)).await;

            let mut buf = BytesMut::with_capacity((gap_frames + 2) as usize);

            // Poll #1 with waker A: drains BUDGET frames behind the gap, stashes the 8-frame
            // remainder in pending_rx. Goes through poll_swap, so the slot holds waker A.
            let p1 = reader.poll_read_into(&mut Context::from_waker(&waker_a), &mut buf);
            assert!(matches!(p1, Poll::Pending), "no contiguous data yet");
            assert!(
                !reader.0.pending_rx.is_empty(),
                "sub-budget remainder must be stashed for the migration path"
            );

            // Poll #2 with waker B (the task migrated): drains the sub-budget pending_rx remainder,
            // skipping poll_swap, so the slot is NOT updated to B. Still a gap → Pending.
            let p2 = reader.poll_read_into(&mut Context::from_waker(&waker_b), &mut buf);
            assert!(matches!(p2, Poll::Pending), "still blocked on the gap at offset 0");

            // The endpoint fills the gap (after its sleep). Wait for that to land.
            bach::time::sleep(Duration::from_millis(50)).await;

            // The live waker (B, used on the most recent poll) MUST have been woken. On the buggy
            // code the slot still holds the stale waker A, so B is never told and the real task
            // would hang forever despite deliverable data.
            assert_eq!(
                b.0.load(Ordering::SeqCst),
                1,
                "BUG: lost wakeup — the gap-filling arrival woke the stale waker A \
                 (a.woken={}), not the live waker B. A migrated reader task hangs forever \
                 with fully-deliverable data buffered.",
                a.0.load(Ordering::SeqCst)
            );

            // And a re-poll with B now delivers all the buffered bytes.
            match reader.poll_read_into(&mut Context::from_waker(&waker_b), &mut buf) {
                Poll::Ready(Ok(n)) => assert!(n > 0, "data must be deliverable once the gap is filled"),
                other => panic!("expected data after gap fill, got {other:?}"),
            }
        }
        .primary()
        .spawn();
    });
}

/// A reset terminates a read with `ConnectionReset`.
#[test]
fn reset_terminates_read() {
    sim(|| {
        let (mut reader, mut pusher) = make_pair();

        async move {
            pusher.push_reset(VarInt::from_u8(42));
        }
        .primary()
        .spawn();

        async move {
            let mut buf = BytesMut::with_capacity(32);
            let err = reader
                .read_into(&mut buf)
                .await
                .expect_err("expected reset error");
            assert_eq!(err.kind(), std::io::ErrorKind::ConnectionReset);
            assert!(reader.0.status.is_reset());
            // Reassembler should be cleared on reset to free memory.
            assert!(reader.0.reassembler.is_empty());
        }
        .primary()
        .spawn();
    });
}

/// Data arrives then a reset: the stream must eventually surface the reset.
#[test]
fn reset_after_partial_data() {
    sim(|| {
        let (mut reader, mut pusher) = make_pair();

        async move {
            pusher.push_data(0, b"partial", false);
            pusher.push_reset(VarInt::from_u8(1));
        }
        .primary()
        .spawn();

        async move {
            let mut buf = BytesMut::with_capacity(64);
            // Read until we hit the reset error.
            loop {
                match reader.read_into(&mut buf).await {
                    Ok(0) => panic!("unexpected clean EOF, expected reset"),
                    Ok(_) => {}
                    Err(e) => {
                        assert_eq!(e.kind(), std::io::ErrorKind::ConnectionReset);
                        break;
                    }
                }
            }
            // The "partial" data was delivered by the interposer before the
            // Reset message was processed in the same queue batch.  TCP has
            // the same semantics: data already in the receive buffer when a
            // RST arrives may have been copied to user-space.
            assert_eq!(&buf[..], b"partial");
            assert!(reader.0.status.is_reset());
            assert!(reader.0.reassembler.is_empty());
            // Subsequent reads after a reset must return ConnectionReset,
            // not BrokenPipe or some other error.
            let err2 = reader
                .read_into(&mut buf)
                .await
                .expect_err("expected ConnectionReset on re-read");
            assert_eq!(err2.kind(), std::io::ErrorKind::ConnectionReset);
        }
        .primary()
        .spawn();
    });
}

/// Reset before data in the same queue batch: reset wins and late data is not
/// delivered to the application.
#[test]
fn reset_before_data_in_same_batch_discards_data() {
    sim(|| {
        let (mut reader, mut pusher) = make_pair();

        async move {
            // Push reset first, then data in the same queue batch.
            pusher.push_reset(VarInt::from_u8(7));
            pusher.push_data(0, b"late", true);
        }
        .primary()
        .spawn();

        async move {
            let mut buf = BytesMut::with_capacity(16);
            let err = reader
                .read_into(&mut buf)
                .await
                .expect_err("expected ConnectionReset");
            assert_eq!(err.kind(), std::io::ErrorKind::ConnectionReset);
            assert!(buf.is_empty(), "data after reset should not be delivered");
            assert!(reader.0.reassembler.is_empty());

            let err2 = reader
                .read_into(&mut buf)
                .await
                .expect_err("expected sticky ConnectionReset");
            assert_eq!(err2.kind(), std::io::ErrorKind::ConnectionReset);
        }
        .primary()
        .spawn();
    });
}

/// If the application reads one byte at a time (yielding between reads), data
/// buffered before a reset is drained before surfacing ConnectionReset.
#[test]
fn reset_after_partial_data_byte_at_a_time_drains_before_error() {
    sim(|| {
        let (mut reader, mut pusher) = make_pair();
        let expected = b"partial";

        async move {
            pusher.push_data(0, expected, false);
            pusher.push_reset(VarInt::from_u8(9));
        }
        .primary()
        .spawn();

        async move {
            let mut buf = BytesMut::with_capacity(64);
            loop {
                // Model an app that reads in tiny chunks and yields.
                bach::task::yield_now().await;

                let result = {
                    let mut limited = buf.with_write_limit(1);
                    reader.read_into(&mut limited).await
                };

                match result {
                    Ok(0) => panic!("unexpected clean EOF, expected reset"),
                    Ok(n) => assert_eq!(n, 1, "expected one-byte reads"),
                    Err(e) => {
                        assert_eq!(e.kind(), std::io::ErrorKind::ConnectionReset);
                        break;
                    }
                }
            }

            assert_eq!(&buf[..], expected);
            assert!(reader.0.reassembler.is_empty());

            let err2 = {
                let mut limited = buf.with_write_limit(1);
                reader
                    .read_into(&mut limited)
                    .await
                    .expect_err("expected sticky ConnectionReset")
            };
            assert_eq!(err2.kind(), std::io::ErrorKind::ConnectionReset);
        }
        .primary()
        .spawn();
    });
}

/// The Reader must emit a `MAX_DATA` (QueueControl) frame after the application
/// consumes enough bytes to cross the replenishment threshold (> window / 2).
///
/// The endpoint task waits for the MAX_DATA frame asynchronously — mirroring
/// how a real endpoint would receive and process such frames from the app side.
#[test]
fn max_data_sent_after_consuming() {
    sim(|| {
        let (mut reader, mut pusher) = make_pair();
        let window_size = reader.0.window_size;
        // Cross the > window/2 threshold in a single read without exceeding the
        // advertised receive window.
        let payload = vec![0xabu8; (window_size / 2 + 1) as usize];
        let payload_len = payload.len();
        let expected_max_data = VarInt::new(window_size + payload_len as u64).unwrap();

        // The writer signals it wants to send well beyond this frame (a full window past what we
        // consume), so the reader extends the window to `consumed + window_size`.
        let hint = window_size + payload_len as u64;

        // Endpoint task: push data, then wait for the MAX_DATA frame.
        async move {
            pusher.push_data_hint(0, &payload, false, hint);
            let frames = pusher.recv_frames().await;
            assert_eq!(frames.len(), 1, "expected exactly one outbound frame");
            assert_eq!(
                frames.front().and_then(decode_max_data_from_queue_control),
                Some(expected_max_data),
                "expected exactly one MAX_DATA frame with the computed limit"
            );
        }
        .primary()
        .spawn();

        // App task: read once.
        async move {
            let mut buf = BytesMut::with_capacity(payload_len + 16);
            let read = reader.read_into(&mut buf).await.expect("read failed");
            assert_eq!(read, payload_len);
            assert_eq!(buf.len(), payload_len);
            // Keep the task alive long enough for the endpoint-side assertion to
            // consume this batch before Reader is dropped at task completion.
            1.s().sleep().await;
        }
        .primary()
        .spawn();
    });
}

/// Recv-credit conservation across a full stream lifecycle.
///
/// With no parked waiters the pool holds the invariant `available + returned == capacity`: every
/// byte a reader acquires by extending its advertised window must eventually be returned, either as
/// inbound data arrives (`release` on the dispatch side) or as the unfilled tail of the window is
/// reclaimed when the stream terminates.
///
/// This reproduces the leak: a reader extends its window past what the peer actually sends, then
/// the stream completes and the reader drops. The advertised-but-unfilled window
/// (`remote_max_data - max(initial_window, max_received_offset)`) is acquired from the pool but
/// never released, so `available + returned` ends up short by exactly that gap.
///
/// The harness seeds the slot's unbacked initial window to the reader's `window_size` (matching
/// production in `path/secret/map/entry.rs`, where both come from `local_recv_max_data`) and mirrors
/// the dispatch release path via `Pusher::push`, so the books balance exactly when there is no leak.
#[test]
fn recv_credit_conserved_across_stream_lifecycle() {
    sim(|| {
        // Pool large enough that the window extension succeeds on the fast path (no parking), so
        // `available` directly reflects acquires and `returned` directly reflects releases.
        let capacity = 8 * 1024 * 1024;
        let pool = crate::sync::Arc::new(crate::credit::Pool::new(
            crate::credit::Config::new(capacity).with_max_single_acquire_uniform(capacity),
        ));
        let assert_pool = pool.clone();

        // Seed the slot's unbacked initial window to the reader's window so the client's unbacked
        // starting window and the suppressed initial release cancel — exactly as in production.
        let window_size = 1024 * 1024;
        let (mut reader, mut pusher) =
            make_pair_with_pool_and_initial_window(pool, window_size as u64);
        assert_eq!(reader.0.window_size, window_size as u64);

        // Peer sends a fraction of the window, but hints it wants to send far more — forcing the
        // reader to acquire a window extension it will never fill.
        let body = vec![0xabu8; 600_000];
        let body_len = body.len();
        let hint = 2 * 1024 * 1024; // writer wants well beyond the standing window
        let tail_len = 8usize;
        let total_len = body_len + tail_len;

        async move {
            pusher.push_data_hint(0, &body, false, hint);
            // Let the app consume `body` and extend the window (the acquire happens here).
            for _ in 0..4 {
                bach::task::yield_now().await;
            }
            // FIN at a low offset: the peer never comes close to filling the extended window.
            pusher.push_data(body_len as u64, &vec![0xcdu8; tail_len], true);
            1.s().sleep().await;
        }
        .primary()
        .spawn();

        async move {
            let mut buf = BytesMut::with_capacity(total_len + 16);
            let outcome = reader.read_to_end(&mut buf).await.expect("read failed");
            assert_eq!(outcome, ReadToEnd::Complete(total_len));
            assert!(reader.0.status.is_complete());
            // The reader advertised well past what arrived; confirm the gap exists before drop.
            assert!(
                reader.0.remote_max_data.as_u64() > window_size as u64,
                "test setup: reader should have extended its window"
            );
            // Drop the reader: its terminal path must release the unfilled window back to the pool.
            drop(reader);
            // Drop is synchronous, but yield so any wake bookkeeping settles before we assert.
            bach::task::yield_now().await;

            let available = assert_pool.debug_available();
            let returned = assert_pool.debug_returned();
            assert_eq!(
                available + returned as i64,
                capacity as i64,
                "recv-credit leak: available({available}) + returned({returned}) != capacity({capacity}); \
                 the advertised-but-unfilled window was acquired but never released on termination"
            );
        }
        .primary()
        .spawn();
    });
}

/// Regression (review finding H1): when a server reader's first/confirming MAX_DATA fails to send,
/// the credit it "returns" to the pool must not include the *unbacked* initial window, which was
/// never acquired from the pool.
///
/// A server reader starts with `remote_max_data = 0` and `unbacked_remaining = window_size`. Its
/// first consumption funds the confirming MAX_DATA extension entirely from `unbacked_remaining`
/// (no pool draw). If `send_max_data_frame` then fails (frame channel closed during teardown),
/// `maybe_send_max_data`'s error path runs `recv_credit_pool.release(granted)` — but `granted`
/// here is pure unbacked credit. Releasing it injects phantom credit into the shared pool,
/// permanently inflating it above `capacity` and breaking conservation (`available + returned`
/// must equal `capacity` with no parked waiters), which defeats receive-side backpressure for
/// every stream sharing the pool.
///
/// Pre-fix: `available + returned` exceeds `capacity` by the unbacked amount. The fix releases
/// only the pool-backed portion (`granted - from_unbacked`) and restores `unbacked_remaining`.
#[test]
fn max_data_send_failure_does_not_release_unbacked_window() {
    sim(|| {
        // Full, untouched pool: any genuine acquire takes the fast path. We assert the pool is
        // never inflated above this capacity by the failed-send path.
        let capacity = 8 * 1024 * 1024u64;
        let pool = crate::sync::Arc::new(crate::credit::Pool::new(
            crate::credit::Config::new(capacity).with_max_single_acquire_uniform(capacity),
        ));
        let assert_pool = pool.clone();

        // Server reader: remote_max_data = 0, unbacked_remaining = window_size.
        let window_size = 1024 * 1024u64;
        let (mut reader, pusher) = make_server_pair_with_pool_and_initial_window(pool, window_size);
        assert_eq!(reader.0.window_size, window_size);
        assert_eq!(reader.0.remote_max_data.as_u64(), 0);

        // Replace the frame receiver with a fresh disconnected one so the reader's `frame_tx`
        // returns BrokenPipe when it tries to send the confirming MAX_DATA — modeling endpoint /
        // submission-task teardown while the stream future is still polled.
        let Pusher {
            dispatcher,
            queue_id,
            binding_id,
            frame_rx: _closed,
            frame_storage,
            recv_credit_pool,
        } = pusher;
        let mut pusher = Pusher {
            dispatcher,
            queue_id,
            binding_id,
            frame_rx: frame::submission_channel(1).1,
            frame_storage,
            recv_credit_pool,
        };

        async move {
            // Small amount of data, well within the unbacked initial window: the reader's first
            // consume funds the confirming MAX_DATA entirely from `unbacked_remaining`.
            pusher.push_data(0, b"hello server", false);
            1.s().sleep().await;
        }
        .primary()
        .spawn();

        async move {
            let mut buf = BytesMut::with_capacity(64);
            // The confirming MAX_DATA send fails on the closed channel → BrokenPipe surfaces.
            let err = reader
                .read_into(&mut buf)
                .await
                .expect_err("expected BrokenPipe when frame channel is closed");
            assert_eq!(err.kind(), std::io::ErrorKind::BrokenPipe);
            drop(reader);
            bach::task::yield_now().await;

            let available = assert_pool.debug_available();
            let returned = assert_pool.debug_returned();
            assert_eq!(
                available + returned as i64,
                capacity as i64,
                "pool inflated by phantom credit: available({available}) + returned({returned}) \
                 != capacity({capacity}); the failed MAX_DATA send released the unbacked initial \
                 window, which was never acquired from the pool",
            );
        }
        .primary()
        .spawn();
    });
}

/// Regression (review finding M1), integration: the dispatch-side per-arrival credit release must
/// be clamped to the reader's advertised window. A server reader bootstraps with
/// `remote_max_data = 0`, so the hard receive-window check is skipped (the bootstrap special-case)
/// and inbound data is accepted before the first MAX_DATA. The reader publishes its unbacked
/// initial window (`window_size`) as the dispatch ceiling. If a peer overshoots *that* window, the
/// dispatch path (`push_stream` -> `observe_offset` -> `recv_credit_pool.release`) would, without
/// the clamp, release credit for the overshoot — credit the reader never acquired — inflating the
/// shared pool above `capacity`.
///
/// The `advertised_window` ceiling caps the release at the window the reader actually advertised,
/// so the pool is never inflated. Pre-fix: `available + returned` exceeds `capacity` by the
/// overshoot beyond `window_size`.
#[test]
fn dispatch_release_clamped_to_advertised_window_on_bootstrap_overshoot() {
    sim(|| {
        let capacity = 8 * 1024 * 1024u64;
        let pool = crate::sync::Arc::new(crate::credit::Pool::new(
            crate::credit::Config::new(capacity).with_max_single_acquire_uniform(capacity),
        ));
        let assert_pool = pool.clone();

        // Server reader: `remote_max_data` starts at 0 (receive-window enforcement skipped), and the
        // unbacked initial window equals `window_size` — the production invariant. The reader
        // publishes that window as the dispatch ceiling at construction.
        let window_size = 1024 * 1024u64;
        let (reader, mut pusher) = make_server_pair_with_pool_and_initial_window(pool, window_size);
        assert_eq!(reader.0.remote_max_data.as_u64(), 0);
        assert_eq!(reader.0.window_size, window_size);

        async move {
            // Peer overshoots the reader's advertised window before any MAX_DATA growth. `Pusher`
            // mirrors dispatch: it releases `observe_offset`'s `release_bytes` back to the pool.
            // Only the in-window portion is unbacked (releases 0); the overshoot must release
            // nothing rather than injecting credit the reader never acquired.
            let overshoot = (window_size + 100_000) as usize;
            pusher.push_data(0, &vec![0xabu8; overshoot], false);
            bach::task::yield_now().await;

            let available = assert_pool.debug_available();
            let returned = assert_pool.debug_returned();
            assert_eq!(
                available + returned as i64,
                capacity as i64,
                "pool inflated by phantom credit: available({available}) + returned({returned}) \
                 != capacity({capacity}); the dispatch release was not clamped to the advertised \
                 window, releasing credit for bytes beyond what the reader acquired",
            );
        }
        .primary()
        .spawn();

        // Keep the reader alive for the duration so its slot (and the published ceiling) stays
        // valid while the pusher injects and asserts.
        async move {
            let _reader = reader;
            1.s().sleep().await;
        }
        .primary()
        .spawn();
    });
}

/// Right-sizing: a client reader bootstraps with a full `window_size` already advertised. When the
/// writer's hint says it wants to send less than that standing window, consuming past the top-up
/// threshold must NOT emit a MAX_DATA — there is no point advertising beyond what the writer wants.
/// (Contrast `max_data_sent_after_consuming`, where the hint asks for a full window ahead.)
#[test]
fn bounded_hint_does_not_over_advertise() {
    sim(|| {
        let (mut reader, mut pusher) = make_pair();
        let window_size = reader.0.window_size;
        // Cross the > window/2 threshold, but the writer only wants a little past what we consume.
        let payload = vec![0xabu8; (window_size / 2 + 1) as usize];
        let payload_len = payload.len();
        let hint = payload_len as u64 + 8;

        async move {
            pusher.push_data_hint(0, &payload, false, hint);
            // No MAX_DATA should be emitted: the writer's desired offset is already below the
            // standing advertised window.
            let frames = pusher.recv_frames_timeout(Duration::from_millis(100)).await;
            assert!(
                frames.is_none(),
                "expected no MAX_DATA when the writer wants less than the standing window, got {:?}",
                frames.map(|q| q.iter().map(|f| f.header).collect::<Vec<_>>())
            );
        }
        .primary()
        .spawn();

        async move {
            let mut buf = BytesMut::with_capacity(payload_len + 16);
            let read = reader.read_into(&mut buf).await.expect("read failed");
            assert_eq!(read, payload_len);
            assert_eq!(
                reader.0.growth_ratio, 1,
                "growth ratio must not change without a blocked signal"
            );
            1.s().sleep().await;
        }
        .primary()
        .spawn();
    });
}

/// A blocked signal whose desired offset outstrips the current cap doubles the growth ratio
/// (slow-start); a blocked signal within the current cap, or a duplicate, is a no-op.
#[test]
fn blocked_signal_doubles_growth_ratio_and_dedups() {
    sim(|| {
        // The growth ratio is capped at `max_single_acquire / window_size`, so the pool's
        // per-request ceiling must comfortably exceed the reader's window for doubling to occur.
        // Build the reader first to learn `window_size`, then size the pool around it.
        let probe = make_pair().0;
        let window_size = probe.0.window_size;
        drop(probe);
        let cap = window_size.saturating_mul(64).max(1024 * 1024);
        let pool = crate::sync::Arc::new(crate::credit::Pool::new(
            crate::credit::Config::new(cap).with_max_single_acquire_uniform(cap),
        ));
        let (mut reader, mut pusher) = make_pair_with_pool(pool);
        let payload = vec![0xcdu8; 64];
        let payload_len = payload.len();
        // Desired offset well beyond consumed + window so the `> cap` gate fires once.
        let desired = window_size.saturating_mul(4);

        async move {
            pusher.push_data(0, &payload, false);
            pusher.push_blocked(desired);
            // Duplicate at the same offset → deduped, no further growth.
            pusher.push_blocked(desired);
            1.s().sleep().await;
        }
        .primary()
        .spawn();

        async move {
            let mut buf = BytesMut::with_capacity(payload_len + 16);
            let _ = reader.read_into(&mut buf).await.expect("read failed");
            for _ in 0..4 {
                bach::task::yield_now().await;
            }
            // One distinct over-cap blocked signal → exactly one doubling (1 → 2). The duplicate is
            // deduped by the `desired > cap`/`acted_blocked_offset` gate.
            assert_eq!(
                reader.0.growth_ratio, 2,
                "expected exactly one doubling from a single distinct over-cap blocked signal"
            );
            1.s().sleep().await;
        }
        .primary()
        .spawn();
    });
}

/// A *synthetic* blocked signal (receiver-generated for an oversized QueueMsg segment) opens the
/// window straight to the known demand offset, bypassing the streaming slow-start ramp. This is the
/// distinction from [`blocked_signal_doubles_growth_ratio_and_dedups`]: a real peer signal ramps
/// `growth_ratio`; a synthetic one targets a fixed, bounded offset because the demand is known.
#[test]
fn synthetic_blocked_opens_window_to_known_demand_without_growth() {
    sim(|| {
        // Pool ceiling well above the demand so a single extension could in principle cover it; the
        // point under test is that the *target* is the known demand, not a slow-start multiple.
        let probe = make_pair().0;
        let window_size = probe.0.window_size;
        drop(probe);
        let cap = window_size.saturating_mul(64).max(1024 * 1024);
        let pool = crate::sync::Arc::new(crate::credit::Pool::new(
            crate::credit::Config::new(cap).with_max_single_acquire_uniform(cap),
        ));
        // Hold a distributor so acquires are granted rather than parking.
        let distributor = crate::credit::Distributor::new(pool.clone());
        let (mut reader, mut pusher) = make_pair_with_pool(pool);

        // A known message demand of 4× the bootstrap window — past the initial slow-start cap (1×).
        let demand = window_size.saturating_mul(4);

        let payload = vec![0xabu8; 64];
        let payload_len = payload.len();

        async move {
            // A little real data so the reader's poll makes progress (returns Ready), plus the
            // synthetic blocked carrying the known demand.
            pusher.push_data(0, &payload, false);
            pusher.push_synthetic_blocked(demand);
            let _keep_alive = &distributor;
            1.s().sleep().await;
        }
        .primary()
        .spawn();

        async move {
            let mut buf = BytesMut::with_capacity(payload_len + 16);
            let _ = reader.read_into(&mut buf).await.expect("read failed");
            for _ in 0..8 {
                bach::task::yield_now().await;
            }
            // The advertised window reached the known demand, even though `growth_ratio` never moved
            // off its initial 1× (the synthetic path does not ramp slow-start).
            assert_eq!(
                reader.0.growth_ratio, 1,
                "synthetic blocked must not drive the streaming growth ramp"
            );
            assert!(
                reader.0.remote_max_data.as_u64() >= demand,
                "window must open to the known message demand: advertised={}, demand={demand}",
                reader.0.remote_max_data.as_u64(),
            );
            1.s().sleep().await;
        }
        .primary()
        .spawn();
    });
}

/// Repro for the bulk-streaming throughput collapse observed in dc-tester (xlarge-request fell to
/// ~3.5 Gbps once reader flow control was wired up).
///
/// Model: a writer is continuously backlogged (every data frame carries the `blocked` bit with a
/// `peer_max_offset` far past what has been advertised), and the application drains in MTU-sized
/// chunks. This is exactly the steady state of a saturated bulk transfer. To keep the pipe full the
/// reader must open its advertised window to roughly a bandwidth-delay product ahead of what the
/// application has consumed; if instead the window saturates at a small multiple of the initial
/// window, the sender's flow-control budget (`remote_max_data - next_offset`) is capped there and
/// the link runs at a fraction of capacity.
///
/// The window must track the writer's hinted demand directly, limited only by the recv pool — not
/// by the streaming slow-start ramp. The writer advertises a high-water mark (`peer_max_offset`)
/// far past what has been advertised; the reader pursues it across `max_single_acquire`-sized
/// acquires (parking and re-arming as the pool grants), so the advertised window leads consumption
/// by a BDP-class margin. `growth_ratio` adds headroom *beyond* the observed demand but does not cap
/// it — so even with a constant demand hint (which `growth_ratio`'s consume-paced dedup would pin at
/// a low multiple) the window still opens to that demand. The pool here mirrors production sizing
/// (16 MiB capacity, 8 MiB per-acquire) so the acquire-slicing path is exercised, not bypassed.
///
/// Assertion: after the application has consumed `TARGET` bytes from a perpetually-blocked writer,
/// the advertised window must lead `consumed` by at least a BDP-class margin (`min_lead`). A policy
/// that clamps the demand to a small multiple of the initial window (the original bug) fails this;
/// pursuing demand directly passes it.
#[test]
fn bulk_stream_opens_window_to_bdp() {
    sim(|| {
        // Pool sized so the per-request ceiling (`max_single_acquire`, 8 MiB) is a generous multiple
        // of the 1 MiB initial window, so the window can open well past a couple of initial windows.
        let pool = crate::sync::Arc::new(crate::credit::Pool::new(
            crate::credit::Config::new(16 * 1024 * 1024)
                .with_max_single_acquire_uniform(8 * 1024 * 1024),
        ));
        // Hold a distributor so pool acquires can actually be granted (otherwise growth parks).
        let distributor = crate::credit::Distributor::new(pool.clone());

        let (mut reader, mut pusher) = make_pair_with_pool(pool);
        let window_size = reader.0.window_size;

        // Drain ~8 MiB in MTU-sized chunks. A healthy window should be several MiB wide by the end.
        const CHUNK: usize = 8 * 1024;
        const TARGET: usize = 8 * 1024 * 1024;
        // The writer always wants to send far past the advertised edge.
        const FAR_AHEAD: u64 = 64 * 1024 * 1024;
        // Require the advertised window to lead consumption by >= 4x the initial window. The
        // demand-driven fix opens to the ~8 MiB pool ceiling; the buggy ramp saturates at ~2 MiB.
        let min_lead = window_size.saturating_mul(4);

        // Endpoint task: feed a perpetually-backlogged writer and drain its outbound MAX_DATA frames
        // so the submission channel never blocks.
        //
        // Faithful to a real bulk `write_from_fin`: the writer's high-watermark (`largest_offset`,
        // delivered here as `peer_max_offset`) is the *total* outstanding demand, which stays
        // essentially CONSTANT across the transfer (`next_offset + buffered_len` ≈ the fixed total).
        // This matters: the reader dedups blocked signals on `desired > acted_blocked_offset`, so a
        // constant demand only ever trips the growth-ratio doubling once. (An earlier version of
        // this test ramped `peer_max_offset` with `offset`, which masked the bug by re-tripping the
        // gate every frame.)
        let total_demand = TARGET as u64 + FAR_AHEAD;
        let writer = async move {
            let mut offset = 0u64;
            let payload = vec![0xa5u8; CHUNK];
            while offset < TARGET as u64 {
                pusher.push(msg::Stream::Data {
                    offset: VarInt::new(offset).unwrap(),
                    // Constant total demand, far past anything advertised — the writer wants to send
                    // the whole stream and says so on every frame.
                    peer_max_offset: VarInt::new(total_demand).unwrap(),
                    payload: BytesMut::from(&payload[..]),
                    fin: false,
                    blocked: true,
                });
                offset += CHUNK as u64;
                // Drain any MAX_DATA the reader emitted; don't block forever if none this turn.
                let _ = pusher.recv_frames_timeout(Duration::from_millis(1)).await;
                bach::task::yield_now().await;
            }
            let _keep_alive = &distributor;
            1.s().sleep().await;
        };

        // App task: consume in chunks, then assert the advertised window opened up.
        let app = async move {
            let mut consumed = 0usize;
            let mut buf = BytesMut::with_capacity(CHUNK);
            while consumed < TARGET {
                buf.clear();
                let n = reader.read_into(&mut buf).await.expect("read failed");
                if n == 0 {
                    bach::task::yield_now().await;
                    continue;
                }
                consumed += n;
            }

            let advertised = reader.0.remote_max_data.as_u64();
            let lead = advertised.saturating_sub(consumed as u64);
            assert!(
                lead >= min_lead,
                "advertised window collapsed to a dribble under sustained streaming: \
                 advertised={advertised}, consumed={consumed}, lead={lead} \
                 (need >= {min_lead}); a perpetually-blocked writer is being held to a ~1-window \
                 budget so the link can't stay full (growth_ratio={})",
                reader.0.growth_ratio,
            );
            1.s().sleep().await;
        };

        writer.primary().spawn();
        app.primary().spawn();
    });
}

#[test]
fn max_data_transmission_failure_surfaces_error() {
    sim(|| {
        let (mut reader, mut pusher) = make_pair();
        let window_size = reader.0.window_size;
        let payload = vec![0u8; (window_size / 2 + 1) as usize];
        let payload_len = payload.len();
        let hint = window_size + payload_len as u64;

        async move {
            pusher.push_data_hint(0, &payload, false, hint);

            let frames = pusher.recv_frames().await;
            pusher.complete_with_status(
                frames,
                frame::TransmissionStatus::Failed(frame::FailureReason::TransmissionError),
            );
        }
        .primary()
        .spawn();

        async move {
            let mut buf = BytesMut::with_capacity(payload_len + 16);

            let read = reader
                .read_into(&mut buf)
                .await
                .expect("first read should succeed");
            assert_eq!(read, payload_len);

            bach::task::yield_now().await;

            let err = reader
                .read_into(&mut buf)
                .await
                .expect_err("expected broken pipe from failed MAX_DATA transmission");
            assert_eq!(err.kind(), std::io::ErrorKind::BrokenPipe);
        }
        .primary()
        .spawn();
    });
}

/// If the peer sends beyond the client's advertised receive window, the Reader
/// errors and emits a QueueReset.
#[test]
fn queue_control_violation_errors_reader_and_sends_reset() {
    sim(|| {
        let (mut reader, mut pusher) = make_pair();
        let payload = vec![0u8; reader.0.window_size as usize + 1];
        let payload_len = payload.len();

        async move {
            pusher.push_data(0, &payload, false);
            let frames = pusher.recv_frames().await;
            assert_eq!(frames.len(), 1, "expected exactly one outbound frame");
            assert!(
                matches!(
                    frames.front().unwrap().header,
                    Header::QueueReset {
                        reset_target: ResetTarget::Both,
                        error_code,
                        ..
                    } if error_code == error::QUEUE_CONTROL_ERROR
                ),
                "expected exactly one QueueReset(Both, QUEUE_CONTROL_ERROR) frame"
            );
        }
        .primary()
        .spawn();

        async move {
            let mut buf = BytesMut::with_capacity(payload_len + 16);
            let err = reader
                .read_into(&mut buf)
                .await
                .expect_err("expected InvalidData on flow-control violation");
            assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
        }
        .primary()
        .spawn();
    });
}

/// Client-side FIN within the advertised window should not require sending
/// MAX_DATA after the final byte is consumed.
#[test]
fn client_fin_within_window_does_not_send_max_data() {
    sim(|| {
        let (mut reader, mut pusher) = make_pair();
        reader.0.window_size = 8;
        reader.0.remote_max_data = VarInt::from_u8(8);
        let payload = b"hello";

        async move {
            pusher.push_data(0, payload, true);
            let frames = pusher.recv_frames_timeout(1.s()).await;
            assert!(
                frames.is_none(),
                "client-side FIN crossing the threshold should not emit outbound frames"
            );
        }
        .primary()
        .spawn();

        async move {
            let mut buf = BytesMut::with_capacity(32);
            let outcome = reader.read_to_end(&mut buf).await.expect("read failed");
            assert_eq!(outcome, ReadToEnd::Complete(5));
            assert_eq!(&buf[..], payload);
        }
        .primary()
        .spawn();
    });
}

/// If FIN is observed on an out-of-order tail before the missing head arrives,
/// client readers still must not emit MAX_DATA after reassembly completes.
#[test]
fn client_fin_observed_before_gap_fill_does_not_send_max_data() {
    sim(|| {
        let (mut reader, mut pusher) = make_pair();
        reader.0.window_size = 8;
        reader.0.remote_max_data = VarInt::from_u8(8);

        async move {
            pusher.push_data(2, b"llo", true);
            // Sleep long enough to ensure the out-of-order FIN segment is
            // processed before the head segment is injected.
            1.s().sleep().await;
            pusher.push_data(0, b"he", false);
            let frames = pusher.recv_frames_timeout(1.s()).await;
            assert!(
                frames.is_none(),
                "client should suppress all outbound frames once FIN has been observed"
            );
        }
        .primary()
        .spawn();

        async move {
            let mut buf = BytesMut::with_capacity(32);
            let outcome = reader.read_to_end(&mut buf).await.expect("read failed");
            assert_eq!(outcome, ReadToEnd::Complete(5));
            assert_eq!(&buf[..], b"hello");
        }
        .primary()
        .spawn();
    });
}

/// Dropping the Reader before a FIN is received must send a `STOP_SENDING`
/// (QueueReset) frame so the peer knows to stop.
///
/// The endpoint task waits for the frame asynchronously, mirroring how a
/// real endpoint would process control frames from the application side.
#[test]
fn drop_before_fin_sends_stop_sending() {
    sim(|| {
        let (mut reader, mut pusher) = make_pair();

        // Endpoint task: push some data (no FIN), then wait for STOP_SENDING.
        async move {
            pusher.push_data(0, b"some data", false);
            let frames = pusher.recv_frames().await;
            assert_eq!(frames.len(), 1, "expected exactly one outbound frame");
            assert!(
                matches!(
                    frames.front().unwrap().header,
                    Header::QueueReset {
                        reset_target: ResetTarget::Control,
                        error_code,
                        ..
                    } if error_code == error::STOP_SENDING
                ),
                "expected exactly one QueueReset(Control, STOP_SENDING) on drop"
            );
        }
        .primary()
        .spawn();

        // App task: do one read then drop the reader without a FIN.
        async move {
            let mut buf = BytesMut::with_capacity(64);
            let _ = reader.read_into(&mut buf).await;
            drop(reader); // no FIN received → Drop sends STOP_SENDING
        }
        .primary()
        .spawn();
    });
}

/// A Reset stashed in `pending_rx` behind the budget boundary must still suppress the drop-time
/// STOP_SENDING. The app does exactly one read — which delivers `BUDGET` data frames and stashes
/// the remainder (more data + the Reset) — then drops the reader without reading again. The drop
/// path scans `pending_rx`, sees the Reset, and must NOT emit STOP_SENDING to the (already
/// resetting) peer.
#[test]
fn drop_with_reset_in_pending_rx_suppresses_stop_sending() {
    // >BUDGET frames → unreasonably large snapshot; rely on the explicit assertions instead.
    let _guard = crate::testing::without_snapshots();
    sim(|| {
        let (mut reader, mut pusher) = make_pair();
        let budget = crate::stream::coop::BUDGET as u64;
        // More than one budget of data so a single read leaves a non-empty `pending_rx`, with the
        // Reset stashed behind the boundary.
        let total = budget + 5;

        async move {
            for offset in 0..total {
                pusher.push_data(offset, b"z", false);
            }
            pusher.push_reset(VarInt::from_u8(9));
            // No STOP_SENDING must arrive: the reader saw the stashed Reset on drop.
            let frames = pusher.recv_frames_timeout(1.s()).await;
            assert!(
                frames.is_none(),
                "no STOP_SENDING should be emitted when a Reset was stashed in pending_rx"
            );
        }
        .primary()
        .spawn();

        async move {
            let mut buf = BytesMut::with_capacity((total + 1) as usize);
            // Exactly one read: delivers BUDGET frames, stashes the rest (incl. the Reset).
            let n = reader.read_into(&mut buf).await.expect("first read failed");
            assert!(n > 0, "first read should deliver data");
            assert!(
                !reader.0.pending_rx.is_empty(),
                "remainder (incl. Reset) must be stashed after one budget-bounded read"
            );
            drop(reader); // drain_pending_reset must find the stashed Reset → no STOP_SENDING
        }
        .primary()
        .spawn();
    });
}

/// Dropping the Reader during panic sends ABNORMAL_TERMINATION to both sides.
#[test]
fn panic_drop_sends_abnormal_termination_reset() {
    sim(|| {
        let (reader, mut pusher) = make_pair();

        async move {
            let frames = pusher.recv_frames().await;
            assert_eq!(frames.len(), 1, "expected exactly one outbound frame");
            assert!(
                matches!(
                    frames.front().unwrap().header,
                    Header::QueueReset {
                        reset_target: ResetTarget::Both,
                        error_code,
                        ..
                    } if error_code == error::ABNORMAL_TERMINATION
                ),
                "expected exactly one QueueReset(Both, ABNORMAL_TERMINATION) on panic drop"
            );
        }
        .primary()
        .spawn();

        async move {
            let panic_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                // Move ownership into the panic scope so Drop runs while the
                // thread is panicking and emits ABNORMAL_TERMINATION.
                let moved_reader = reader;
                let _ = &moved_reader;
                panic!("intentional test panic while dropping reader");
            }));
            assert!(panic_result.is_err());
        }
        .primary()
        .spawn();
    });
}

/// After clean FIN completion, dropping Reader must not emit STOP_SENDING.
#[test]
fn drop_after_fin_completion_sends_no_reset() {
    sim(|| {
        let (mut reader, mut pusher) = make_pair();

        async move {
            pusher.push_data(0, b"ok", true);
            let frames = pusher.recv_frames_timeout(1.s()).await;
            assert!(
                frames.is_none(),
                "no frame should be emitted after clean completion"
            );
        }
        .primary()
        .spawn();

        async move {
            let mut buf = BytesMut::with_capacity(16);
            let outcome = reader.read_to_end(&mut buf).await.expect("read failed");
            assert_eq!(outcome, ReadToEnd::Complete(2));
            assert_eq!(&buf[..], b"ok");
            drop(reader);
        }
        .primary()
        .spawn();
    });
}

/// Flow-control violations should emit exactly one reset frame even if the app
/// performs additional reads after the initial error.
#[test]
fn queue_control_violation_emits_single_reset_frame() {
    sim(|| {
        let (mut reader, mut pusher) = make_pair();
        let payload = vec![0u8; reader.0.window_size as usize + 1];

        async move {
            pusher.push_data(0, &payload, false);
            let frames = pusher.recv_frames().await;
            assert_eq!(frames.len(), 1, "expected exactly one reset frame");
            assert!(
                matches!(
                    frames.front().unwrap().header,
                    Header::QueueReset {
                        reset_target: ResetTarget::Both,
                        error_code,
                        ..
                    } if error_code == error::QUEUE_CONTROL_ERROR
                ),
                "expected one QUEUE_CONTROL_ERROR reset"
            );

            let extra = pusher.recv_frames_timeout(1.s()).await;
            assert!(
                extra.is_none(),
                "reader should not emit additional frames on follow-up reads"
            );
        }
        .primary()
        .spawn();

        async move {
            let mut buf = BytesMut::with_capacity(32);
            let first = reader
                .read_into(&mut buf)
                .await
                .expect_err("expected InvalidData on first violating read");
            assert_eq!(first.kind(), std::io::ErrorKind::InvalidData);

            let second = reader
                .read_into(&mut buf)
                .await
                .expect_err("expected sticky reset on follow-up read");
            assert_eq!(second.kind(), std::io::ErrorKind::ConnectionReset);
        }
        .primary()
        .spawn();
    });
}

/// `read_to_end` should report `BufferFull` if the application-provided buffer
/// has no remaining capacity at call time.
#[test]
fn read_to_end_empty_buffer_returns_buffer_full() {
    sim(|| {
        let (mut reader, mut pusher) = make_pair();

        async move {
            pusher.push_data(0, b"hello", true);
        }
        .primary()
        .spawn();

        async move {
            let mut backing = BytesMut::with_capacity(16);
            let mut limited = backing.with_write_limit(0);
            let outcome = reader
                .read_to_end(&mut limited)
                .await
                .expect("expected BufferFull for zero-capacity buffer");
            assert_eq!(outcome, ReadToEnd::BufferFull(0));
            assert!(backing.is_empty());
        }
        .primary()
        .spawn();
    });
}

/// `read_to_end` should return `BufferFull` once a fixed-size/non-growable
/// buffer is full, while preserving bytes that were already copied.
#[test]
fn read_to_end_full_buffer_returns_buffer_full() {
    sim(|| {
        let (mut reader, mut pusher) = make_pair();

        async move {
            pusher.push_data(0, b"hello", true);
        }
        .primary()
        .spawn();

        async move {
            let mut backing = BytesMut::with_capacity(16);
            let outcome = {
                let mut limited = backing.with_write_limit(1);
                reader
                    .read_to_end(&mut limited)
                    .await
                    .expect("expected BufferFull once fixed-size buffer is full")
            };
            assert_eq!(outcome, ReadToEnd::BufferFull(1));
            assert_eq!(&backing[..], b"h");
        }
        .primary()
        .spawn();
    });
}

/// When the frame channel receiver is dropped (simulating a dead endpoint) the
/// Reader surfaces a `BrokenPipe` error when it tries to send flow-control
/// frames (e.g. `MAX_DATA`).  The Reader must not panic.
#[test]
fn broken_frame_channel_is_handled_gracefully() {
    sim(|| {
        let (mut reader, pusher) = make_pair();
        let window_size = reader.0.window_size;

        // Destructure pusher to drop the original frame_rx (breaks reader's
        // frame_tx).  A fresh disconnected receiver takes its place so the
        // Pusher struct remains valid for pushing stream messages.
        let Pusher {
            dispatcher,
            queue_id,
            binding_id,
            frame_rx: _closed,
            frame_storage,
            recv_credit_pool,
        } = pusher;
        let mut pusher = Pusher {
            dispatcher,
            queue_id,
            binding_id,
            // Dummy disconnected receiver — not used for assertions in this test.
            frame_rx: frame::submission_channel(1).1,
            frame_storage,
            recv_credit_pool,
        };

        // Endpoint task: push enough data to trigger a MAX_DATA send without
        // exceeding the advertised receive window. The hint signals ongoing writer demand so the
        // reader attempts a window extension (which then fails on the closed frame channel).
        let payload = vec![0u8; (window_size / 2 + 1) as usize];
        let payload_len = payload.len();
        let hint = window_size + payload_len as u64;
        async move {
            pusher.push_data_hint(0, &payload, false, hint);
        }
        .primary()
        .spawn();

        // App task: MAX_DATA cannot be sent (frame channel closed) → BrokenPipe.
        async move {
            let mut buf = BytesMut::with_capacity(payload_len + 16);
            let err = reader
                .read_into(&mut buf)
                .await
                .expect_err("expected BrokenPipe when frame channel is closed");
            assert_eq!(err.kind(), std::io::ErrorKind::BrokenPipe);
        }
        .primary()
        .spawn();
    });
}

/// Reproduces the production panic: the reader's first
/// `maybe_send_max_data` parks on a recv pool that cannot grant the full
/// delta. On the *next* poll — driven by the stream channel waking the
/// reader's task when more data arrives — `maybe_send_max_data` re-enters
/// `poll_acquire` while the slot is still RC_LINKED, tripping the
/// `prepare_park` debug assertion (refcount=1 vs. 2).
///
/// Setup:
///   * Recv pool capacity is small, and we pre-park a separate slot that
///     consumes everything. The reader's window-extension acquire then
///     genuinely parks on a live tier list (no closed-pool short-circuit).
///   * The distributor is constructed but never run, so no grants ever
///     fire — the slot stays RC_LINKED indefinitely.
///   * The pusher delivers data in two batches separated by a yield so
///     the reader's task is re-woken via the stream channel while the
///     pool slot is still parked.
#[test]
fn maybe_send_max_data_re_polls_without_double_parking() {
    sim(|| {
        // Zero-capacity pool with an unrestricted per-priority cap: every
        // acquire takes the park branch. (`Config::normalized` clamps
        // `max_single_acquire` to capacity *unless* capacity is zero,
        // which is exactly the carve-out tests use to force parking.)
        let cfg = crate::credit::Config {
            capacity: 0,
            max_single_acquire: [u64::MAX; crate::credit::Priority::LEVELS],
            min_grant_slice: [u64::MAX; crate::credit::Priority::LEVELS],
        };
        let pool = crate::sync::Arc::new(crate::credit::Pool::new(cfg));
        // Keep a distributor alive (so the pool stays open) but never
        // run it — `Distributor::drop` is what closes the pool.
        let distributor = crate::credit::Distributor::new(pool.clone());

        let (mut reader, mut pusher) = make_pair_with_pool(pool);
        let window_size = reader.0.window_size;
        let payload_first = vec![0xab; (window_size / 2 + 1) as usize];
        let payload_first_len = payload_first.len();
        let payload_second = vec![0xcd; 64];
        let payload_second_len = payload_second.len();
        // Signal ongoing writer demand so the reader attempts a window extension and parks on the
        // zero-capacity pool — that park is what the double-park short-circuit must handle.
        let hint = window_size + payload_first_len as u64;

        async move {
            pusher.push_data_hint(0, &payload_first, false, hint);
            // Yield so the app task consumes the first batch and parks
            // on the pool. Then push more — the stream-channel wake
            // re-polls the reader's task while the pool slot is still
            // RC_LINKED, exercising the `poll_granted` short-circuit.
            bach::task::yield_now().await;
            bach::task::yield_now().await;
            pusher.push_data(payload_first_len as u64, &payload_second, false);
            // Hold the distributor for the lifetime of the test so the
            // pool never closes mid-poll.
            let _keep_alive = &distributor;
            // Let the app task make whatever progress it can; if the
            // double-park bug fires, this test panics in poll_acquire
            // before either side completes.
            1.s().sleep().await;
        }
        .primary()
        .spawn();

        async move {
            let mut buf = BytesMut::with_capacity(payload_first_len + payload_second_len + 16);
            let n = reader.read_into(&mut buf).await.expect("first read failed");
            assert_eq!(n, payload_first_len);
            // The second read drives `poll_read_into_inner` again; before
            // the fix this panicked in `prepare_park`'s debug_assert.
            // After the fix it returns Pending on the existing park and
            // delivers the buffered payload_second when the stream
            // channel fires.
            let n2 = reader
                .read_into(&mut buf)
                .await
                .expect("second read failed");
            assert_eq!(n2, payload_second_len);
        }
        .primary()
        .spawn();
    });
}

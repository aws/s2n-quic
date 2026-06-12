// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Tests for the stream Writer.
//!
//! ## Organization
//!
//! * **Bach async tests** – run the Writer with two primary tasks:
//!   * **Application task** owns [`Writer`] and calls write APIs.
//!   * **Endpoint task** owns [`Pusher`] and asserts on emitted [`Frame`]s.

use super::*;
use crate::{
    endpoint::frame::{self, Frame, Header, PriorityStorage, SubmissionReceiver},
    intrusive,
    packet::datagram::ResetTarget,
    path::secret::map::Entry as PathSecretEntry,
    stream::metrics::WriterMetrics,
    testing::sim,
};
use bach::{ext::*, time::timeout};
use bytes::Bytes;
use s2n_quic_core::{endpoint, stream::testing::Data, varint::VarInt};
use std::{
    net::SocketAddr,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    task::Poll,
    time::Duration,
};

fn test_writer_metrics() -> Arc<WriterMetrics> {
    Arc::new(WriterMetrics::new(
        &crate::counter::Registry::default(),
        "test",
    ))
}

// ─── Test helpers ─────────────────────────────────────────────────────────────

struct PairBuilder {
    ep_type: endpoint::Type,
    credit_pool: Option<crate::sync::Arc<crate::credit::Pool>>,
    priority: crate::credit::Priority,
    params: Option<s2n_quic_core::dc::ApplicationParams>,
    application_data: Option<crate::path::secret::map::ApplicationData>,
}

impl Default for PairBuilder {
    fn default() -> Self {
        Self {
            ep_type: endpoint::Type::Client,
            credit_pool: None,
            priority: crate::credit::Priority::default(),
            params: None,
            application_data: None,
        }
    }
}

impl PairBuilder {
    fn server() -> Self {
        Self {
            ep_type: endpoint::Type::Server,
            credit_pool: None,
            priority: crate::credit::Priority::default(),
            params: None,
            application_data: None,
        }
    }

    /// Override the handshake `ApplicationParams` used to build the writer. Lets a test model the
    /// production split where the connection-level `remote_max_data` differs from the per-stream
    /// `local_recv_max_data` the peer actually advertises and enforces.
    fn with_params(mut self, params: s2n_quic_core::dc::ApplicationParams) -> Self {
        self.params = Some(params);
        self
    }

    fn client_no_remote_queue_id() -> Self {
        Self::default()
    }

    /// Override the credit pool used by the writer. Allows tests to assert against pool counters
    /// (`debug_available`, `debug_parked_demand`) and to drive the pool into specific states.
    fn with_credit_pool(mut self, pool: crate::sync::Arc<crate::credit::Pool>) -> Self {
        self.credit_pool = Some(pool);
        self
    }

    fn with_priority(mut self, priority: crate::credit::Priority) -> Self {
        self.priority = priority;
        self
    }

    fn with_application_data(
        mut self,
        data: crate::path::secret::map::ApplicationData,
    ) -> Self {
        self.application_data = Some(data);
        self
    }

    fn build(self) -> (Writer, Pusher) {
        let acceptor_id = VarInt::from_u8(7);
        let peer: SocketAddr = "127.0.0.1:4433".parse().unwrap();
        let mut entry_builder = PathSecretEntry::builder(peer).endpoint_type(self.ep_type);
        if let Some(params) = self.params {
            entry_builder = entry_builder.params(params);
        }
        if self.application_data.is_some() {
            entry_builder = entry_builder.application_data(self.application_data);
        }
        let path_secret_entry = entry_builder.build();

        let client_state =
            std::sync::Arc::new(crate::queue::ClientState::new(VarInt::from_u16(100), 0));
        let dest_queue_id = client_state.peer_free.try_alloc().unwrap();
        let alloc = client_state.alloc_local(dest_queue_id).unwrap();
        let dispatcher = crate::queue::ClientDispatch::new(client_state);

        let queue_id = alloc.control.queue_id();
        let binding_id = alloc.control.binding_id();

        let (frame_tx, frame_rx) = frame::submission_channel(1);

        let send_credit_pool = self.credit_pool.unwrap_or_else(|| {
            crate::sync::Arc::new(crate::credit::Pool::new(crate::credit::Config::default()))
        });
        let writer = match self.ep_type {
            endpoint::Type::Client => Writer::new_client(
                frame_tx,
                path_secret_entry,
                dest_queue_id,
                acceptor_id,
                alloc.control,
                crate::time::DefaultClock::default(),
                test_writer_metrics(),
                send_credit_pool,
                self.priority,
            ),
            endpoint::Type::Server => Writer::new_server(
                frame_tx,
                path_secret_entry,
                dest_queue_id,
                acceptor_id,
                alloc.control,
                crate::time::DefaultClock::default(),
                test_writer_metrics(),
                send_credit_pool,
                self.priority,
            ),
        };

        let pusher = Pusher {
            dispatcher,
            queue_id,
            binding_id,
            frame_rx,
            frame_storage: PriorityStorage::default(),
            assembler: PayloadAssembler::default(),
        };

        (writer, pusher)
    }
}

fn make_client_pair() -> (Writer, Pusher) {
    PairBuilder::default().build()
}

fn make_server_pair() -> (Writer, Pusher) {
    PairBuilder::server().build()
}

#[test]
fn peer_addr_returns_handshake_addr() {
    let (writer, _) = make_client_pair();
    let expected: SocketAddr = "127.0.0.1:4433".parse().unwrap();
    assert_eq!(writer.peer_addr(), expected);
}

#[test]
fn application_data_is_none_when_not_set() {
    let (writer, _) = make_client_pair();
    assert!(writer.application_data().is_none());
}

#[test]
fn application_data_returns_value_from_path_secret_entry() {
    let data: crate::path::secret::map::ApplicationData = Arc::new(42u32);
    let (writer, _) = PairBuilder::default()
        .with_application_data(data.clone())
        .build();
    let returned = writer
        .application_data()
        .expect("application_data should be Some");
    assert!(Arc::ptr_eq(returned, &data));
}

/// Reassembles payload from a sequence of data frames, validating contiguous
/// offsets and FIN semantics along the way.
///
/// Designed to be held for the lifetime of a Pusher task so that offset tracking
/// stays consistent across multiple `recv_frames` batches.
#[derive(Default)]
struct PayloadAssembler {
    expected_offset: u64,
    payload: Vec<u8>,
    fin_count: usize,
}

impl PayloadAssembler {
    /// Feeds a batch of frames through the assembler, asserting that each frame
    /// carries a contiguous offset and valid FIN semantics.
    ///
    /// `header_filter` is called with each frame's header to perform
    /// variant-specific assertions (e.g. ensuring only QueueData is emitted).
    /// It must return `(offset, is_fin)` for the frame.
    fn push(
        &mut self,
        frames: &intrusive::Queue<Frame>,
        header_filter: impl Fn(&Header) -> (VarInt, bool),
    ) {
        for frame in frames.iter() {
            assert_eq!(
                self.fin_count, 0,
                "frame after FIN: writer must not send data after setting is_fin"
            );
            let (offset, is_fin) = header_filter(&frame.header);
            assert_eq!(offset.as_u64(), self.expected_offset);
            if is_fin {
                self.fin_count += 1;
            }
            for chunk in frame.payload.chunks() {
                self.payload.extend_from_slice(chunk);
            }
            self.expected_offset += frame.payload.len() as u64;
        }
    }

    /// Feeds frames expecting only `Header::QueueData` variants.
    fn push_queue_data(&mut self, frames: &intrusive::Queue<Frame>) {
        self.push(frames, |header| match *header {
            Header::QueueData { offset, is_fin, .. } => (offset, is_fin),
            _ => panic!("expected QueueData frame, got {header:?}"),
        });
    }

    /// Feeds frames expecting QueueData-init variants (with dest_acceptor_id).
    fn push_queue_data_init(&mut self, frames: &intrusive::Queue<Frame>) {
        self.push(frames, |header| match *header {
            Header::QueueData {
                offset,
                is_fin,
                dest_acceptor_id: Some(_),
                ..
            } => (offset, is_fin),
            _ => panic!("expected QueueData-init frame, got {header:?}"),
        });
    }

    fn assert_payload(&self, expected: &[u8]) {
        assert_eq!(self.payload, expected);
    }

    fn assert_fin_count(&self, expected: usize) {
        assert_eq!(self.fin_count, expected, "unexpected FIN count");
    }
}

struct Pusher {
    dispatcher: crate::queue::ClientDispatch,
    queue_id: VarInt,
    binding_id: VarInt,
    frame_rx: SubmissionReceiver,
    frame_storage: PriorityStorage,
    assembler: PayloadAssembler,
}

impl Pusher {
    fn push_control(&mut self, message: msg::Control) {
        if self
            .dispatcher
            .send_control(
                self.queue_id,
                self.binding_id,
                intrusive::Entry::new(message),
            )
            .is_err()
        {
            panic!("send_control should succeed in tests");
        }
    }

    fn push_reset(&mut self, error_code: VarInt) {
        self.push_control(msg::Control::Reset { error_code });
    }

    fn push_max_data(&mut self, maximum_data: VarInt) {
        self.push_control(msg::Control::MaxData { maximum_data });
    }

    /// Receives one submitted burst.
    ///
    /// Tests that expect multiple submission cycles should call this helper
    /// again.
    async fn recv_frames(&mut self) -> intrusive::Queue<Frame> {
        core::future::poll_fn(|cx| self.frame_rx.poll_swap(cx, &mut self.frame_storage)).await;
        let mut combined_frames = intrusive::Queue::default();
        for (_priority, mut queue) in self.frame_storage.drain() {
            combined_frames.append(&mut queue);
        }
        combined_frames
    }

    async fn recv_frames_timeout(&mut self, duration: Duration) -> Option<intrusive::Queue<Frame>> {
        let queue = timeout(duration, self.recv_frames()).await.ok()?;
        if queue.is_empty() {
            None
        } else {
            Some(queue)
        }
    }

    /// Accumulate frames across multiple `recv_frames` batches until a FIN-bearing frame is
    /// observed, returning the combined queue in wire order.
    ///
    /// Per-frame cooperative yielding (`stream/coop.rs`) can split a single large `write_msg`
    /// across several polls, so its frames arrive in more than one `poll_swap` batch. Tests that
    /// assert on the *entire* message (e.g. "exactly one FIN", "all chunks present") must drain
    /// until the FIN rather than inspecting just the first batch.
    async fn recv_frames_until_fin(&mut self) -> intrusive::Queue<Frame> {
        let mut combined = intrusive::Queue::default();
        loop {
            let mut batch = self.recv_frames().await;
            let saw_fin = batch.iter().any(|frame| match frame.header {
                Header::QueueMsg { is_fin, .. } | Header::QueueData { is_fin, .. } => is_fin,
                _ => false,
            });
            combined.append(&mut batch);
            if saw_fin {
                return combined;
            }
        }
    }

    fn complete_all(
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

// ─── Bach async tests ─────────────────────────────────────────────────────────

/// Repro: the Init send path (`send_queue_data_init` / `prepare_early_data`) frames a payload
/// clamped only by MTU / buffered length / offset capacity — NOT by the credits actually held in
/// `pending_credits`. `poll_acquire_credits` for the byte-stream Init path uses `min_progress = 1`,
/// so it returns `Ready` as soon as `pending_credits >= 1`, even when the pool granted far less than
/// the full `want`. `take_credits(payload.len())` then over-reports: it returns `payload.len()` and
/// saturates `pending_credits` to 0, stamping `frame.flow_credits = payload.len()` while only
/// `min(want, max_single_acquire)` was ever debited from the pool.
///
/// Downstream the assembler (or the cancelled drain) releases `frame.flow_credits` back to the
/// send pool, so the pool gains `payload.len() - acquired` phantom credits it never had — a credit
/// over-release that inflates the pool past capacity and breaks the no-snipe / flow-control bound.
///
/// Here we drive a single (uncontended) writer against a pool whose `max_single_acquire` is smaller
/// than one MTU payload, so a single acquire grants less than the Init frame's payload. The frame's
/// `flow_credits` must never exceed what the pool could have handed out.
#[test]
fn init_frame_flow_credits_never_exceed_acquired() {
    sim(|| {
        // Tiny pool: capacity 100 ⇒ max_single_acquire = max(100/256, 1) = 1 for the default
        // (Medium) tier. A single acquire can therefore grant at most 1 credit.
        let pool = crate::sync::Arc::new(crate::credit::Pool::new(crate::credit::Config::new(100)));
        let initial_available = pool.debug_available();

        let (mut writer, mut pusher) = PairBuilder::default()
            .with_credit_pool(pool.clone())
            .build();

        async move {
            // Drain whatever frames the writer emits over the next few polls. Each frame's
            // `flow_credits` must be backed by credit actually debited from the pool: the sum can
            // never exceed the pool's total capacity, and per frame it must not exceed
            // `initial_available`. The pre-fix Init path over-reports (stamps payload.len() while
            // holding fewer credits), tripping either this assertion or the `take_credits`
            // debug_assert in debug builds.
            let mut total_credits = 0u64;
            for _ in 0..4 {
                let Some(frames) = pusher.recv_frames_timeout(Duration::from_millis(50)).await
                else {
                    break;
                };
                for frame in frames.iter() {
                    assert!(
                        (frame.flow_credits as i64) <= initial_available,
                        "frame.flow_credits ({}) exceeds total pool capacity ({}); \
                         releasing it downstream over-releases credit (payload_len={})",
                        frame.flow_credits,
                        initial_available,
                        frame.payload.len(),
                    );
                    total_credits += frame.flow_credits;
                }
            }
            assert!(
                (total_credits as i64) <= initial_available,
                "total flow_credits across all frames ({total_credits}) exceeds pool capacity ({initial_available})",
            );
        }
        .primary()
        .spawn();

        async move {
            // Payload larger than the single-acquire grant so the Init path frames more bytes than
            // the credits it holds.
            let mut payload = Bytes::from(vec![0u8; 64]);
            let _ = writer.write_from(&mut payload).await;
            // Keep the writer alive briefly so the receiver task can observe its frames.
            Duration::from_millis(200).sleep().await;
            drop(writer);
        }
        .primary()
        .spawn();
    });
}

#[test]
fn client_write_all_from_fin_sends_queue_init_with_early_data_and_fin() {
    sim(|| {
        let (mut writer, mut pusher) = make_client_pair();

        async move {
            let frames = pusher.recv_frames().await;
            assert_eq!(frames.len(), 1, "expected exactly one frame");
            let frame = frames.front().unwrap();
            assert!(matches!(
                frame.header,
                Header::QueueData {
                    is_fin: true,
                    dest_acceptor_id: Some(_),
                    ..
                }
            ));
            assert_eq!(frame.payload, &b"hello"[..]);
        }
        .primary()
        .spawn();

        async move {
            let mut payload = Bytes::from_static(b"hello");
            let written = writer
                .write_all_from_fin(&mut payload)
                .await
                .expect("write should succeed");
            assert_eq!(written, 5);
        }
        .primary()
        .spawn();
    });
}

#[test]
fn write_msg_preserves_is_wakeup_flag() {
    sim(|| {
        let (mut writer, mut pusher) = make_server_pair();
        writer.0.remote_max_data = VarInt::MAX;

        async move {
            let frames = pusher.recv_frames().await;
            assert!(!frames.is_empty(), "expected at least one frame");
            let mut queue_msg_count = 0usize;
            for frame in frames.iter() {
                match frame.header {
                    Header::QueueMsg { is_wakeup, .. } => {
                        queue_msg_count += 1;
                        assert!(
                            !is_wakeup,
                            "write_msg should preserve MsgFlags.is_wakeup=false when not flow-control constrained"
                        );
                    }
                    Header::QueueData { is_fin: true, .. } => {
                        // This task exits immediately after write_msg; dropping an
                        // open writer emits a best-effort trailing FIN frame.
                    }
                    _ => panic!("unexpected frame while validating QueueMsg: {:?}", frame.header),
                }
            }
            assert!(queue_msg_count > 0, "expected at least one QueueMsg frame");
        }
        .primary()
        .spawn();

        async move {
            let payload_len = writer.0.packet_size as usize + 1;
            let mut payload = Data::new(payload_len as u64);
            let written = writer
                .write_msg(
                    &mut payload,
                    MsgFlags {
                        is_fin: false,
                        is_wakeup: false,
                    },
                )
                .await
                .expect("write_msg should succeed");
            assert_eq!(written, payload_len);
        }
        .primary()
        .spawn();
    });
}

#[test]
fn write_msg_large_payload_uses_multiple_msg_segments() {
    sim(|| {
        let (mut writer, mut pusher) = make_server_pair();
        writer.0.remote_max_data = VarInt::MAX;
        let chunk_size = writer.0.msg_packet_size as usize;
        let first_segment_size = crate::queue::msg_entry::MAX_CHUNKS as usize * chunk_size;
        let second_segment_size = chunk_size + 17;
        let payload_len = first_segment_size + second_segment_size;

        async move {
            let frames = pusher.recv_frames_until_fin().await;
            assert!(!frames.is_empty(), "expected QueueMsg frames");

            // The first segment must be a full `MAX_CHUNKS` QueueMsg segment at offset 0 carrying
            // no FIN — that is the "splits into multiple segments" invariant this test guards.
            //
            // The remaining bytes after the first segment may be framed either as a second
            // QueueMsg segment OR, when per-frame coop yielding defers the small tail to a fresh
            // poll where it fits the QueueData fast path, as QueueData. Either is correct, so for
            // the tail we only assert reassembly and FIN placement.
            let mut first_msg_id = None::<u64>;
            let mut first_next_chunk = 0u64;
            let mut first_chunk_count = 0usize;
            let mut fin_count = 0usize;
            let mut last_was_fin = false;
            let mut in_tail = false;
            let mut expected = Data::new(payload_len as u64);

            for frame in frames.iter() {
                assert!(
                    !last_was_fin,
                    "no frame may follow the FIN: {:?}",
                    frame.header
                );

                match frame.header {
                    Header::QueueMsg {
                        msg_id,
                        stream_offset,
                        message_size,
                        chunk_size: frame_chunk_size,
                        chunk_index,
                        is_fin,
                        ..
                    } if !in_tail && first_msg_id.is_none_or(|id| id == msg_id.as_u64()) => {
                        // First segment.
                        first_msg_id.get_or_insert(msg_id.as_u64());
                        assert_eq!(stream_offset.as_u64(), 0);
                        assert_eq!(message_size.as_u64(), first_segment_size as u64);
                        assert_eq!(frame_chunk_size.as_u64(), chunk_size as u64);
                        assert_eq!(chunk_index.as_u64(), first_next_chunk);
                        assert!(!is_fin, "first segment must not carry FIN");
                        first_next_chunk += 1;
                        first_chunk_count += 1;
                    }
                    Header::QueueMsg {
                        stream_offset,
                        is_fin,
                        ..
                    } => {
                        // Tail framed as a second QueueMsg segment.
                        in_tail = true;
                        assert_eq!(stream_offset.as_u64(), first_segment_size as u64);
                        if is_fin {
                            fin_count += 1;
                            last_was_fin = true;
                        }
                    }
                    Header::QueueData { offset, is_fin, .. } => {
                        // Tail re-routed to the QueueData fast path by coop deferral.
                        in_tail = true;
                        assert_eq!(offset.as_u64(), first_segment_size as u64);
                        if is_fin {
                            fin_count += 1;
                            last_was_fin = true;
                        }
                    }
                    other => panic!("expected QueueMsg or QueueData, got {:?}", other),
                }

                for chunk in frame.payload.chunks() {
                    expected.receive(std::slice::from_ref(&chunk));
                }
            }

            assert_eq!(
                first_chunk_count,
                crate::queue::msg_entry::MAX_CHUNKS as usize,
                "first segment should fill MAX_CHUNKS"
            );
            assert!(
                in_tail,
                "expected a tail segment after the first MAX_CHUNKS segment"
            );
            assert_eq!(fin_count, 1, "exactly one FIN-bearing frame expected");
            assert!(
                expected.is_finished(),
                "payload should reassemble completely"
            );
        }
        .primary()
        .spawn();

        async move {
            let mut payload = Data::new(payload_len as u64);
            let written = writer
                .write_msg(
                    &mut payload,
                    MsgFlags {
                        is_fin: true,
                        is_wakeup: true,
                    },
                )
                .await
                .expect("write_msg should succeed");
            assert_eq!(written, payload_len);
        }
        .primary()
        .spawn();
    });
}

/// Reproduction: the server writer must not send past the per-stream receive window the peer
/// actually advertises and enforces (`local_recv_max_data`), regardless of the larger
/// connection-level `remote_max_data`.
///
/// Production split: `remote_max_data` (the connection-level data
/// window, ~8 MiB) and `local_recv_max_data` (the per-stream recv window, 64 KiB) diverged. But
/// `Writer::new_server` seeds its initial flow-control budget from `parameters.remote_max_data` and
/// starts in `Open`, so the server writer believes it may send the full 8 MiB immediately — before
/// the peer's reader has advertised any pool-backed window. The peer reader only ever advertises and
/// enforces `local_recv_max_data` (plus pool-backed growth), so the writer overshoots its real
/// window and the reader tears the connection down with `QUEUE_CONTROL_ERROR` ("sender exceeded
/// receive window").
///
/// This test pins the invariant at the writer boundary: with a 64 KiB peer recv window, the server
/// writer must not emit any frame whose end offset exceeds 64 KiB until MAX_DATA grows the window.
/// Before the fix the writer's `remote_max_data` is 8 MiB and it streams a >64 KiB message out in
/// one shot, so the assertion fails.
#[test]
fn server_writer_respects_peer_recv_window_not_connection_window() {
    sim(|| {
        // Model the production split: large connection-level window, small per-stream recv window.
        const CONNECTION_WINDOW: u32 = 8 * 1024 * 1024;
        const PEER_RECV_WINDOW: u32 = 64 * 1024;

        let mut params = s2n_quic_core::dc::testing::TEST_APPLICATION_PARAMS.clone();
        params.remote_max_data = VarInt::from_u32(CONNECTION_WINDOW);
        params.local_send_max_data = VarInt::from_u32(CONNECTION_WINDOW);
        // The peer's reader advertises and enforces this; it is the only true bound on the writer.
        params.local_recv_max_data = VarInt::from_u32(PEER_RECV_WINDOW);

        let (mut writer, mut pusher) = PairBuilder::server().with_params(params).build();

        // A message comfortably larger than the peer recv window but well within the connection
        // window.
        let payload_len = (PEER_RECV_WINDOW as usize) * 4;

        async move {
            // Collect everything the writer emits before any MAX_DATA is granted. The peer would
            // enforce its advertised window of PEER_RECV_WINDOW; any frame whose end offset exceeds
            // that is data the peer never authorized and would reset the connection over.
            let mut max_end_offset = 0u64;
            while let Some(frames) = pusher.recv_frames_timeout(Duration::from_millis(50)).await {
                for frame in frames.iter() {
                    let (offset, len) = match frame.header {
                        Header::QueueData { offset, .. } => (offset.as_u64(), frame.payload.len()),
                        Header::QueueMsg {
                            stream_offset,
                            chunk_size,
                            chunk_index,
                            ..
                        } => (
                            stream_offset.as_u64()
                                + chunk_index.as_u64() * chunk_size.as_u64(),
                            frame.payload.len(),
                        ),
                        // Blocked/other control frames carry no stream bytes.
                        _ => continue,
                    };
                    max_end_offset = max_end_offset.max(offset + len as u64);
                }
            }

            assert!(
                max_end_offset <= PEER_RECV_WINDOW as u64,
                "server writer sent data up to offset {max_end_offset} but the peer only advertised \
                 a {PEER_RECV_WINDOW}-byte receive window; the overshoot is data the peer never \
                 authorized and would trigger QUEUE_CONTROL_ERROR (\"sender exceeded receive \
                 window\")",
            );
        }
        .primary()
        .spawn();

        async move {
            let mut payload = Data::new(payload_len as u64);
            // Drive the writer once. It blocks once it has sent up to its (believed) window.
            let slot = writer.0.slot_ptr();
            let _ = core::future::poll_fn(|cx| {
                let mut buf = Data::new(payload_len as u64);
                match writer.0.poll_write_msg(
                    cx,
                    slot,
                    &mut buf,
                    MsgFlags {
                        is_fin: false,
                        is_wakeup: false,
                    },
                ) {
                    Poll::Pending => Poll::Ready(()),
                    Poll::Ready(_) => Poll::Ready(()),
                }
            })
            .await;
            // Keep the writer alive long enough for the pusher to drain frames.
            let _ = &mut payload;
            50.ms().sleep().await;
        }
        .primary()
        .spawn();
    });
}

#[test]
fn control_reset_terminates_write() {
    sim(|| {
        let (mut writer, mut pusher) = make_client_pair();

        async move {
            pusher.push_reset(VarInt::from_u8(9));
        }
        .primary()
        .spawn();

        async move {
            let mut payload = Bytes::from_static(b"hello");
            let err = writer
                .write_from(&mut payload)
                .await
                .expect_err("expected ConnectionReset");
            assert_eq!(err.kind(), io::ErrorKind::ConnectionReset);
            assert!(writer.0.status.is_shutdown());
        }
        .primary()
        .spawn();
    });
}

#[test]
fn client_second_write_blocks_until_max_data() {
    sim(|| {
        let (mut writer, mut pusher) = make_client_pair();

        async move {
            let frames = pusher.recv_frames().await;
            assert_eq!(frames.len(), 1, "expected exactly one frame");
            let frame = frames.front().unwrap();
            assert!(matches!(
                frame.header,
                Header::QueueData {
                    is_fin: false,
                    dest_acceptor_id: Some(_),
                    ..
                }
            ));
            assert_eq!(frame.payload, &b"hello"[..]);

            // Give the app task a scheduling opportunity to attempt a second
            // write while Writer is still in `Status::QueueBindSent` (before any
            // remote MAX_DATA credit is injected).
            bach::task::yield_now().await;
            pusher.push_max_data(VarInt::from_u16(4096));

            let next = pusher.recv_frames().await;
            assert_eq!(next.len(), 1, "expected exactly one frame");
            let frame = next.front().unwrap();
            assert!(matches!(
                frame.header,
                Header::QueueData { is_fin: true, .. }
            ));
            assert_eq!(frame.payload, &b"!"[..]);
        }
        .primary()
        .spawn();

        async move {
            let mut first = Bytes::from_static(b"hello");
            let written = writer.write_from(&mut first).await.expect("first write");
            assert_eq!(written, 5);

            let mut second = Bytes::from_static(b"!");
            let write_blocked =
                core::future::poll_fn(|cx| match writer.poll_write_from(cx, &mut second, false) {
                    Poll::Pending => Poll::Ready(true),
                    Poll::Ready(_) => Poll::Ready(false),
                })
                .await;
            assert!(
                write_blocked,
                "expected second write to block before MAX_DATA"
            );

            let written = writer
                .write_from_fin(&mut second)
                .await
                .expect("second write");
            assert_eq!(written, 1);
        }
        .primary()
        .spawn();
    });
}

#[test]
fn server_first_write_emits_queue_data_not_queue_init() {
    sim(|| {
        let (mut writer, mut pusher) = make_server_pair();

        async move {
            let frames = pusher.recv_frames().await;
            assert!(!frames.is_empty(), "expected at least one QueueData frame");
            pusher.assembler.push_queue_data(&frames);
            pusher.assembler.assert_payload(b"hello");
        }
        .primary()
        .spawn();

        async move {
            let mut payload = Bytes::from_static(b"hello");
            let written = writer
                .write_from(&mut payload)
                .await
                .expect("write should succeed");
            assert_eq!(written, 5);
        }
        .primary()
        .spawn();
    });
}

#[test]
fn server_queue_control_budget_caps_transmitted_bytes() {
    sim(|| {
        let (mut writer, mut pusher) = make_server_pair();
        writer.0.remote_max_data = VarInt::from_u8(3);

        async move {
            let frames = pusher.recv_frames().await;
            assert!(!frames.is_empty(), "expected at least one QueueData frame");
            pusher.assembler.push_queue_data(&frames);
            pusher.assembler.assert_payload(b"abc");

            let extra = pusher.recv_frames_timeout(Duration::from_millis(100)).await;
            assert!(
                extra.is_none(),
                "expected no additional burst after exhausting remote flow budget"
            );
        }
        .primary()
        .spawn();

        async move {
            let mut payload = Bytes::from_static(b"abcdef");
            let written = writer
                .write_from(&mut payload)
                .await
                .expect("write should respect remote budget");
            assert_eq!(written, 3);
            assert_eq!(payload.as_ref(), &b"def"[..]);
        }
        .primary()
        .spawn();
    });
}

/// When the writer can send part of its buffer but the remainder exceeds the remote window, the
/// emitted data frame carries the in-band `blocked` bit and a `largest_offset` equal to the full
/// high watermark — so the reader learns the writer wants more without a standalone frame.
#[test]
fn data_frame_carries_blocked_bit_when_partially_windowed() {
    sim(|| {
        let (mut writer, mut pusher) = make_server_pair();
        writer.0.remote_max_data = VarInt::from_u8(3);

        async move {
            let frames = pusher.recv_frames().await;
            let data = frames
                .iter()
                .find(|f| matches!(f.header, Header::QueueData { .. }))
                .expect("expected a QueueData frame");
            match data.header {
                Header::QueueData {
                    offset,
                    largest_offset,
                    blocked,
                    ..
                } => {
                    assert_eq!(offset, VarInt::ZERO);
                    assert!(
                        blocked,
                        "data frame must carry the blocked bit when more is buffered"
                    );
                    // High watermark is the full 6-byte buffer even though only 3 bytes fit.
                    assert_eq!(largest_offset, VarInt::from_u8(6));
                }
                _ => unreachable!(),
            }
        }
        .primary()
        .spawn();

        async move {
            let mut payload = Bytes::from_static(b"abcdef");
            let written = writer
                .write_from(&mut payload)
                .await
                .expect("write should respect remote budget");
            assert_eq!(written, 3);
        }
        .primary()
        .spawn();
    });
}

#[test]
fn client_preserves_max_data_on_out_of_order_lower_update() {
    sim(|| {
        let (mut writer, mut pusher) = make_client_pair();

        async move {
            let init = pusher.recv_frames().await;
            assert_eq!(init.len(), 1, "expected exactly one QueueInit frame");
            let init_frame = init.front().unwrap();
            assert!(matches!(
                init_frame.header,
                Header::QueueData {
                    is_fin: false,
                    dest_acceptor_id: Some(_),
                    ..
                }
            ));
            assert_eq!(init_frame.payload, &b"abc"[..]);

            pusher.push_max_data(VarInt::from_u8(8));
            pusher.push_max_data(VarInt::from_u8(3));

            // The QueueInit consumed 3 bytes, so the assembler starts at offset 3.
            pusher.assembler.expected_offset = 3;
            let next = pusher.recv_frames().await;
            pusher.assembler.push_queue_data(&next);
            pusher.assembler.assert_payload(b"defgh");
        }
        .primary()
        .spawn();

        async move {
            let mut first = Bytes::from_static(b"abc");
            let written = writer.write_from(&mut first).await.expect("first write");
            assert_eq!(written, 3);

            bach::task::yield_now().await;

            let mut second = Bytes::from_static(b"defghij");
            let written = writer.write_from(&mut second).await.expect("second write");
            assert_eq!(
                written, 5,
                "writer should keep the max observed MAX_DATA even when a smaller update arrives later"
            );
        }
        .primary()
        .spawn();
    });
}

#[test]
fn server_write_from_fin_blocks_while_budget_exhausted_then_sends_single_fin_frame() {
    sim(|| {
        let (mut writer, mut pusher) = make_server_pair();
        writer.0.remote_max_data = VarInt::from_u8(1);

        async move {
            // The first burst contains the "a" data frame and may also contain a standalone
            // QueueDataBlocked signal (emitted when the follow-up write hits the exhausted window).
            // Exactly one data frame, the rest blocked signals.
            let first = pusher.recv_frames().await;
            let data: Vec<_> = first
                .iter()
                .filter(|f| matches!(f.header, Header::QueueData { .. }))
                .collect();
            assert_eq!(data.len(), 1, "expected exactly one data frame");
            assert!(matches!(
                data[0].header,
                Header::QueueData {
                    is_fin: false,
                    offset,
                    ..
                } if offset == VarInt::ZERO
            ));
            assert_eq!(data[0].payload, &b"a"[..]);
            assert!(
                first.iter().all(|f| matches!(
                    f.header,
                    Header::QueueData { .. } | Header::QueueDataBlocked { .. }
                )),
                "only data or blocked frames expected in the first burst"
            );

            pusher.push_max_data(VarInt::from_u8(2));

            // After MAX_DATA the FIN data frame goes out (the blocked signal, if any, was already
            // consumed above).
            let second = pusher.recv_frames().await;
            let fin = second
                .iter()
                .find(|f| matches!(f.header, Header::QueueData { is_fin: true, .. }))
                .expect("expected a FIN data frame after MAX_DATA");
            assert!(matches!(
                fin.header,
                Header::QueueData { offset, .. } if offset == VarInt::from_u8(1)
            ));
            assert_eq!(fin.payload, &b"b"[..]);
        }
        .primary()
        .spawn();

        async move {
            let mut first = Bytes::from_static(b"a");
            let written = writer.write_from(&mut first).await.expect("first write");
            assert_eq!(written, 1);

            let mut second = Bytes::from_static(b"b");
            let write_blocked =
                core::future::poll_fn(|cx| match writer.poll_write_from(cx, &mut second, true) {
                    Poll::Pending => Poll::Ready(true),
                    Poll::Ready(_) => Poll::Ready(false),
                })
                .await;
            assert!(
                write_blocked,
                "expected write_from_fin to block while remote flow budget is exhausted"
            );

            let written = writer
                .write_from_fin(&mut second)
                .await
                .expect("second write after MAX_DATA");
            assert_eq!(written, 1);
        }
        .primary()
        .spawn();
    });
}

#[test]
fn client_fin_write_then_drop_emits_no_extra_packet() {
    sim(|| {
        let (mut writer, mut pusher) = make_client_pair();

        async move {
            let first = pusher.recv_frames().await;
            assert!(!first.is_empty(), "expected at least one frame");
            pusher.assembler.push_queue_data_init(&first);
            pusher.assembler.assert_payload(b"hi");
            pusher.assembler.assert_fin_count(1);

            let extra = pusher.recv_frames_timeout(Duration::from_millis(100)).await;
            assert!(
                extra.is_none(),
                "expected no extra frame burst after FIN was already sent"
            );
        }
        .primary()
        .spawn();

        async move {
            let mut payload = Bytes::from_static(b"hi");
            let written = writer
                .write_from_fin(&mut payload)
                .await
                .expect("fin write");
            assert_eq!(written, 2);
            drop(writer);
        }
        .primary()
        .spawn();
    });
}

#[test]
fn server_fin_write_then_drop_emits_no_extra_packet() {
    sim(|| {
        let (mut writer, mut pusher) = make_server_pair();

        async move {
            let first = pusher.recv_frames().await;
            assert!(!first.is_empty(), "expected at least one frame");
            pusher.assembler.push_queue_data(&first);
            pusher.assembler.assert_payload(b"hi");
            pusher.assembler.assert_fin_count(1);

            let extra = pusher.recv_frames_timeout(Duration::from_millis(100)).await;
            assert!(
                extra.is_none(),
                "expected no extra frame burst after FIN was already sent"
            );
        }
        .primary()
        .spawn();

        async move {
            let mut payload = Bytes::from_static(b"hi");
            let written = writer
                .write_from_fin(&mut payload)
                .await
                .expect("fin write");
            assert_eq!(written, 2);
            drop(writer);
        }
        .primary()
        .spawn();
    });
}

#[test]
fn transmission_error_completion_causes_broken_pipe_and_reset() {
    sim(|| {
        let (mut writer, mut pusher) = make_server_pair();

        async move {
            let first = pusher.recv_frames().await;
            pusher.complete_all(
                first,
                frame::TransmissionStatus::Failed(frame::FailureReason::TransmissionError),
            );

            let reset = pusher
                .recv_frames_timeout(Duration::from_secs(1))
                .await
                .expect("expected QueueReset after transmission failure");
            assert_eq!(reset.len(), 1, "expected exactly one reset frame");
            assert!(
                matches!(
                    reset.front().unwrap().header,
                    Header::QueueReset {
                        reset_target: ResetTarget::Both,
                        error_code,
                        ..
                    } if error_code == error::RETRANSMISSIONS_EXHAUSTED
                ),
                "expected retransmission-exhausted QueueReset"
            );
        }
        .primary()
        .spawn();

        async move {
            let mut payload = Bytes::from_static(b"hello");
            let written = writer.write_from(&mut payload).await.expect("first write");
            assert_eq!(written, 5);

            bach::task::yield_now().await;

            let mut retry = Bytes::from_static(b"!");
            let err = writer
                .write_from(&mut retry)
                .await
                .expect_err("expected BrokenPipe");
            assert_eq!(err.kind(), io::ErrorKind::BrokenPipe);
        }
        .primary()
        .spawn();
    });
}

#[test]
fn drop_open_writer_sends_fin_packet() {
    sim(|| {
        let (writer, mut pusher) = make_server_pair();

        async move {
            let frames = pusher.recv_frames().await;
            assert_eq!(frames.len(), 1, "expected exactly one FIN frame on drop");
            assert!(matches!(
                frames.front().unwrap().header,
                Header::QueueData {
                    is_fin: true,
                    offset,
                    ..
                } if offset == VarInt::ZERO
            ));
        }
        .primary()
        .spawn();

        async move {
            drop(writer);
        }
        .primary()
        .spawn();
    });
}

#[test]
fn panic_drop_sends_abnormal_termination_reset() {
    sim(|| {
        let (writer, mut pusher) = make_server_pair();

        async move {
            let frames = pusher.recv_frames().await;
            assert_eq!(frames.len(), 1, "expected exactly one reset frame");
            assert!(
                matches!(
                    frames.front().unwrap().header,
                    Header::QueueReset {
                        reset_target: ResetTarget::Both,
                        error_code,
                        ..
                    } if error_code == error::ABNORMAL_TERMINATION
                ),
                "expected QueueReset(Both, ABNORMAL_TERMINATION) when dropping during panic"
            );
        }
        .primary()
        .spawn();

        async move {
            let panic_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
                let _writer = writer;
                panic!("intentional test panic while dropping writer");
            }));
            assert!(panic_result.is_err());
        }
        .primary()
        .spawn();
    });
}

// ─── QueueInitReset / QueueInitFin tests ───────────────────────────────────────
//
// The tests below document the behavior of the QueueInitReset and QueueInitFin
// messages, which are sent when the client needs to notify the server while in
// QueueBindSent state (before MAX_DATA arrives and the server queue ID becomes known).
//
// QueueInitReset is for error/abnormal termination; QueueInitFin is for graceful close.

/// Panic drop while in QueueBindSent: QueueReset is sent so the server can
/// look up the stream via binding_id and terminate both queues.
#[test]
fn client_panic_drop_during_queue_init_sent_sends_queue_reset() {
    crate::testing::sim(|| {
        use crate::testing::ext::*;

        let (mut writer, mut pusher) = PairBuilder::client_no_remote_queue_id().build();

        async move {
            // QueueInit arrives first
            let frames = pusher.recv_frames().await;
            assert_eq!(frames.len(), 1);
            assert!(matches!(
                frames.front().unwrap().header,
                Header::QueueData { is_fin: false, dest_acceptor_id: Some(_), .. }
            ));

            // QueueReset follows on panic
            let reset = pusher
                .recv_frames_timeout(Duration::from_secs(1))
                .await
                .expect("expected QueueReset after panic drop");
            assert_eq!(reset.len(), 1);
            assert!(
                matches!(
                    reset.front().unwrap().header,
                    Header::QueueReset { error_code, .. } if error_code == error::ABNORMAL_TERMINATION
                ),
                "expected QueueReset(ABNORMAL_TERMINATION) with non-sentinel attempt_id on panic in QueueBindSent"
            );
        }
        .primary()
        .spawn();

        async move {
            let mut payload = Bytes::from_static(b"hello");
            let written = writer.write_from(&mut payload).await.expect("first write");
            assert_eq!(written, 5);
            assert!(writer.0.status.is_init_sent());

            // yield so the pusher receives QueueInit before the panic triggers QueueReset
            bach::task::yield_now().await;

            let panic_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
                let _writer = writer;
                panic!("intentional test panic during QueueBindSent");
            }));
            assert!(panic_result.is_err());
        }
        .primary()
        .spawn();
    });
}

/// TransmissionError during QueueBindSent: QueueReset is sent so the server
/// can look up the stream by binding_id and terminate the queues.
#[test]
fn client_transmission_error_during_queue_init_sent_sends_queue_reset() {
    crate::testing::sim(|| {
        use crate::testing::ext::*;

        let (mut writer, mut pusher) = PairBuilder::client_no_remote_queue_id().build();

        async move {
            let frames = pusher.recv_frames().await;
            assert_eq!(frames.len(), 1);
            pusher.complete_all(
                frames,
                frame::TransmissionStatus::Failed(frame::FailureReason::TransmissionError),
            );

            // QueueReset should be emitted
            let reset = pusher
                .recv_frames_timeout(Duration::from_secs(1))
                .await
                .expect("expected QueueReset after TransmissionError in QueueBindSent");
            assert_eq!(reset.len(), 1);
            assert!(
                matches!(
                    reset.front().unwrap().header,
                    Header::QueueReset { error_code, .. } if error_code == error::RETRANSMISSIONS_EXHAUSTED
                ),
                "expected QueueReset(RETRANSMISSIONS_EXHAUSTED) with non-sentinel attempt_id"
            );
        }
        .primary()
        .spawn();

        async move {
            let mut payload = Bytes::from_static(b"hello");
            let written = writer.write_from(&mut payload).await.expect("first write");
            assert_eq!(written, 5);

            bach::task::yield_now().await;

            let mut retry = Bytes::from_static(b"!");
            let err = writer
                .write_from(&mut retry)
                .await
                .expect_err("expected BrokenPipe");
            assert_eq!(err.kind(), io::ErrorKind::BrokenPipe);
            assert!(writer.0.status.is_shutdown());
        }
        .primary()
        .spawn();
    });
}

/// PeerDead completion failure during QueueBindSent: no QueueReset because the
/// peer is unreachable (it makes no difference whether we send it or not).
#[test]
fn client_peer_dead_during_queue_init_sent_no_reset() {
    crate::testing::sim(|| {
        use crate::testing::ext::*;

        let (mut writer, mut pusher) = PairBuilder::client_no_remote_queue_id().build();

        async move {
            let frames = pusher.recv_frames().await;
            assert_eq!(frames.len(), 1);
            pusher.complete_all(
                frames,
                frame::TransmissionStatus::Failed(frame::FailureReason::PeerDead),
            );

            // PeerDead: no QueueReset (peer can't receive it)
            let extra = pusher.recv_frames_timeout(Duration::from_secs(1)).await;
            assert!(extra.is_none(), "no QueueReset for PeerDead");
        }
        .primary()
        .spawn();

        async move {
            let mut payload = Bytes::from_static(b"hello");
            let written = writer.write_from(&mut payload).await.expect("first write");
            assert_eq!(written, 5);

            bach::task::yield_now().await;

            let mut retry = Bytes::from_static(b"!");
            let err = writer
                .write_from(&mut retry)
                .await
                .expect_err("expected TimedOut");
            assert_eq!(err.kind(), io::ErrorKind::TimedOut);
            assert!(writer.0.status.is_shutdown());
        }
        .primary()
        .spawn();
    });
}

/// Dropping a client writer while in QueueBindSent (before MAX_DATA arrives):
/// the drop calls shutdown() → send_fin_packet() → send QueueData FIN(),
/// which sends QueueData FIN to let the server deliver EOF to the reader.
#[test]
fn client_drop_in_queue_init_sent_sends_queue_data_fin() {
    sim(|| {
        let (mut writer, mut pusher) = PairBuilder::client_no_remote_queue_id().build();

        async move {
            // QueueInit frame arrives first
            let frames = pusher.recv_frames().await;
            assert_eq!(frames.len(), 1);
            assert!(matches!(
                frames.front().unwrap().header,
                Header::QueueData { is_fin: false, dest_acceptor_id: Some(_), .. }
            ));

            // QueueData FIN arrives when the writer is dropped
            let fin_frames = pusher
                .recv_frames_timeout(Duration::from_secs(1))
                .await
                .expect("expected QueueData FIN on drop in QueueBindSent");
            assert_eq!(fin_frames.len(), 1);
            assert!(
                matches!(
                    fin_frames.front().unwrap().header,
                    Header::QueueData { offset, is_fin: true, dest_acceptor_id: Some(_), .. } if offset == VarInt::from_u8(5)
                ),
                "expected QueueData FIN(offset=5) from normal drop in QueueBindSent"
            );
        }
        .primary()
        .spawn();

        async move {
            let mut payload = Bytes::from_static(b"hello");
            let written = writer.write_from(&mut payload).await.expect("first write");
            assert_eq!(written, 5);
            assert!(writer.0.status.is_init_sent());
            // yield so pusher receives QueueInit before drop
            bach::task::yield_now().await;
            drop(writer);
        }
        .primary()
        .spawn();
    });
}

/// PeerDead completion failure during QueueBindSent: no QueueReset because the
/// peer is unreachable (it makes no difference whether we send it or not).
#[test]
fn client_peer_dead_during_queue_init_sent_no_reset_silent() {
    sim(|| {
        let (mut writer, mut pusher) = PairBuilder::client_no_remote_queue_id().build();

        async move {
            let frames = pusher.recv_frames().await;
            assert_eq!(frames.len(), 1);
            pusher.complete_all(
                frames,
                frame::TransmissionStatus::Failed(frame::FailureReason::PeerDead),
            );

            // No QueueReset emitted (remote_queue_id is None)
            let extra = pusher.recv_frames_timeout(Duration::from_secs(1)).await;
            assert!(
                extra.is_none(),
                "no QueueReset possible during QueueBindSent (remote_queue_id unknown)"
            );
        }
        .primary()
        .spawn();

        async move {
            let mut payload = Bytes::from_static(b"hello");
            let written = writer.write_from(&mut payload).await.expect("first write");
            assert_eq!(written, 5);

            bach::task::yield_now().await;

            let mut retry = Bytes::from_static(b"!");
            let err = writer
                .write_from(&mut retry)
                .await
                .expect_err("expected TimedOut");
            assert_eq!(err.kind(), io::ErrorKind::TimedOut);
            assert!(writer.0.status.is_shutdown());
        }
        .primary()
        .spawn();
    });
}

/// TransmissionError during QueueBindSent (legacy test, superseded by
/// client_transmission_error_during_queue_init_sent_sends_queue_reset).
/// This test verifies the application-facing error behavior remains correct.
#[test]
fn client_transmission_error_during_queue_init_sent() {
    sim(|| {
        let (mut writer, mut pusher) = PairBuilder::client_no_remote_queue_id().build();

        async move {
            let frames = pusher.recv_frames().await;
            assert_eq!(frames.len(), 1);
            pusher.complete_all(
                frames,
                frame::TransmissionStatus::Failed(frame::FailureReason::TransmissionError),
            );

            // QueueInitReset is emitted; consume it so the test doesn't leave stale frames.
            let _reset = pusher
                .recv_frames_timeout(Duration::from_secs(1))
                .await
                .expect("expected QueueReset");
        }
        .primary()
        .spawn();

        async move {
            let mut payload = Bytes::from_static(b"hello");
            let written = writer.write_from(&mut payload).await.expect("first write");
            assert_eq!(written, 5);

            bach::task::yield_now().await;

            let mut retry = Bytes::from_static(b"!");
            let err = writer
                .write_from(&mut retry)
                .await
                .expect_err("expected BrokenPipe");
            assert_eq!(err.kind(), io::ErrorKind::BrokenPipe);
            assert!(writer.0.status.is_shutdown());
        }
        .primary()
        .spawn();
    });
}

/// Control channel closed while writer is in QueueBindSent (endpoint died).
/// Next write attempt returns ConnectionReset.
#[test]
fn client_control_channel_closed_during_queue_init_sent() {
    sim(|| {
        let (mut writer, mut pusher) = PairBuilder::client_no_remote_queue_id().build();

        async move {
            // Wait for the QueueInit frame (proves writer reached QueueBindSent)
            let frames = pusher.recv_frames().await;
            assert_eq!(frames.len(), 1);
            assert!(matches!(
                frames.front().unwrap().header,
                Header::QueueData {
                    dest_acceptor_id: Some(_),
                    ..
                }
            ));
            // Close the dispatch (simulates endpoint eviction)
            pusher.dispatcher.close(&mut |_| {});
            drop(pusher);
        }
        .primary()
        .spawn();

        async move {
            let mut payload = Bytes::from_static(b"hello");
            let written = writer.write_from(&mut payload).await.expect("first write");
            assert_eq!(written, 5);
            assert!(writer.0.status.is_init_sent());

            // Next write detects closed control channel
            let mut retry = Bytes::from_static(b"!");
            let err = writer
                .write_from(&mut retry)
                .await
                .expect_err("expected error from closed channel");
            assert_eq!(err.kind(), io::ErrorKind::ConnectionReset);
        }
        .primary()
        .spawn();
    });
}

/// Explicit shutdown() while in QueueBindSent: send_fin_packet() now sends a
/// QueueInitFin so the server can deliver EOF to the reader at the correct offset.
#[test]
fn client_shutdown_during_queue_init_sent() {
    sim(|| {
        let (mut writer, mut pusher) = PairBuilder::client_no_remote_queue_id().build();

        async move {
            let frames = pusher.recv_frames().await;
            assert_eq!(frames.len(), 1);
            assert!(matches!(
                frames.front().unwrap().header,
                Header::QueueData { is_fin: false, dest_acceptor_id: Some(_), .. }
            ));

            // QueueData FIN arrives after explicit shutdown()
            let fin_frames = pusher
                .recv_frames_timeout(Duration::from_secs(1))
                .await
                .expect("expected QueueData FIN after shutdown in QueueBindSent");
            assert_eq!(fin_frames.len(), 1);
            assert!(
                matches!(
                    fin_frames.front().unwrap().header,
                    Header::QueueData { offset, is_fin: true, dest_acceptor_id: Some(_), .. } if offset == VarInt::from_u8(5)
                ),
                "expected QueueData FIN(offset=5) after explicit shutdown() in QueueBindSent"
            );
        }
        .primary()
        .spawn();

        async move {
            let mut payload = Bytes::from_static(b"hello");
            let written = writer.write_from(&mut payload).await.expect("first write");
            assert_eq!(written, 5);
            assert!(writer.0.status.is_init_sent());

            // yield so pusher receives QueueInit before shutdown sends QueueInitFin
            bach::task::yield_now().await;

            writer.shutdown().expect("shutdown should succeed");
            assert!(writer.0.status.is_shutdown());

            // Writes after shutdown return BrokenPipe
            let mut retry = Bytes::from_static(b"!");
            let err = writer
                .write_from(&mut retry)
                .await
                .expect_err("expected BrokenPipe after shutdown");
            assert_eq!(err.kind(), io::ErrorKind::BrokenPipe);
        }
        .primary()
        .spawn();
    });
}

/// Empty-buffer write_from_fin sends QueueInit with empty payload and FIN=true.
/// Tests the Init → QueueBindSent → FinSent transition in a single call.
#[test]
fn client_empty_fin_queue_init() {
    sim(|| {
        let (mut writer, mut pusher) = make_client_pair();

        async move {
            let frames = pusher.recv_frames().await;
            assert_eq!(frames.len(), 1);
            let frame = frames.front().unwrap();
            assert!(matches!(
                frame.header,
                Header::QueueData {
                    is_fin: true,
                    dest_acceptor_id: Some(_),
                    ..
                }
            ));
            assert!(frame.payload.is_empty(), "expected empty payload with FIN");

            // No additional frame on drop (FIN already sent)
            let extra = pusher.recv_frames_timeout(Duration::from_secs(1)).await;
            assert!(extra.is_none(), "no extra frame after FIN already sent");
        }
        .primary()
        .spawn();

        async move {
            let mut payload = Bytes::new();
            let written = writer
                .write_from_fin(&mut payload)
                .await
                .expect("empty fin write");
            assert_eq!(written, 0);
            assert!(writer.0.status.is_fin_sent());
            drop(writer);
        }
        .primary()
        .spawn();
    });
}

/// Server writer receives Reset while in Open state (actively writing).
/// Existing test only covers client during QueueBindSent.
#[test]
fn server_reset_received_while_open() {
    sim(|| {
        let (mut writer, mut pusher) = make_server_pair();

        async move {
            let _first = pusher.recv_frames().await;
            // Now inject reset
            pusher.push_reset(VarInt::from_u8(33));
        }
        .primary()
        .spawn();

        async move {
            let mut payload = Bytes::from_static(b"hello");
            let written = writer.write_from(&mut payload).await.expect("first write");
            assert_eq!(written, 5);

            bach::task::yield_now().await;

            let mut more = Bytes::from_static(b"world");
            let err = writer
                .write_from(&mut more)
                .await
                .expect_err("expected ConnectionReset");
            assert_eq!(err.kind(), io::ErrorKind::ConnectionReset);
            assert!(writer.0.status.is_shutdown());
        }
        .primary()
        .spawn();
    });
}

/// PeerDead completion failure surfaces TimedOut error to the application.
#[test]
fn client_peer_dead_completion_surfaces_timed_out() {
    sim(|| {
        let (mut writer, mut pusher) = make_server_pair();

        async move {
            let frames = pusher.recv_frames().await;
            pusher.complete_all(
                frames,
                frame::TransmissionStatus::Failed(frame::FailureReason::PeerDead),
            );

            // PeerDead does not emit QueueReset (peer can't receive it)
            let extra = pusher.recv_frames_timeout(Duration::from_secs(1)).await;
            assert!(extra.is_none(), "no QueueReset for PeerDead");
        }
        .primary()
        .spawn();

        async move {
            let mut payload = Bytes::from_static(b"hello");
            let written = writer.write_from(&mut payload).await.expect("first write");
            assert_eq!(written, 5);

            bach::task::yield_now().await;

            let mut retry = Bytes::from_static(b"!");
            let err = writer
                .write_from(&mut retry)
                .await
                .expect_err("expected TimedOut");
            assert_eq!(err.kind(), io::ErrorKind::TimedOut);
            assert!(writer.0.status.is_shutdown());
        }
        .primary()
        .spawn();
    });
}

/// UnknownPathSecret completion failure surfaces ConnectionRefused.
#[test]
fn server_unknown_path_secret_completion_surfaces_connection_refused() {
    sim(|| {
        let (mut writer, mut pusher) = make_server_pair();

        async move {
            let frames = pusher.recv_frames().await;
            pusher.complete_all(
                frames,
                frame::TransmissionStatus::Failed(frame::FailureReason::UnknownPathSecret),
            );

            let extra = pusher.recv_frames_timeout(Duration::from_secs(1)).await;
            assert!(extra.is_none(), "no QueueReset for UnknownPathSecret");
        }
        .primary()
        .spawn();

        async move {
            let mut payload = Bytes::from_static(b"hello");
            let written = writer.write_from(&mut payload).await.expect("first write");
            assert_eq!(written, 5);

            bach::task::yield_now().await;

            let mut retry = Bytes::from_static(b"!");
            let err = writer
                .write_from(&mut retry)
                .await
                .expect_err("expected ConnectionRefused");
            assert_eq!(err.kind(), io::ErrorKind::ConnectionRefused);
            assert!(writer.0.status.is_shutdown());
        }
        .primary()
        .spawn();
    });
}

/// After explicit shutdown(), subsequent writes return BrokenPipe (not
/// ConnectionReset — no reset_error_code stored for graceful shutdown).
#[test]
fn client_write_after_shutdown_returns_broken_pipe() {
    sim(|| {
        let (mut writer, mut pusher) = make_server_pair();

        async move {
            // Consume the FIN frame from shutdown
            let _frames = pusher.recv_frames().await;
        }
        .primary()
        .spawn();

        async move {
            writer.shutdown().expect("shutdown should succeed");
            assert!(writer.0.status.is_shutdown());

            let mut payload = Bytes::from_static(b"hello");
            let err = writer
                .write_from(&mut payload)
                .await
                .expect_err("expected BrokenPipe after shutdown");
            assert_eq!(err.kind(), io::ErrorKind::BrokenPipe);
        }
        .primary()
        .spawn();
    });
}

/// The endpoint send credit pool is the sole local send bound: with a pool that holds only 3
/// bytes, the writer sends 3 bytes and then blocks until the simulated admit path releases those
/// credits back to the pool, at which point it can send more.
#[test]
fn server_send_pool_caps_local_write() {
    sim(|| {
        const CAPACITY: u64 = 3;
        let pool = test_credit_pool(CAPACITY);

        // Distributor folds released credits back into `available` and wakes the parked writer.
        let dist = crate::credit::Distributor::new(pool.clone());
        async move {
            use crate::socket::channel::Budget;
            dist.distribute(Budget::new(1 << 20), TestWakerSink).await;
        }
        .spawn();

        let (mut writer, mut pusher) = PairBuilder::server().with_credit_pool(pool.clone()).build();

        let pool_for_pusher = pool.clone();
        async move {
            let first = pusher.recv_frames().await;
            pusher.assembler.push_queue_data(&first);
            pusher.assembler.assert_payload(b"abc");

            // Writer is blocked — the pool is drained and nothing has been admitted yet.
            let extra = pusher.recv_frames_timeout(Duration::from_millis(100)).await;
            assert!(
                extra.is_none(),
                "writer should block when the send pool is exhausted"
            );

            // Simulate the assembler admitting the first batch: release its credits back to the
            // pool. This wakes the parked writer.
            pool_for_pusher.release(sum_flow_credits(&first));

            let second = pusher.recv_frames().await;
            pusher.assembler.push_queue_data(&second);
            pusher.assembler.assert_payload(b"abcdef");
        }
        .primary()
        .spawn();

        async move {
            let mut payload = Bytes::from_static(b"abcdef");
            let first = writer.write_from(&mut payload).await.expect("first write");
            assert_eq!(first, 3, "capped by the send pool capacity");

            let second = writer.write_from(&mut payload).await.expect("second write");
            assert_eq!(second, 3, "released credits allow more data");
        }
        .primary()
        .spawn();
    });
}

/// Calling shutdown() multiple times is idempotent: only one FIN frame emitted.
#[test]
fn server_shutdown_idempotent() {
    sim(|| {
        let (mut writer, mut pusher) = make_server_pair();

        async move {
            let frames = pusher.recv_frames().await;
            assert_eq!(frames.len(), 1);
            assert!(matches!(
                frames.front().unwrap().header,
                Header::QueueData {
                    is_fin: true,
                    offset,
                    ..
                } if offset == VarInt::ZERO
            ));

            // No second FIN frame
            let extra = pusher.recv_frames_timeout(Duration::from_secs(1)).await;
            assert!(
                extra.is_none(),
                "no duplicate FIN from second shutdown call"
            );
        }
        .primary()
        .spawn();

        async move {
            writer.shutdown().expect("first shutdown");
            assert!(writer.0.status.is_shutdown());
            writer.shutdown().expect("second shutdown is no-op");
            assert!(writer.0.status.is_shutdown());
        }
        .primary()
        .spawn();
    });
}

/// Panic drop during QueueBindSent (legacy test, superseded by
/// client_panic_drop_during_queue_init_sent_sends_queue_reset).
/// This verifies that QueueInitReset can be sent even when panicking, and that
/// the frame observable from the test matches expectations.
#[test]
fn client_panic_drop_during_queue_init_sent() {
    sim(|| {
        let (mut writer, mut pusher) = PairBuilder::client_no_remote_queue_id().build();

        async move {
            // QueueInit arrives
            let frames = pusher.recv_frames().await;
            assert_eq!(frames.len(), 1);
            assert!(matches!(
                frames.front().unwrap().header,
                Header::QueueData { is_fin: false, dest_acceptor_id: Some(_), .. }
            ));

            // QueueInitReset arrives after panic
            let reset = pusher
                .recv_frames_timeout(Duration::from_secs(1))
                .await
                .expect("expected QueueReset on panic in QueueBindSent");
            assert_eq!(reset.len(), 1);
            assert!(
                matches!(
                    reset.front().unwrap().header,
                    Header::QueueReset { error_code, .. } if error_code == error::ABNORMAL_TERMINATION
                ),
                "expected QueueReset(ABNORMAL_TERMINATION) with non-sentinel attempt_id"
            );
        }
        .primary()
        .spawn();

        async move {
            let mut payload = Bytes::from_static(b"hello");
            let written = writer.write_from(&mut payload).await.expect("first write");
            assert_eq!(written, 5);
            assert!(writer.0.status.is_init_sent());

            // yield so the pusher receives QueueInit before the panic triggers QueueReset
            bach::task::yield_now().await;

            let panic_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
                let _writer = writer;
                panic!("intentional test panic during QueueBindSent");
            }));
            assert!(panic_result.is_err());
        }
        .primary()
        .spawn();
    });
}

/// Client sends early data in QueueBindSent then requests graceful close via
/// write_from_fin() — the second call sees QueueBindSent and emits QueueInitFin.
///
/// This covers the scenario: client writes some data in the QueueInit (is_fin=false)
/// and then signals the end of the write side before MAX_DATA arrives from the server.
#[test]
fn client_write_from_fin_in_queue_init_sent_sends_queue_data_fin() {
    crate::testing::sim(|| {
        use crate::testing::ext::*;

        let (mut writer, mut pusher) = PairBuilder::client_no_remote_queue_id().build();

        async move {
            // First batch: QueueInit with early data, no FIN
            let frames = pusher.recv_frames().await;
            assert_eq!(frames.len(), 1);
            assert!(
                matches!(
                    frames.front().unwrap().header,
                    Header::QueueData { is_fin: false, dest_acceptor_id: Some(_), .. }
                ),
                "expected QueueInit(is_fin=false) with early data"
            );

            // Second batch: QueueInitFin at the correct write offset
            let fin_frames = pusher
                .recv_frames_timeout(Duration::from_secs(1))
                .await
                .expect("expected QueueData FIN after write_from_fin in QueueBindSent");
            assert_eq!(fin_frames.len(), 1);
            assert!(
                matches!(
                    fin_frames.front().unwrap().header,
                    Header::QueueData { offset, is_fin: true, dest_acceptor_id: Some(_), .. } if offset == VarInt::from_u8(5)
                ),
                "expected QueueData FIN(offset=5) — offset must equal early data length"
            );
        }
        .primary()
        .spawn();

        async move {
            // First write: early data in QueueInit, no FIN
            let mut data = Bytes::from_static(b"hello");
            let n = writer.write_from(&mut data).await.expect("first write");
            assert_eq!(n, 5);
            assert!(writer.0.status.is_init_sent());

            // yield so the pusher can receive QueueInit before we send QueueInitFin
            bach::task::yield_now().await;

            // Graceful close — MAX_DATA not received yet
            let mut empty = Bytes::new();
            let n = writer
                .write_from_fin(&mut empty)
                .await
                .expect("write_from_fin should succeed");
            assert_eq!(n, 0, "no additional payload");
            // FIN was sent; status transitions to FinSent (not Shutdown until drop)
            assert!(writer.0.status.is_fin_sent());
        }
        .primary()
        .spawn();
    });
}

// ─── write_msg tests ───────────────────────────────────────────────────────────
//
// The tests below cover the write_msg API surface:
//   * empty-buffer paths (no-FIN and FIN variants)
//   * small-payload routing to QueueData instead of QueueMsg
//   * flow-control blocking and unblocking via MAX_DATA
//   * error propagation after remote reset, explicit shutdown, and FIN-already-sent
//   * client-side init-state (QueueData-init) and subsequent blocking in InitSent
//   * cooperative yielding: write_msg yields after BUDGET consecutive completions

/// write_msg with an empty buffer and is_fin=false returns 0 without emitting
/// any frame. The caller may call again with actual data later.
#[test]
fn write_msg_empty_buffer_no_fin_returns_zero_without_frames() {
    sim(|| {
        let (mut writer, mut pusher) = make_server_pair();
        writer.0.remote_max_data = VarInt::MAX;

        async move {
            // A drop-triggered FIN is the only frame that should appear.
            let frames = pusher.recv_frames().await;
            assert_eq!(frames.len(), 1);
            assert!(
                matches!(
                    frames.front().unwrap().header,
                    Header::QueueData { is_fin: true, .. }
                ),
                "expected only the drop-FIN frame"
            );
        }
        .primary()
        .spawn();

        async move {
            let mut empty: Bytes = Bytes::new();
            let written = writer
                .write_msg(
                    &mut empty,
                    MsgFlags {
                        is_fin: false,
                        is_wakeup: false,
                    },
                )
                .await
                .expect("write_msg on empty buf should succeed");
            assert_eq!(written, 0);
        }
        .primary()
        .spawn();
    });
}

/// write_msg with an empty buffer and is_fin=true sends a QueueData FIN frame.
#[test]
fn write_msg_empty_buffer_with_fin_emits_fin_frame() {
    sim(|| {
        let (mut writer, mut pusher) = make_server_pair();
        writer.0.remote_max_data = VarInt::MAX;

        async move {
            let frames = pusher.recv_frames().await;
            assert_eq!(frames.len(), 1, "expected exactly one FIN frame");
            let frame = frames.front().unwrap();
            assert!(
                matches!(
                    frame.header,
                    Header::QueueData {
                        is_fin: true,
                        offset,
                        ..
                    } if offset == VarInt::ZERO
                ),
                "expected QueueData FIN at offset 0"
            );
            assert!(
                frame.payload.is_empty(),
                "FIN frame payload should be empty"
            );
        }
        .primary()
        .spawn();

        async move {
            let mut empty: Bytes = Bytes::new();
            let written = writer
                .write_msg(
                    &mut empty,
                    MsgFlags {
                        is_fin: true,
                        is_wakeup: false,
                    },
                )
                .await
                .expect("write_msg with empty buf + FIN should succeed");
            assert_eq!(written, 0);
        }
        .primary()
        .spawn();
    });
}

/// A payload that fits within a single packet_size threshold is routed to a
/// QueueData frame, not a QueueMsg frame. This avoids MsgTable overhead for
/// small messages.
#[test]
fn write_msg_small_payload_routes_to_queue_data_frame() {
    sim(|| {
        let (mut writer, mut pusher) = make_server_pair();
        writer.0.remote_max_data = VarInt::MAX;
        // Any payload at or below packet_size triggers the QueueData fast-path.
        let small_payload = vec![0xABu8; writer.0.packet_size as usize];

        async move {
            let frames = pusher.recv_frames().await;
            assert!(!frames.is_empty(), "expected at least one frame");
            for frame in frames.iter() {
                assert!(
                    matches!(frame.header, Header::QueueData { .. }),
                    "small payload should produce QueueData frames, got {:?}",
                    frame.header
                );
            }
        }
        .primary()
        .spawn();

        async move {
            let mut payload = Bytes::from(small_payload);
            writer
                .write_msg(
                    &mut payload,
                    MsgFlags {
                        is_fin: false,
                        is_wakeup: false,
                    },
                )
                .await
                .expect("write_msg should succeed");
        }
        .primary()
        .spawn();
    });
}

/// write_msg blocks (returns Pending) when the remote flow budget is zero, and
/// unblocks once MAX_DATA arrives from the peer.
#[test]
fn server_write_msg_blocks_when_remote_budget_zero() {
    sim(|| {
        let (mut writer, mut pusher) = make_server_pair();
        // Override to zero so the first write can't proceed.
        writer.0.remote_max_data = VarInt::ZERO;

        async move {
            // No data frame should arrive while budget is zero.
            let early = pusher.recv_frames_timeout(Duration::from_millis(100)).await;
            assert!(
                early
                    .iter()
                    .flat_map(|q| q.iter())
                    .all(|f| matches!(f.header, Header::QueueDataBlocked { .. })),
                "expected no data frame while remote flow budget is zero"
            );

            // Grant budget; the blocked write_msg should now complete.
            pusher.push_max_data(VarInt::from_u16(4096));

            let frames = pusher.recv_frames().await;
            assert!(!frames.is_empty(), "expected frames after MAX_DATA");
        }
        .primary()
        .spawn();

        async move {
            let slot = writer.0.slot_ptr();
            let mut payload = Bytes::from_static(b"hello");

            // First poll must return Pending because remote budget is zero.
            let blocked = core::future::poll_fn(|cx| {
                let mut buf = Bytes::from_static(b"hello");
                match writer.0.poll_write_msg(
                    cx,
                    slot,
                    &mut buf,
                    MsgFlags {
                        is_fin: false,
                        is_wakeup: false,
                    },
                ) {
                    Poll::Pending => Poll::Ready(true),
                    Poll::Ready(_) => Poll::Ready(false),
                }
            })
            .await;
            assert!(blocked, "expected write_msg to block when budget is zero");

            writer
                .write_msg(
                    &mut payload,
                    MsgFlags {
                        is_fin: false,
                        is_wakeup: false,
                    },
                )
                .await
                .expect("write_msg should succeed after MAX_DATA");
        }
        .primary()
        .spawn();
    });
}

/// write_msg blocks when remote budget is exhausted, and resumes after MAX_DATA
/// raises the flow-control window.
#[test]
fn server_write_msg_unblocks_after_max_data() {
    sim(|| {
        let (mut writer, mut pusher) = make_server_pair();
        writer.0.remote_max_data = VarInt::ZERO;

        async move {
            // While budget is 0 no data frame arrives (only possibly a QueueDataBlocked signal).
            let early = pusher.recv_frames_timeout(Duration::from_millis(100)).await;
            assert!(
                early
                    .iter()
                    .flat_map(|q| q.iter())
                    .all(|f| matches!(f.header, Header::QueueDataBlocked { .. })),
                "no data frame expected before MAX_DATA"
            );

            pusher.push_max_data(VarInt::from_u16(8192));

            let frames = pusher.recv_frames().await;
            assert!(
                !frames.is_empty(),
                "expected frames after MAX_DATA unblocks write_msg"
            );
        }
        .primary()
        .spawn();

        async move {
            let mut payload = Bytes::from_static(b"unblocked");
            writer
                .write_msg(
                    &mut payload,
                    MsgFlags {
                        is_fin: false,
                        is_wakeup: false,
                    },
                )
                .await
                .expect("write_msg should complete after MAX_DATA");
        }
        .primary()
        .spawn();
    });
}

/// A write into a zero remote window with no data frame to carry the in-band `blocked` bit emits a
/// standalone `QueueDataBlocked` frame carrying the desired high-water offset, and re-emits only
/// when that watermark grows (dedup on `last_blocked_offset`).
#[test]
fn write_from_blocked_emits_queue_data_blocked() {
    sim(|| {
        let (mut writer, mut pusher) = make_server_pair();
        writer.0.remote_max_data = VarInt::ZERO;

        async move {
            let frames = pusher.recv_frames().await;
            let mut blocked = frames
                .iter()
                .filter_map(|f| match f.header {
                    Header::QueueDataBlocked { desired_offset, .. } => Some(desired_offset),
                    _ => None,
                })
                .peekable();
            assert!(
                blocked.peek().is_some(),
                "expected a QueueDataBlocked frame, got {:?}",
                frames.iter().map(|f| &f.header).collect::<Vec<_>>()
            );
            // The desired offset is the writer's high watermark (5 bytes buffered from offset 0).
            assert_eq!(blocked.next(), Some(VarInt::from_u8(5)));
        }
        .primary()
        .spawn();

        async move {
            let slot = writer.0.slot_ptr();
            // Poll twice with the same buffered data: the first emits a blocked frame, the second
            // must not re-emit (same high watermark → deduped).
            for _ in 0..2 {
                let pending = core::future::poll_fn(|cx| {
                    let mut buf = Bytes::from_static(b"hello");
                    match writer.0.poll_write_from(cx, slot, &mut buf, false) {
                        Poll::Pending => Poll::Ready(true),
                        Poll::Ready(_) => Poll::Ready(false),
                    }
                })
                .await;
                assert!(pending, "write must block while remote window is zero");
            }
            // Keep the writer alive so the pusher observes the frame before drop.
            10.ms().sleep().await;
        }
        .primary()
        .spawn();
    });
}

/// write_msg returns ConnectionReset when a remote Reset arrives before the
/// write can complete.
#[test]
fn write_msg_after_remote_reset_returns_connection_reset() {
    sim(|| {
        let (mut writer, mut pusher) = make_server_pair();
        writer.0.remote_max_data = VarInt::ZERO;

        async move {
            pusher.push_reset(VarInt::from_u8(42));
        }
        .primary()
        .spawn();

        async move {
            let mut payload = Bytes::from_static(b"data");
            let err = writer
                .write_msg(
                    &mut payload,
                    MsgFlags {
                        is_fin: false,
                        is_wakeup: false,
                    },
                )
                .await
                .expect_err("expected ConnectionReset");
            assert_eq!(
                err.kind(),
                io::ErrorKind::ConnectionReset,
                "expected ConnectionReset, got {err:?}"
            );
        }
        .primary()
        .spawn();
    });
}

/// write_msg returns BrokenPipe after the writer has been explicitly shut down.
#[test]
fn write_msg_after_shutdown_returns_broken_pipe() {
    sim(|| {
        let (mut writer, pusher) = make_server_pair();

        async move {
            // Keep the pusher alive so the frame channel stays open; shutdown()
            // sends a FIN frame and would fail with "frame channel closed" if
            // the receiver were dropped before the task runs.
            let _pusher = pusher;
            writer.shutdown().expect("shutdown should succeed");
            let mut payload = Bytes::from_static(b"data");
            let err = writer
                .write_msg(
                    &mut payload,
                    MsgFlags {
                        is_fin: false,
                        is_wakeup: false,
                    },
                )
                .await
                .expect_err("expected BrokenPipe after shutdown");
            assert_eq!(
                err.kind(),
                io::ErrorKind::BrokenPipe,
                "expected BrokenPipe after shutdown, got {err:?}"
            );
        }
        .primary()
        .spawn();
    });
}

/// write_msg returns BrokenPipe after a FIN has already been sent.
#[test]
fn write_msg_after_fin_sent_returns_broken_pipe() {
    sim(|| {
        let (mut writer, _pusher) = make_server_pair();

        async move {
            // Force FinSent state by marking it directly.
            writer.0.status.on_send_fin().ok();
            assert!(writer.0.status.is_fin_sent());

            let mut payload = Bytes::from_static(b"data");
            let err = writer
                .write_msg(
                    &mut payload,
                    MsgFlags {
                        is_fin: false,
                        is_wakeup: false,
                    },
                )
                .await
                .expect_err("expected BrokenPipe after fin sent");
            assert_eq!(
                err.kind(),
                io::ErrorKind::BrokenPipe,
                "expected BrokenPipe after FinSent, got {err:?}"
            );
        }
        .primary()
        .spawn();
    });
}

/// Client write_msg sends a QueueData-init frame on the first write (the
/// bootstrap message that lets the server know which acceptor queue to use and
/// triggers it to send MAX_DATA), then blocks on any subsequent write while in
/// InitSent waiting for the server to confirm the stream.
///
/// Crucially, the client starts with `remote_max_data = VarInt::ZERO` (zero
/// flow-control window), yet the first write still completes immediately — the
/// init packet is always sent unconditionally so the stream can be established.
/// See also `client_write_msg_zero_window_does_not_block_init`.
#[test]
fn client_write_msg_first_write_sends_queue_data_init_then_blocks() {
    sim(|| {
        let (mut writer, mut pusher) = make_client_pair();
        // Clients always start with a zero remote window — the init frame must
        // bypass this restriction.
        assert_eq!(
            writer.0.remote_max_data,
            VarInt::ZERO,
            "client must start with zero remote budget"
        );

        async move {
            // The bootstrap init frame(s) must arrive.
            let frames = pusher.recv_frames().await;
            let has_init = frames.iter().any(|f| {
                matches!(
                    f.header,
                    Header::QueueData {
                        dest_acceptor_id: Some(_),
                        ..
                    } | Header::QueueMsg {
                        dest_acceptor_id: Some(_),
                        ..
                    }
                )
            });
            assert!(
                has_init,
                "expected at least one init frame with dest_acceptor_id"
            );

            // No further frames — writer is blocked in InitSent until MAX_DATA.
            let extra = pusher.recv_frames_timeout(Duration::from_millis(100)).await;
            assert!(
                extra.is_none(),
                "expected no further frames while in InitSent"
            );
        }
        .primary()
        .spawn();

        async move {
            let slot = writer.0.slot_ptr();
            // Small payload (≤ packet_size) → send_queue_data_init fast-path.
            // The write completes immediately despite the zero window because the
            // init frame is always sent unconditionally.
            let payload_len = writer.0.packet_size as usize;
            let mut buf = Data::new(payload_len as u64);
            let result = core::future::poll_fn(|cx| {
                writer.0.poll_write_msg(
                    cx,
                    slot,
                    &mut buf,
                    MsgFlags {
                        is_fin: false,
                        is_wakeup: false,
                    },
                )
            })
            .await;
            assert!(result.is_ok(), "first write should succeed: {result:?}");
            assert!(
                writer.0.status.is_init_sent(),
                "writer should be in InitSent after first write"
            );

            // A second write call must block: we are now in InitSent, waiting
            // for the server's MAX_DATA before more data can be sent.
            let blocked = core::future::poll_fn(|cx| {
                let mut buf2 = Data::new(1);
                match writer.0.poll_write_msg(
                    cx,
                    slot,
                    &mut buf2,
                    MsgFlags {
                        is_fin: false,
                        is_wakeup: false,
                    },
                ) {
                    Poll::Pending => Poll::Ready(true),
                    Poll::Ready(_) => Poll::Ready(false),
                }
            })
            .await;
            assert!(blocked, "second write_msg should block while in InitSent");
        }
        .primary()
        .spawn();
    });
}

/// Client write_msg does NOT block on the first write even when the remote
/// flow-control window is zero.
///
/// Servers correctly block when their budget is zero (see
/// `server_write_msg_blocks_when_remote_budget_zero`), but clients in the Init
/// state must always be able to send the bootstrap packet that establishes the
/// stream and triggers the server to open a flow-control window.
#[test]
fn client_write_msg_zero_window_does_not_block_init() {
    sim(|| {
        let (mut writer, mut pusher) = make_client_pair();
        // Clients always start with zero remote budget.
        assert_eq!(writer.0.remote_max_data, VarInt::ZERO);

        async move {
            // The init frame must arrive despite the zero window.
            let frames = pusher.recv_frames().await;
            assert!(
                !frames.is_empty(),
                "client must send init frame even with zero window"
            );
            let has_init = frames.iter().any(|f| {
                matches!(
                    f.header,
                    Header::QueueData {
                        dest_acceptor_id: Some(_),
                        ..
                    } | Header::QueueMsg {
                        dest_acceptor_id: Some(_),
                        ..
                    }
                )
            });
            assert!(has_init, "init frame must carry dest_acceptor_id");
        }
        .primary()
        .spawn();

        async move {
            let slot = writer.0.slot_ptr();
            let mut payload = Bytes::from_static(b"hello");
            // First write should complete (not block) even with zero window.
            let result = core::future::poll_fn(|cx| {
                writer.0.poll_write_msg(
                    cx,
                    slot,
                    &mut payload,
                    MsgFlags {
                        is_fin: false,
                        is_wakeup: false,
                    },
                )
            })
            .await;
            assert!(
                result.is_ok(),
                "client first write must succeed despite zero window: {result:?}"
            );
        }
        .primary()
        .spawn();
    });
}

/// write_msg yields cooperatively when the inner send_msg chunk loop exhausts the per-frame coop
/// budget, giving other tasks repeated chances to run even under continuous write pressure with
/// unlimited credits. A single payload spanning many budgets must yield many times, so a competing
/// task that runs once per yield is observed to run multiple times *during* the write.
#[test]
fn write_msg_coop_yields_after_budget_completions() {
    // The multi-budget payload emits thousands of per-frame trace lines — unreasonably large to
    // snapshot; the interleaving assertion is the regression signal.
    let _guard = crate::testing::without_snapshots();
    sim(|| {
        let (mut writer, mut pusher) = make_server_pair();
        writer.0.remote_max_data = VarInt::MAX;

        // Counts how many times the competing task was scheduled. With per-frame coop yielding the
        // writer hands control back repeatedly mid-message, so this climbs well above 1.
        let runs = Arc::new(AtomicUsize::new(0));
        let runs_clone = runs.clone();
        let done = Arc::new(AtomicBool::new(false));
        let done_clone = done.clone();

        let drain_done = done.clone();

        // Competing task: tick once per scheduling, until the writer signals completion.
        async move {
            while !done_clone.load(Ordering::Relaxed) {
                runs_clone.fetch_add(1, Ordering::Relaxed);
                bach::task::yield_now().await;
            }
        }
        .spawn();

        // Frame-drainer task: keeps the frame channel from filling up. Bounded by `done` so the
        // sim can terminate once the writer finishes (a perpetual self-waking task would otherwise
        // keep bach runnable forever and spin wall-clock).
        async move {
            while !drain_done.load(Ordering::Relaxed) {
                let _ = pusher.recv_frames_timeout(Duration::from_millis(1)).await;
            }
        }
        .spawn();

        async move {
            // Payload spanning many coop budgets so the inner send_msg chunk loop yields
            // repeatedly. Sized in whole segments to keep the framing tidy; `Data` is a virtual
            // generator so no real allocation occurs.
            let chunk_size = writer.0.msg_packet_size as usize;
            let max_segment_size = crate::queue::msg_entry::MAX_CHUNKS as usize * chunk_size;
            let payload_len = max_segment_size * 4;
            let mut payload = Data::new(payload_len as u64);
            writer
                .write_msg(
                    &mut payload,
                    MsgFlags {
                        is_fin: true,
                        is_wakeup: false,
                    },
                )
                .await
                .expect("write_msg should succeed");
            done.store(true, Ordering::Relaxed);
            // The write spanned many budgets (payload >> BUDGET frames), so the competing task must
            // have been scheduled multiple times mid-write — proving repeated cooperative yields.
            assert!(
                runs.load(Ordering::Relaxed) > 1,
                "competing task should run repeatedly during a multi-budget write_msg coop yield, ran {} time(s)",
                runs.load(Ordering::Relaxed),
            );
        }
        .primary()
        .spawn();
    });
}

/// send_data (the `write_from` path) yields cooperatively when its per-frame coop budget is
/// exhausted under a large buffer with effectively unlimited credit, and still transmits every
/// byte. Mirrors `write_msg_coop_yields_after_budget_completions` for the QueueData path.
#[test]
fn send_data_yields_under_large_buffer() {
    // >BUDGET QueueData frames emit too many per-frame trace lines to snapshot reasonably.
    let _guard = crate::testing::without_snapshots();
    sim(|| {
        let (mut writer, mut pusher) = make_server_pair();
        writer.0.remote_max_data = VarInt::MAX;

        let runs = Arc::new(AtomicUsize::new(0));
        let runs_clone = runs.clone();
        let done = Arc::new(AtomicBool::new(false));
        let done_clone = done.clone();

        let drain_done = done.clone();

        async move {
            while !done_clone.load(Ordering::Relaxed) {
                runs_clone.fetch_add(1, Ordering::Relaxed);
                bach::task::yield_now().await;
            }
        }
        .spawn();

        // Bounded drainer (see the write_msg variant for why a perpetual loop spins wall-clock).
        async move {
            while !drain_done.load(Ordering::Relaxed) {
                let _ = pusher.recv_frames_timeout(Duration::from_millis(1)).await;
            }
        }
        .spawn();

        async move {
            // Several budgets' worth of MTU-sized QueueData frames so send_data's per-frame coop
            // break fires repeatedly across polls.
            let mtu = writer.0.packet_size as usize;
            let budget = crate::stream::coop::BUDGET as usize;
            let total = mtu * budget * 4;
            use s2n_quic_core::buffer::reader::Storage as _;
            let mut payload = Data::new(total as u64);
            let mut written = 0usize;
            while !payload.buffer_is_empty() {
                written += writer
                    .write_from(&mut payload)
                    .await
                    .expect("write_from should succeed");
            }
            done.store(true, Ordering::Relaxed);
            assert_eq!(written, total, "every buffered byte must be transmitted");
            assert!(
                runs.load(Ordering::Relaxed) > 1,
                "competing task should run repeatedly during a multi-budget send_data coop yield, ran {} time(s)",
                runs.load(Ordering::Relaxed),
            );
        }
        .primary()
        .spawn();
    });
}

/// BUG REPRODUCTION: Client write_msg with a payload just above packet_size
/// splits into a 2-chunk QueueMsg segment. During Init (force_first=true),
/// only chunk 0 is sent. After MAX_DATA unblocks the writer, `send_msg` is
/// called with the remaining bytes. If those remaining bytes fit within
/// packet_size, the size-based routing early-return at the top of `send_msg`
/// routes them to `send_data` (QueueData) instead of resuming the pending
/// QueueMsg segment. The receiver's MsgTable entry never completes because
/// chunk 1 never arrives via QueueMsg — the data arrives via QueueData at
/// the wrong offset, and the MsgTable is permanently stuck.
#[test]
fn client_write_msg_partial_segment_resume_must_use_queue_msg_not_queue_data() {
    sim(|| {
        let (mut writer, mut pusher) = make_client_pair();
        // Payload just above packet_size so it takes the QueueMsg path but
        // produces exactly 2 chunks: first chunk = msg_packet_size, second chunk
        // = packet_size + 1 - msg_packet_size (which is < packet_size).
        let payload_len = writer.0.packet_size as usize + 1;

        async move {
            // First batch: the init bootstrap frame(s). Should contain at least
            // one QueueMsg with chunk_index=0.
            let frames = pusher.recv_frames().await;
            let mut saw_queue_msg_chunk_0 = false;
            for frame in frames.iter() {
                match frame.header {
                    Header::QueueMsg {
                        chunk_index,
                        dest_acceptor_id: Some(_),
                        ..
                    } => {
                        assert_eq!(
                            chunk_index.as_u64(),
                            0,
                            "init should only send chunk_index=0"
                        );
                        saw_queue_msg_chunk_0 = true;
                    }
                    Header::QueueData {
                        dest_acceptor_id: Some(_),
                        ..
                    } => {
                        // Small-payload init path — this test requires a
                        // payload above packet_size, so this shouldn't happen.
                        panic!("expected QueueMsg init (payload > packet_size), got QueueData");
                    }
                    _ => panic!("unexpected frame during init: {:?}", frame.header),
                }
            }
            assert!(
                saw_queue_msg_chunk_0,
                "expected QueueMsg chunk_index=0 during init"
            );

            // Grant MAX_DATA so the writer can resume the pending segment.
            pusher.push_max_data(VarInt::from_u16(4096));

            // Second batch: the resumed segment. This MUST contain QueueMsg
            // chunk_index=1, NOT a QueueData frame.
            let resume_frames = pusher
                .recv_frames_timeout(Duration::from_secs(5))
                .await
                .expect("expected resumed frames after MAX_DATA");

            let mut saw_queue_msg_chunk_1 = false;
            for frame in resume_frames.iter() {
                match frame.header {
                    Header::QueueMsg { chunk_index, .. } => {
                        assert_eq!(
                            chunk_index.as_u64(),
                            1,
                            "resumed segment should send chunk_index=1"
                        );
                        saw_queue_msg_chunk_1 = true;
                    }
                    Header::QueueData { .. } => {
                        panic!(
                            "BUG: remaining bytes routed to QueueData instead of \
                             resuming QueueMsg segment (pending_chunk_index was ignored)"
                        );
                    }
                    _ => {}
                }
            }
            assert!(
                saw_queue_msg_chunk_1,
                "expected QueueMsg chunk_index=1 after resume, but it never arrived"
            );
        }
        .primary()
        .spawn();

        async move {
            let mut payload = Data::new(payload_len as u64);
            writer
                .write_msg(
                    &mut payload,
                    MsgFlags {
                        is_fin: true,
                        is_wakeup: true,
                    },
                )
                .await
                .expect("write_msg should succeed");
        }
        .primary()
        .spawn();
    });
}

/// Regression: resuming a partial QueueMsg segment must never `take_credits` more than the
/// writer currently holds.
///
/// The init path (`force_first`) sends only chunk 0 of its first segment, leaving a multi-chunk
/// pending segment. On resume the two segment-level credit guards in `send_msg`
/// (segment-fits-in-remote-budget and pending_credits >= chunk_size) are both skipped because
/// `is_resuming` is true, and the inner chunk loop calls `take_credits(chunk_len)` for each
/// remaining chunk. A single pool acquire only tops `pending_credits` up by `max_single_acquire`,
/// so when the pending segment spans more bytes than one acquire delivers, the loop drains the
/// held credit below one chunk and the next `take_credits` underflows (debug panic; release builds
/// silently stamp a too-small `flow_credits`, corrupting the receiver's flow accounting).
///
/// This is the residual of the production `Config::new(2 MiB)` regression: even once the
/// per-acquire cap is floored at one chunk (so the writer makes forward progress), a segment
/// larger than the cap still needs several acquires, and the resume loop must re-check held credit
/// per chunk. Here the cap is pinned to exactly one chunk so each poll sends exactly one chunk —
/// liveness is preserved while the multi-acquire underflow is exercised.
#[test]
fn write_msg_resume_does_not_underflow_credits_when_pool_cap_below_chunk() {
    sim(|| {
        // Build the pair first so we can read the negotiated chunk size, then pin the pool's
        // per-acquire cap to exactly one chunk: a single acquire delivers one chunk's worth of
        // credit, so a multi-chunk resume segment requires several acquires.
        let chunk_size = {
            let (probe, _) = PairBuilder::default().build();
            probe.0.msg_packet_size as u64
        };
        let pool = crate::sync::Arc::new(crate::credit::Pool::new(
            crate::credit::Config::new(2 * 1024 * 1024).with_max_single_acquire_uniform(chunk_size),
        ));
        let (mut writer, mut pusher) = PairBuilder::default()
            .with_credit_pool(pool.clone())
            .build();
        assert_eq!(
            pool.max_single_acquire(crate::credit::Priority::default()),
            writer.0.msg_packet_size as u64,
            "test precondition: per-acquire cap is one chunk, so a multi-chunk segment needs \
             several acquires and the resume loop must re-check held credit per chunk"
        );

        // Distributor folds released/parked credit back into the pool and wakes the parked writer
        // so the resume poll actually runs.
        let dist = crate::credit::Distributor::new(pool.clone());
        async move {
            use crate::socket::channel::Budget;
            dist.distribute(Budget::new(1 << 20), TestWakerSink).await;
        }
        .spawn();

        // Several full chunks in one segment: init (force_first) sends only chunk 0 and parks the
        // rest as a pending segment that spans multiple chunks. Resuming it requires more credit
        // than one acquire delivers, so the writer sends one chunk per poll and re-acquires
        // between chunks. Before the per-chunk guard, the resume loop kept taking full chunks past
        // the credit it held and underflowed `take_credits`.
        let chunk_bytes = writer.0.msg_packet_size as usize;
        let payload_len = 4 * chunk_bytes;
        let pool_for_pusher = pool.clone();

        async move {
            // Init bootstrap batch (chunk 0) — drain it and release its credits to simulate
            // admission, then grant a large remote window so the writer confirms (InitSent→Open)
            // and the resume is gated only by held credit, not the remote budget.
            let init = pusher.recv_frames().await;
            let mut total_payload: usize = init.iter().map(|f| f.payload.len()).sum();
            pool_for_pusher.release(sum_flow_credits(&init));
            pusher.push_max_data(VarInt::from_u32(1 << 20));

            // Drain the remaining chunks batch-by-batch, releasing each batch's credits to simulate
            // the assembler admitting them. Each release lets the distributor grant the next
            // chunk's worth of credit (the cap is one chunk), waking the parked writer. Loop until
            // the whole message has been received.
            while total_payload < payload_len {
                let batch = pusher
                    .recv_frames_timeout(Duration::from_secs(5))
                    .await
                    .expect("writer should keep making progress, one chunk per acquire");
                for frame in batch.iter() {
                    total_payload += frame.payload.len();
                    // Every chunk must carry credits equal to its payload — never a value
                    // truncated by a saturating underflow.
                    assert_eq!(
                        frame.flow_credits,
                        frame.payload.len() as u64,
                        "each chunk must carry credits equal to its payload size"
                    );
                }
                pool_for_pusher.release(sum_flow_credits(&batch));
            }
            assert_eq!(
                total_payload, payload_len,
                "all payload bytes must be transmitted across the resumed chunks"
            );
        }
        .primary()
        .spawn();

        async move {
            let mut payload = Data::new(payload_len as u64);
            writer
                .write_msg(
                    &mut payload,
                    MsgFlags {
                        is_fin: true,
                        is_wakeup: true,
                    },
                )
                .await
                .expect("write_msg should succeed without credit underflow");
        }
        .primary()
        .spawn();
    });
}

/// write_msg with FIN on a large multi-segment payload marks FIN only on the
/// last chunk of the last segment, not on intermediate segments.
///
/// This complements `write_msg_large_payload_uses_multiple_msg_segments` by
/// focusing on is_fin placement rather than segment structure.
#[test]
fn write_msg_fin_only_on_last_chunk_of_last_segment() {
    sim(|| {
        let (mut writer, mut pusher) = make_server_pair();
        writer.0.remote_max_data = VarInt::MAX;
        let chunk_size = writer.0.msg_packet_size as usize;
        // Two full segments: first has MAX_CHUNKS chunks, second has one extra.
        let first_segment = crate::queue::msg_entry::MAX_CHUNKS as usize * chunk_size;
        let second_segment = chunk_size;
        let payload_len = first_segment + second_segment;

        async move {
            let frames = pusher.recv_frames_until_fin().await;
            assert!(!frames.is_empty());

            let mut fin_count = 0usize;
            let mut non_fin_before_fin = false;
            let mut last_was_fin = false;

            // Per-frame coop yielding can split this message across polls, so the small final
            // segment may be re-routed through the QueueData fast path (the buffer holds a single
            // chunk on a fresh poll and `pending_chunk_index == 0`). Either framing is correct —
            // assert only on FIN placement: exactly one FIN, nothing after it, non-FIN frames
            // before it.
            for frame in frames.iter() {
                let is_fin = match frame.header {
                    Header::QueueMsg { is_fin, .. } | Header::QueueData { is_fin, .. } => is_fin,
                    _ => panic!("expected QueueMsg or QueueData, got {:?}", frame.header),
                };
                if last_was_fin {
                    panic!("frame after FIN: {:?}", frame.header);
                }
                if is_fin {
                    fin_count += 1;
                    last_was_fin = true;
                } else {
                    non_fin_before_fin = true;
                }
            }

            assert_eq!(fin_count, 1, "exactly one FIN-bearing chunk expected");
            assert!(
                non_fin_before_fin,
                "expected non-FIN chunks before the FIN chunk"
            );
        }
        .primary()
        .spawn();

        async move {
            let mut payload = Data::new(payload_len as u64);
            writer
                .write_msg(
                    &mut payload,
                    MsgFlags {
                        is_fin: true,
                        is_wakeup: false,
                    },
                )
                .await
                .expect("write_msg with fin should succeed");
        }
        .primary()
        .spawn();
    });
}

// ── Credit-pool integration ─────────────────────────────────────────────────

/// Build a credit pool with a custom capacity. Per-priority caps default to capacity so a single
/// acquire can drain the whole pool.
fn test_credit_pool(capacity: u64) -> crate::sync::Arc<crate::credit::Pool> {
    crate::sync::Arc::new(crate::credit::Pool::new(crate::credit::Config {
        capacity,
        max_single_acquire: [capacity; crate::credit::Priority::LEVELS],
        // Floor == cap so a single acquire can drain the whole pool (no fair-share split).
        min_grant_slice: [capacity; crate::credit::Priority::LEVELS],
    }))
}

/// Sum the `flow_credits` field across every frame in `queue`.
fn sum_flow_credits(queue: &intrusive::Queue<Frame>) -> u64 {
    queue.iter().map(|f| f.flow_credits).sum()
}

/// Steady-state: every byte the writer sends is attributed to a `Frame.flow_credits` field
/// equal to its payload size. The test simulates the production assembler by releasing each
/// admitted frame's credits back to the pool, then asserts that the pool counters reconcile:
/// `acquire_bytes == release_bytes` across the lifecycle.
#[test]
fn credit_pool_round_trip_attaches_credits_and_admit_restores_capacity() {
    sim(|| {
        const CAPACITY: u64 = 16 * 1024;
        let pool = test_credit_pool(CAPACITY);
        let (mut writer, mut pusher) = PairBuilder::default()
            .with_credit_pool(pool.clone())
            .build();

        // Distributor folds `returned` back into `available` when it polls; spawning it lets us
        // assert against `debug_available` after a few yield points.
        let dist = crate::credit::Distributor::new(pool.clone());
        async move {
            use crate::socket::channel::Budget;
            dist.distribute(Budget::new(1 << 20), TestWakerSink).await;
        }
        .spawn();

        let pool_for_pusher = pool.clone();
        async move {
            // Init frame round-trip: payload is bounded by packet_size.
            let init = pusher.recv_frames().await;
            assert_eq!(init.len(), 1, "expected single init frame");
            let init_credits = sum_flow_credits(&init);
            assert_eq!(
                init_credits, 5,
                "init frame should carry exactly the bytes it transports"
            );
            // Simulate the assembler admitting the frame: release its credits back to the pool.
            pool_for_pusher.release(init_credits);

            // Steady-state writes after the server's MAX_DATA: each frame's flow_credits matches
            // its payload size; admit them all by releasing.
            pusher.push_max_data(VarInt::from_u32(8 * 1024));
            let steady = pusher.recv_frames().await;
            assert!(!steady.is_empty(), "expected steady-state frames");
            for frame in steady.iter() {
                assert_eq!(
                    frame.flow_credits,
                    frame.payload.len() as u64,
                    "every data frame must carry credits equal to its payload size"
                );
            }
            pool_for_pusher.release(sum_flow_credits(&steady));
        }
        .primary()
        .spawn();

        let pool_for_writer = pool.clone();
        async move {
            let mut hello = Bytes::from_static(b"hello");
            writer
                .write_from(&mut hello)
                .await
                .expect("init write succeeds");

            let mut bulk = Data::new(8 * 1024);
            writer.write_from(&mut bulk).await.expect("bulk write");

            drop(writer);

            // Yield so the distributor folds the released credits back into `available`.
            for _ in 0..4 {
                bach::task::yield_now().await;
            }

            // All credits acquired by the writer have been either (a) released by the simulated
            // admit path on the Pusher side, or (b) released by `WriterAllocPtr::drop` for any
            // bytes the writer was still holding when it went away.
            assert_eq!(
                pool_for_writer.debug_available(),
                CAPACITY as i64,
                "acquire/release must net to zero across the lifecycle"
            );
        }
        .primary()
        .spawn();
    });
}

/// A second poll on a writer whose pool is exhausted parks. After the pool is replenished the
/// distributor wakes the slot and the next poll proceeds. Covers the LINKED→APP grant path.
#[test]
fn credit_pool_parked_write_unblocks_after_release() {
    sim(|| {
        // Capacity holds exactly one bulk write. The first bulk drains the pool; the second
        // parks until the pusher simulates an admit by releasing credits.
        const CAPACITY: u64 = 128;
        let pool = test_credit_pool(CAPACITY);

        // Distributor task — runs in the background, must not block runtime termination.
        let dist = crate::credit::Distributor::new(pool.clone());
        async move {
            use crate::socket::channel::Budget;
            dist.distribute(Budget::new(1 << 20), TestWakerSink).await;
        }
        .spawn();

        let (mut writer, mut pusher) = PairBuilder::default()
            .with_credit_pool(pool.clone())
            .build();

        let pool_for_pusher = pool.clone();
        async move {
            // Init frame: simulate the assembler admitting it (release its credits).
            let init = pusher.recv_frames().await;
            pool_for_pusher.release(sum_flow_credits(&init));

            // Open the steady-state window.
            pusher.push_max_data(VarInt::from_u32(4 * CAPACITY as u32));

            // First bulk arrives but stays in the channel — admit only when we want to wake the
            // parked second bulk.
            let first = pusher.recv_frames().await;
            assert!(!first.is_empty(), "first bulk should produce frames");

            // Pool is now drained (debited by `first`'s flow_credits, not yet released). Release
            // them — this wakes the parked writer task.
            pool_for_pusher.release(sum_flow_credits(&first));

            let second = pusher.recv_frames().await;
            assert!(!second.is_empty(), "second batch must arrive after release");
        }
        .primary()
        .spawn();

        async move {
            let mut first = Bytes::from_static(b"hi");
            writer.write_from(&mut first).await.expect("init write");

            // First bulk: takes the entire steady-state capacity.
            let mut bulk = Data::new(CAPACITY);
            writer
                .write_from(&mut bulk)
                .await
                .expect("first bulk write");

            // Second bulk: parks because the pool is drained until the pusher releases.
            let mut more = Data::new(CAPACITY);
            writer
                .write_from(&mut more)
                .await
                .expect("second bulk write completes after release");
        }
        .primary()
        .spawn();
    });
}

/// Dropping a writer while its credit slot is parked must transfer ownership of the slot
/// allocation to the pool via `Slot::abandon`. The pool reclaims the slot on its next pass and
/// invokes `drop_writer_alloc` to free the heap. This exercises the LINKED→DEAD branch of
/// `WriterAllocPtr::drop`.
#[test]
fn parked_writer_drop_transfers_ownership_to_pool() {
    sim(|| {
        // Capacity = 2 covers the init's "hi" exactly; any subsequent acquire parks because the
        // pool is drained.
        const CAPACITY: u64 = 2;
        let pool = test_credit_pool(CAPACITY);

        let dist = crate::credit::Distributor::new(pool.clone());
        async move {
            use crate::socket::channel::Budget;
            dist.distribute(Budget::new(1 << 20), TestWakerSink).await;
        }
        .spawn();

        let (mut writer, mut pusher) = PairBuilder::default()
            .with_credit_pool(pool.clone())
            .build();

        async move {
            let _init = pusher.recv_frames().await;
            // Open the steady-state window so the writer's next acquire isn't gated by
            // `min_send_budget==0`. We do NOT release the init credits — the pool stays drained.
            pusher.push_max_data(VarInt::from_u32(1024));
            // Hold the pusher so its task keeps running while the writer parks and then drops.
            // Without this, the pusher task ends and the runtime tears down before the writer
            // gets a chance to park.
            for _ in 0..8 {
                bach::task::yield_now().await;
            }
        }
        .primary()
        .spawn();

        async move {
            let mut hello = Bytes::from_static(b"hi");
            writer.write_from(&mut hello).await.expect("init write");

            // Yield so the pusher's `push_max_data` lands and the writer's status becomes Open.
            for _ in 0..2 {
                bach::task::yield_now().await;
            }

            // The pool has zero credits available now (init took both bytes and the pusher
            // didn't release them). The next write parks the slot.
            let mut bulk = Data::new(64);
            let parked =
                core::future::poll_fn(|cx| match writer.poll_write_from(cx, &mut bulk, false) {
                    Poll::Pending => Poll::Ready(true),
                    Poll::Ready(_) => Poll::Ready(false),
                })
                .await;
            assert!(parked, "expected the bulk write to park on credit acquire");

            // Drop the writer while its slot is LINKED. `Slot::abandon` succeeds; the
            // distributor's next pass observes the dead slot and frees the allocation via
            // `drop_writer_alloc`. The test's success criterion is "no leak/panic at drop"; the
            // bach runtime tears down cleanly because the distributor handles the dead slot.
            drop(writer);
        }
        .primary()
        .spawn();
    });
}

/// Dropping a writer that is parked on a credit acquire while still **holding** unconsumed
/// `pending_credits` must release that credit back to the pool. This is the leak regression for
/// `drop_writer_alloc`: that path runs only on the LINKED→DEAD abandon branch (writer dropped while
/// parked), where the pool — not the app — frees the allocation, so it is the only place the held
/// remainder can be returned.
///
/// Construction: the per-acquire cap is one chunk **plus a sub-chunk remainder**. The msg path
/// frames the one whole chunk it can, leaving the remainder in `pending_credits`; since the
/// remainder is below one chunk (`msg_progress_floor`), the resume loop re-acquires for the rest and
/// parks — while still holding the remainder. Dropping there exercises the leak path. The test
/// asserts the pool's `available` returns to the full capacity once the distributor reaps the dead
/// slot; before the fix the remainder stayed permanently debited.
#[test]
fn parked_writer_drop_releases_held_pending_credits() {
    let _no_snap = crate::testing::without_snapshots();
    sim(|| {
        let chunk_size = {
            let (probe, _) = PairBuilder::default().build();
            probe.0.msg_packet_size as u64
        };
        // Per-acquire cap = one chunk + a sub-chunk remainder. Each acquire delivers
        // `chunk + remainder` credit; the msg path frames the one whole chunk, leaving `remainder`
        // in `pending_credits`. Because `remainder < chunk` (the msg progress floor), the resume
        // re-acquires for the next chunk — and with the pool drained that acquire parks while the
        // writer is still holding `remainder`. That is the state the leak fix must clean up.
        let remainder = chunk_size / 2;
        let per_acquire = chunk_size + remainder;
        // Capacity == one acquire's worth: a single acquire drains the pool, so the very next
        // acquire parks. Small enough that even after the pusher admits the emitted chunk, the
        // freed credit (`chunk`) is below the parked acquire's need (`chunk + remainder`), so the
        // writer stays parked rather than being re-granted.
        let capacity = per_acquire;
        let pool = crate::sync::Arc::new(crate::credit::Pool::new(crate::credit::Config {
            capacity,
            max_single_acquire: [per_acquire; crate::credit::Priority::LEVELS],
            min_grant_slice: [per_acquire; crate::credit::Priority::LEVELS],
        }));
        let (mut writer, mut pusher) = PairBuilder::default()
            .with_credit_pool(pool.clone())
            .build();

        let dist = crate::credit::Distributor::new(pool.clone());
        async move {
            use crate::socket::channel::Budget;
            dist.distribute(Budget::new(1 << 20), TestWakerSink).await;
        }
        .spawn();

        // Multi-chunk message so the writer must acquire more than once: the init sends chunk 0 and
        // parks the rest as a pending multi-chunk segment that the resume loop drives.
        let chunk_bytes = writer.0.msg_packet_size as usize;
        let payload_len = 4 * chunk_bytes;

        let dropped = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));

        // ── Pusher: collect emitted credit, but release it only AFTER the drop ──
        // Holding releases until after the drop serves two purposes:
        //   1. The pool stays drained, so the writer parks early (holding its sub-chunk remainder)
        //      instead of being continuously re-granted.
        //   2. The single post-drop release is what wakes the distributor, so its next pass reaps
        //      the now-dead slot (recovering the parked acquire's bytes) AND runs `drop_writer_alloc`
        //      (which must release the held remainder). Without a post-drop wake the dead slot is
        //      never reaped and the pool can't reach quiescence.
        // Once quiescent, `available == capacity` iff the held remainder was released; a leak leaves
        // it short by exactly `remainder`.
        let pool_for_pusher = pool.clone();
        let dropped_pusher = dropped.clone();
        async move {
            let init = pusher.recv_frames().await;
            let mut held = sum_flow_credits(&init);
            pusher.push_max_data(VarInt::from_u32(1 << 20));

            // Accumulate every emitted frame's credit without releasing, until the writer is dropped.
            while !dropped_pusher.load(std::sync::atomic::Ordering::Relaxed) {
                if let Some(batch) = pusher.recv_frames_timeout(Duration::from_millis(1)).await {
                    held += sum_flow_credits(&batch);
                }
                bach::task::yield_now().await;
            }
            // Drain anything emitted right before the drop, then release everything at once. This
            // release wakes the distributor to reap the dead slot and fold all returned credit back.
            if let Some(batch) = pusher.recv_frames_timeout(Duration::from_millis(1)).await {
                held += sum_flow_credits(&batch);
            }
            pool_for_pusher.release(held);
        }
        .primary()
        .spawn();

        let pool_for_assert = pool.clone();
        async move {
            // Drive the write until it confirms, frames the chunk it can afford, and parks holding
            // the sub-chunk remainder.
            let mut payload = Data::new(payload_len as u64);
            for _ in 0..16 {
                let done = core::future::poll_fn(|cx| {
                    let slot = writer.0.slot_ptr();
                    match writer.0.poll_write_msg(
                        cx,
                        slot,
                        &mut payload,
                        MsgFlags {
                            is_fin: true,
                            is_wakeup: true,
                        },
                    ) {
                        Poll::Ready(r) => {
                            r.expect("write_msg poll failed");
                            Poll::Ready(true)
                        }
                        Poll::Pending => Poll::Ready(false),
                    }
                })
                .await;
                if done {
                    break;
                }
                bach::task::yield_now().await;
            }

            // Precondition: the writer is parked (slot LINKED) and still holds credit. If either is
            // false the test isn't exercising the leak path.
            assert!(
                writer.0.pending_credits > 0,
                "test precondition: writer must be holding unconsumed credit at drop, got 0"
            );
            let slot = writer.0.slot_ptr();
            assert!(
                unsafe { slot.as_ref() }.is_linked(),
                "test precondition: writer's slot must be parked (LINKED) at drop"
            );

            // Drop while parked & holding credit → LINKED→DEAD abandon → the distributor reaps the
            // dead slot via `drop_writer_alloc`, which must release the held remainder.
            drop(writer);
            dropped.store(true, std::sync::atomic::Ordering::Relaxed);

            // Let the pusher observe the drop, release all its held credit (waking the distributor),
            // and the distributor reap the dead slot and fold all returned credit back into
            // `available`. The pusher polls on a 1ms `recv_frames_timeout`, so advance real
            // simulated time (not zero-time `yield_now`) to let that timeout elapse and the release
            // + reap run to quiescence.
            bach::time::sleep(Duration::from_millis(50)).await;

            assert_eq!(
                pool_for_assert.debug_available(),
                capacity as i64,
                "pool must fully recover after a parked writer holding credit is dropped; \
                 a short result means the held pending_credits leaked \
                 (regression in drop_writer_alloc)"
            );
        }
        .primary()
        .spawn();
    });
}

/// `set_priority` updates the writer's priority field and is reflected by the `priority()` getter.
#[test]
fn priority_getter_reflects_set_priority() {
    let (mut writer, _pusher) = PairBuilder::default()
        .with_priority(crate::credit::Priority::Low)
        .build();
    assert_eq!(writer.priority(), crate::credit::Priority::Low);
    writer.set_priority(crate::credit::Priority::High);
    assert_eq!(writer.priority(), crate::credit::Priority::High);
}

/// Inline `WakerSink` that wakes every batched waker immediately. Mirrors the production sink's
/// contract: after `append_wakers` returns, the batch must be empty.
struct TestWakerSink;

impl crate::credit::WakerSink for TestWakerSink {
    fn append_wakers(&mut self, batch: &mut std::collections::VecDeque<core::task::Waker>) {
        for w in batch.drain(..) {
            w.wake();
        }
    }
}

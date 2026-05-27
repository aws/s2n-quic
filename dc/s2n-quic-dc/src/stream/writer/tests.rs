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
    testing::sim,
};
use bach::{ext::*, time::timeout};
use bytes::Bytes;
use s2n_quic_core::{endpoint, varint::VarInt};
use std::{net::SocketAddr, task::Poll, time::Duration};

// ─── Test helpers ─────────────────────────────────────────────────────────────

struct PairBuilder {
    ep_type: endpoint::Type,
}

impl Default for PairBuilder {
    fn default() -> Self {
        Self {
            ep_type: endpoint::Type::Client,
        }
    }
}

impl PairBuilder {
    fn server() -> Self {
        Self {
            ep_type: endpoint::Type::Server,
        }
    }

    fn client_no_remote_queue_id() -> Self {
        Self::default()
    }

    fn build(self) -> (Writer, Pusher) {
        let acceptor_id = VarInt::from_u8(7);
        let peer: SocketAddr = "127.0.0.1:4433".parse().unwrap();
        let path_secret_entry = PathSecretEntry::builder(peer)
            .endpoint_type(self.ep_type)
            .build();

        let client_state =
            std::sync::Arc::new(crate::queue::ClientState::new(VarInt::from_u16(100)));
        let dest_queue_id = client_state.peer_free.try_alloc().unwrap();
        let alloc = client_state.alloc_local(dest_queue_id).unwrap();
        let dispatcher = crate::queue::ClientDispatch::new(client_state);

        let queue_id = alloc.control.queue_id();
        let binding_id = alloc.control.binding_id();

        let (frame_tx, frame_rx) = frame::submission_channel(1);

        let writer = match self.ep_type {
            endpoint::Type::Client => Writer::new_client(
                frame_tx,
                path_secret_entry,
                dest_queue_id,
                acceptor_id,
                alloc.control,
            ),
            endpoint::Type::Server => Writer::new_server(
                frame_tx,
                path_secret_entry,
                dest_queue_id,
                acceptor_id,
                alloc.control,
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
            let first = pusher.recv_frames().await;
            assert_eq!(first.len(), 1, "expected exactly one frame");
            let first_frame = first.front().unwrap();
            assert!(matches!(
                first_frame.header,
                Header::QueueData {
                    is_fin: false,
                    offset,
                    ..
                } if offset == VarInt::ZERO
            ));
            assert_eq!(first_frame.payload, &b"a"[..]);

            let extra = pusher.recv_frames_timeout(Duration::from_millis(100)).await;
            assert!(
                extra.is_none(),
                "expected no frame while remote flow budget is exhausted"
            );

            pusher.push_max_data(VarInt::from_u8(2));

            let second = pusher.recv_frames().await;
            assert_eq!(second.len(), 1, "expected exactly one frame");
            let second_frame = second.front().unwrap();
            assert!(matches!(
                second_frame.header,
                Header::QueueData {
                    is_fin: true,
                    offset,
                    ..
                } if offset == VarInt::from_u8(1)
            ));
            assert_eq!(second_frame.payload, &b"b"[..]);
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

/// Local inflight budget (max_inflight_bytes) caps how much data the writer
/// can send before completions free up space.
#[test]
fn server_local_inflight_budget_blocks_write() {
    sim(|| {
        let (mut writer, mut pusher) = make_server_pair();
        writer.0.max_inflight_bytes = 3;

        async move {
            let first = pusher.recv_frames().await;
            pusher.assembler.push_queue_data(&first);
            pusher.assembler.assert_payload(b"abc");

            // Writer is blocked — no more frames
            let extra = pusher.recv_frames_timeout(Duration::from_millis(100)).await;
            assert!(
                extra.is_none(),
                "writer should block when local inflight budget is exhausted"
            );

            // Free up budget via completions
            pusher.complete_all(first, frame::TransmissionStatus::Acknowledged);

            // Writer unblocks and sends more data
            let second = pusher.recv_frames().await;
            pusher.assembler.push_queue_data(&second);
            pusher.assembler.assert_payload(b"abcdef");
        }
        .primary()
        .spawn();

        async move {
            let mut payload = Bytes::from_static(b"abcdef");
            let first = writer.write_from(&mut payload).await.expect("first write");
            assert_eq!(first, 3, "capped by local inflight budget");

            let second = writer.write_from(&mut payload).await.expect("second write");
            assert_eq!(second, 3, "freed budget allows more data");
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
